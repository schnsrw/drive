//! File + folder REST API. All routes require `AuthSession`.
//!
//! Endpoints (mounted under `/api`):
//!
//! - `GET    /api/folders/root/children`        — list root folder contents
//! - `GET    /api/folders/{id}`                 — folder metadata + children
//! - `POST   /api/folders`                      — create folder
//! - `POST   /api/files`                        — multipart streaming upload
//! - `PATCH  /api/files/{id}`                   — rename / move
//! - `PATCH  /api/folders/{id}`                 — rename / move
//! - `POST   /api/files/{id}/trash`             — soft-delete
//! - `POST   /api/files/{id}/restore`           — undo trash
//! - `GET    /api/files/{id}/download`          — 302 to signed URL

use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use bytes::Bytes;
use drive_auth::AuthSession;
use drive_db::{AuditRepo, File, FileRepo, Folder, FolderRepo, NewAuditEvent, NewFile, NewFolder};
use drive_storage::SignedUrl;
use serde::{Deserialize, Serialize};

use crate::HttpState;

#[derive(Debug, thiserror::Error)]
pub(crate) enum FilesError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("validation: {0}")]
    Validation(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("forbidden extension: {0}")]
    ForbiddenExtension(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for FilesError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => {
                (StatusCode::NOT_FOUND, Json(ErrBody { error: "not found" })).into_response()
            }
            Self::Forbidden => {
                (StatusCode::FORBIDDEN, Json(ErrBody { error: "forbidden" })).into_response()
            }
            Self::Validation(m) => {
                (StatusCode::BAD_REQUEST, Json(ErrBody { error: &m })).into_response()
            }
            Self::Conflict(m) => {
                (StatusCode::CONFLICT, Json(ErrBody { error: &m })).into_response()
            }
            Self::ForbiddenExtension(ext) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(ExtErrBody {
                    error: "file type not allowed",
                    extension: ext,
                }),
            )
                .into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "files internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrBody {
                        error: "internal error",
                    }),
                )
                    .into_response()
            }
        }
    }
}

#[derive(Serialize)]
struct ExtErrBody {
    error: &'static str,
    extension: String,
}

#[derive(Serialize)]
struct ErrBody<'a> {
    error: &'a str,
}

// ─── Responses ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ListResp {
    folders: Vec<FolderDto>,
    files: Vec<FileDto>,
}

#[derive(Serialize)]
struct FolderDto {
    id: String,
    parent_id: Option<String>,
    name: String,
    created_at: String,
    modified_at: String,
}

impl From<Folder> for FolderDto {
    fn from(f: Folder) -> Self {
        Self {
            id: f.id,
            parent_id: f.parent_id,
            name: f.name,
            created_at: rfc3339(f.created_at),
            modified_at: rfc3339(f.modified_at),
        }
    }
}

#[derive(Serialize)]
struct FileDto {
    id: String,
    parent_id: Option<String>,
    name: String,
    size: u64,
    content_type: Option<String>,
    version: u32,
    created_at: String,
    modified_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
}

impl From<File> for FileDto {
    fn from(f: File) -> Self {
        Self {
            id: f.id,
            parent_id: f.parent_id,
            name: f.name,
            size: f.size,
            content_type: f.content_type,
            version: f.version,
            created_at: rfc3339(f.created_at),
            modified_at: rfc3339(f.modified_at),
            thumbnail: f.thumbnail,
        }
    }
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// ─── Validation ─────────────────────────────────────────────────────────

/// Display-name sanitisation — minimum bar from `docs/research/06-security.md` §2.
/// (Full unicode normalisation lives in a Phase-2 helper; we keep the essentials.)
fn sanitise_display_name(name: &str) -> Result<String, FilesError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(FilesError::Validation("name cannot be empty".into()));
    }
    if trimmed.chars().count() > 255 {
        return Err(FilesError::Validation(
            "name too long (max 255 chars)".into(),
        ));
    }
    // Reject ASCII control chars, NUL, path separators, leading `.` / `-` /
    // whitespace, and a tiny Windows reserved-names blacklist.
    for c in trimmed.chars() {
        if c.is_ascii_control() || c == '\0' || c == '/' || c == '\\' {
            return Err(FilesError::Validation(format!(
                "name contains a forbidden character: {c:?}"
            )));
        }
    }
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = trimmed.split('.').next().unwrap_or(trimmed).to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(FilesError::Validation(format!(
            "name is reserved: {trimmed}"
        )));
    }
    Ok(trimmed.to_string())
}

fn ensure_owner(folder_owner: &str, session: &AuthSession) -> Result<(), FilesError> {
    if folder_owner != session.user_id {
        return Err(FilesError::Forbidden);
    }
    Ok(())
}

pub(crate) fn storage_key(file_id: &str) -> String {
    format!("files/{file_id}")
}

/// Extensions refused at upload time. See docs/ux/07-preview-surface.md
/// §"Upload restrictions". Office macro-enabled formats (.docm/.xlsm/.pptm)
/// are intentionally NOT here — per CLAUDE.md they're allowed as opaque
/// blobs and never auto-opened in an editor.
const FORBIDDEN_UPLOAD_EXTENSIONS: &[&str] = &[
    // Windows scripts / executables
    "exe", "com", "scr", "bat", "cmd", "msi", "msp", "ps1", "psm1", "vbs", "vbe", "wsf", "wsh",
    "jse", "reg", "lnk", "scf", // POSIX shells / runnable bundles
    "sh", "bash", "zsh", "fish", "csh", "ksh", "command", "app", "dmg", "pkg",
    // Runtime artefacts
    "jar", "class", "dll", "so", "dylib", // Shortcut-style files that resolve elsewhere
    "url", "desktop",
];

/// Cap for client-supplied thumbnail data URIs. 64 KB is plenty for a
/// 200×200 PNG/WebP at reasonable quality. Beyond that we drop it and the
/// SPA falls back to the procedural thumbnail.
const THUMBNAIL_MAX_BYTES: usize = 64 * 1024;

/// Cheap validation: must look like a `data:image/*;base64,…` URI and fit
/// the byte cap. We don't decode the base64 — the client owns the format
/// choice and the server treats it as opaque display metadata.
fn validate_thumbnail_uri(s: &str) -> bool {
    if s.len() > THUMBNAIL_MAX_BYTES {
        return false;
    }
    s.starts_with("data:image/") && s.contains(";base64,")
}

/// Returns `Err(FilesError::ForbiddenExtension)` if the last dotted
/// extension of `filename` is in the upload blocklist. Case-insensitive.
pub(crate) fn check_upload_extension(filename: &str) -> Result<(), FilesError> {
    let lower = filename.to_ascii_lowercase();
    let Some(idx) = lower.rfind('.') else {
        return Ok(());
    };
    let ext = &lower[idx + 1..];
    if ext.is_empty() {
        return Ok(());
    }
    if FORBIDDEN_UPLOAD_EXTENSIONS.contains(&ext) {
        return Err(FilesError::ForbiddenExtension(ext.to_string()));
    }
    Ok(())
}

// ─── Handlers ───────────────────────────────────────────────────────────

async fn list_root(
    State(s): State<HttpState>,
    session: AuthSession,
) -> Result<Json<ListResp>, FilesError> {
    let folders = FolderRepo::new(&s.db)
        .list_children(None, &session.user_id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    let files = FileRepo::new(&s.db)
        .list_children(None, &session.user_id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    Ok(Json(ListResp {
        folders: folders.into_iter().map(FolderDto::from).collect(),
        files: files.into_iter().map(FileDto::from).collect(),
    }))
}

#[derive(Serialize)]
struct FolderDetail {
    folder: FolderDto,
    children: ListResp,
}

async fn get_folder(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<FolderDetail>, FilesError> {
    let folder = FolderRepo::new(&s.db)
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&folder.owner_id, &session)?;

    let folders = FolderRepo::new(&s.db)
        .list_children(Some(&folder.id), &session.user_id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    let files = FileRepo::new(&s.db)
        .list_children(Some(&folder.id), &session.user_id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    Ok(Json(FolderDetail {
        folder: folder.into(),
        children: ListResp {
            folders: folders.into_iter().map(FolderDto::from).collect(),
            files: files.into_iter().map(FileDto::from).collect(),
        },
    }))
}

#[derive(Deserialize)]
struct CreateFolderBody {
    parent_id: Option<String>,
    name: String,
}

async fn create_folder(
    State(s): State<HttpState>,
    session: AuthSession,
    Json(body): Json<CreateFolderBody>,
) -> Result<Json<FolderDto>, FilesError> {
    let name = sanitise_display_name(&body.name)?;

    if let Some(pid) = &body.parent_id {
        let parent = FolderRepo::new(&s.db)
            .find_by_id(pid)
            .await
            .map_err(|_| FilesError::NotFound)?;
        ensure_owner(&parent.owner_id, &session)?;
    }

    let f = FolderRepo::new(&s.db)
        .insert(&NewFolder {
            parent_id: body.parent_id,
            name,
            owner_id: session.user_id.clone(),
        })
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "folders.create".into(),
            target_kind: Some("folder".into()),
            target_id: Some(f.id.clone()),
            target_name: Some(f.name.clone()),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(Json(f.into()))
}

/// Streaming multipart upload — Phase 1 buffers the first part's bytes
/// (axum 0.8 Multipart yields chunked `Bytes` via `Field::chunk`). The
/// `parent_id` is read from the `parent_id` form field if present; the
/// file part is named `file`.
async fn upload_file(
    State(s): State<HttpState>,
    session: AuthSession,
    mut multipart: Multipart,
) -> Result<Json<FileDto>, FilesError> {
    let mut parent_id: Option<String> = None;
    let mut file_bytes: Option<Bytes> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut thumbnail: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| FilesError::Validation(format!("multipart parse: {e}")))?
    {
        match field.name() {
            Some("parent_id") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| FilesError::Validation(e.to_string()))?;
                if !text.is_empty() {
                    parent_id = Some(text);
                }
            }
            Some("file") => {
                filename = field.file_name().map(str::to_string);
                content_type = field.content_type().map(str::to_string);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| FilesError::Validation(e.to_string()))?;
                file_bytes = Some(bytes);
            }
            Some("thumbnail") => {
                // Client-generated data URI. Hard-cap server-side so a
                // misbehaving client can't blow up the metadata row.
                let text = field
                    .text()
                    .await
                    .map_err(|e| FilesError::Validation(e.to_string()))?;
                if !text.is_empty() && validate_thumbnail_uri(&text) {
                    thumbnail = Some(text);
                }
            }
            _ => {}
        }
    }

    let filename = filename
        .ok_or_else(|| FilesError::Validation("missing file field with filename".into()))?;
    let bytes = file_bytes.ok_or_else(|| FilesError::Validation("missing file bytes".into()))?;
    let name = sanitise_display_name(&filename)?;
    check_upload_extension(&name)?;

    if let Some(pid) = &parent_id {
        let parent = FolderRepo::new(&s.db)
            .find_by_id(pid)
            .await
            .map_err(|_| FilesError::NotFound)?;
        ensure_owner(&parent.owner_id, &session)?;
    }

    let id = ulid::Ulid::new().to_string();
    let size = bytes.len() as u64;
    let meta = s
        .storage
        .put(&storage_key(&id), bytes, content_type.as_deref())
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    let file = FileRepo::new(&s.db)
        .insert(&NewFile {
            id,
            parent_id,
            name,
            size,
            content_type: meta.content_type.or(content_type),
            etag: meta.etag,
            owner_id: session.user_id.clone(),
            thumbnail,
        })
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.upload".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id.clone()),
            target_name: Some(file.name.clone()),
            ip_address: None,
            metadata: Some(format!(r#"{{"size":{}}}"#, file.size)),
        },
    );
    Ok(Json(file.into()))
}

#[derive(Deserialize)]
struct PatchBody {
    #[serde(default)]
    name: Option<String>,
    /// `None` means "no change"; `Some(None)` is JSON `null` → move to root.
    #[serde(default, deserialize_with = "deser_optional_string")]
    parent_id: Option<Option<String>>,
}

fn deser_optional_string<'de, D>(d: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Option<String>>::deserialize(d)?;
    Ok(opt)
}

async fn patch_file(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<PatchBody>,
) -> Result<Json<FileDto>, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;

    let mut renamed = false;
    if let Some(name) = body.name {
        let sane = sanitise_display_name(&name)?;
        files
            .rename(&id, &sane)
            .await
            .map_err(|e| FilesError::Internal(e.to_string()))?;
        renamed = true;
    }
    if let Some(parent) = body.parent_id {
        if let Some(pid) = parent.as_deref() {
            let folder = FolderRepo::new(&s.db)
                .find_by_id(pid)
                .await
                .map_err(|_| FilesError::NotFound)?;
            ensure_owner(&folder.owner_id, &session)?;
            files
                .move_to(&id, Some(pid))
                .await
                .map_err(|e| FilesError::Internal(e.to_string()))?;
        } else {
            files
                .move_to(&id, None)
                .await
                .map_err(|e| FilesError::Internal(e.to_string()))?;
        }
    }
    let updated = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    if renamed {
        AuditRepo::emit(
            &s.db,
            NewAuditEvent {
                actor_id: Some(session.user_id.clone()),
                actor_username: Some(session.username.clone()),
                action: "files.rename".into(),
                target_kind: Some("file".into()),
                target_id: Some(updated.id.clone()),
                target_name: Some(updated.name.clone()),
                ip_address: None,
                metadata: None,
            },
        );
    }
    Ok(Json(updated.into()))
}

async fn patch_folder(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<PatchBody>,
) -> Result<Json<FolderDto>, FilesError> {
    let repo = FolderRepo::new(&s.db);
    let folder = repo
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&folder.owner_id, &session)?;

    let mut renamed_folder = false;
    if let Some(name) = body.name {
        let sane = sanitise_display_name(&name)?;
        repo.rename(&id, &sane)
            .await
            .map_err(|e| FilesError::Internal(e.to_string()))?;
        renamed_folder = true;
    }
    if let Some(parent) = body.parent_id {
        if let Some(pid) = parent.as_deref() {
            // Don't allow moving a folder into itself or under its own descendant.
            if pid == id {
                return Err(FilesError::Conflict(
                    "cannot move folder into itself".into(),
                ));
            }
            let new_parent = repo
                .find_by_id(pid)
                .await
                .map_err(|_| FilesError::NotFound)?;
            ensure_owner(&new_parent.owner_id, &session)?;
            repo.move_to(&id, Some(pid))
                .await
                .map_err(|e| FilesError::Internal(e.to_string()))?;
        } else {
            repo.move_to(&id, None)
                .await
                .map_err(|e| FilesError::Internal(e.to_string()))?;
        }
    }
    let updated = repo
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    if renamed_folder {
        AuditRepo::emit(
            &s.db,
            NewAuditEvent {
                actor_id: Some(session.user_id.clone()),
                actor_username: Some(session.username.clone()),
                action: "folders.rename".into(),
                target_kind: Some("folder".into()),
                target_id: Some(updated.id.clone()),
                target_name: Some(updated.name.clone()),
                ip_address: None,
                metadata: None,
            },
        );
    }
    Ok(Json(updated.into()))
}

async fn trash_file(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;
    files
        .trash(&id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.trash".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id),
            target_name: Some(file.name),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_file(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;
    files
        .restore(&id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.restore".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id),
            target_name: Some(file.name),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn download_file(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Response, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.download".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id.clone()),
            target_name: Some(file.name.clone()),
            ip_address: None,
            metadata: None,
        },
    );

    let signed = s
        .storage
        .signed_get(&storage_key(&id), Duration::from_secs(300))
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    let target = match signed {
        SignedUrl::Native { url, .. } => url.to_string(),
        SignedUrl::Token { token, .. } => {
            // Send the user to the user-content origin's /raw/{token} handler.
            let mut base = s.config.usercontent_origin.clone();
            base.set_path(&format!("/raw/{token}"));
            base.to_string()
        }
    };

    let mut r = (StatusCode::FOUND, ()).into_response();
    r.headers_mut()
        .insert(header::LOCATION, HeaderValue::from_str(&target).unwrap());
    Ok(r)
}

pub(crate) fn router(state: HttpState, body_limit_bytes: usize) -> Router {
    Router::new()
        .route("/api/folders/root/children", get(list_root))
        .route("/api/folders/{id}", get(get_folder).patch(patch_folder))
        .route("/api/folders", post(create_folder))
        .route(
            "/api/files",
            post(upload_file).layer(DefaultBodyLimit::max(body_limit_bytes)),
        )
        .route("/api/files/{id}", patch(patch_file))
        .route("/api/files/{id}/trash", post(trash_file))
        .route("/api/files/{id}/restore", post(restore_file))
        .route("/api/files/{id}/download", get(download_file))
        .with_state(state)
}
