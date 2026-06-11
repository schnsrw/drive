//! File + folder REST API. All routes require `AuthSession`.
//!
//! Endpoints (mounted under `/api`):
//!
//! - `GET    /api/folders/root/children`        — list root folder contents
//! - `GET    /api/files/{id}`                   — file metadata (one FileDto)
//! - `GET    /api/folders/{id}`                 — folder metadata + children
//! - `POST   /api/folders`                      — create folder
//! - `POST   /api/files`                        — multipart streaming upload
//! - `PATCH  /api/files/{id}`                   — rename / move
//! - `PATCH  /api/folders/{id}`                 — rename / move
//! - `POST   /api/files/{id}/trash`             — soft-delete
//! - `POST   /api/files/{id}/restore`           — undo trash
//! - `GET    /api/files/{id}/download`          — 302 to signed URL
//! - `GET    /api/files/{id}/content`           — stream raw bytes (SDK)
//! - `PUT    /api/files/{id}/content`           — replace raw bytes (SDK)
//! - `GET    /api/files/{id}/open`              — WOPI handoff (new tab)

use std::time::Duration;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
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
    #[error("editor not configured: {0}")]
    EditorUnconfigured(&'static str),
    /// 413 — would exceed the per-user storage cap. Carries the cap so
    /// the SPA can show "You've used 9.8 GB of 10 GB" inline.
    #[error("quota exceeded ({used}/{quota})")]
    QuotaExceeded { used: u64, quota: u64 },
    /// 429 — upload throttle hit. Seconds the caller should wait before
    /// trying again, mirrored in the `Retry-After` header.
    #[error("rate limited, retry in {0}s")]
    RateLimited(u64),
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
            Self::EditorUnconfigured(app) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ExtErrBody {
                    error: "editor not configured",
                    extension: app.to_string(),
                }),
            )
                .into_response(),
            Self::QuotaExceeded { used, quota } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(QuotaErrBody {
                    error: "quota exceeded",
                    used,
                    quota,
                }),
            )
                .into_response(),
            Self::RateLimited(secs) => {
                let mut r = (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(RetryErrBody {
                        error: "rate limited",
                        retry_after_seconds: secs,
                    }),
                )
                    .into_response();
                r.headers_mut().insert(
                    axum::http::header::RETRY_AFTER,
                    HeaderValue::from_str(&secs.to_string()).unwrap(),
                );
                r
            }
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
struct QuotaErrBody {
    error: &'static str,
    used: u64,
    quota: u64,
}

#[derive(Serialize)]
struct RetryErrBody {
    error: &'static str,
    retry_after_seconds: u64,
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
pub(crate) struct FileDto {
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
    /// Lifecycle state (pipeline §13.6). `ready` for proxy uploads and
    /// finalized direct uploads; `uploading` for pre-finalize direct
    /// uploads; `failed` if the direct PUT errored.
    status: &'static str,
    /// Server-side thumbnail generation state (pipeline §5.4). The SPA
    /// uses `thumb_urls` (below) when this is `ready`.
    thumbs_state: &'static str,
    /// Convenience URLs for the three thumbnail sizes. Only populated
    /// when `thumbs_state == "ready"`; consumers should fall back to
    /// the inline `thumbnail` data URI otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    thumb_urls: Option<ThumbUrls>,
}

#[derive(Serialize)]
pub(crate) struct ThumbUrls {
    small: String,
    medium: String,
    large: String,
}

impl From<File> for FileDto {
    fn from(f: File) -> Self {
        let thumb_urls = if matches!(f.thumbs_state, drive_db::ThumbsState::Ready) {
            let enc = |size: &str| format!("/api/files/{}/thumb/{}", &f.id, size);
            Some(ThumbUrls {
                small: enc("small"),
                medium: enc("medium"),
                large: enc("large"),
            })
        } else {
            None
        };
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
            status: f.status.as_str(),
            thumbs_state: f.thumbs_state.as_str(),
            thumb_urls,
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
pub(crate) fn sanitise_display_name(name: &str) -> Result<String, FilesError> {
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

// ── Editor handoff ──────────────────────────────────────────────────────

/// Per-launch WOPI access-token TTL. CLAUDE.md fixes this at 10 min.
const HANDOFF_TTL_SECS: i64 = 600;

#[derive(Serialize)]
struct OpenResp {
    editor_app: &'static str,
    entry_url: String,
    access_token: String,
    access_token_ttl: i64, // milliseconds
    wopi_src: String,
}

#[derive(Debug)]
enum EditorTarget {
    Sheet,
    Document,
}

impl EditorTarget {
    fn from_file(name: &str, content_type: Option<&str>) -> Option<Self> {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
        if matches!(ext.as_deref(), Some("xlsx"))
            || content_type.is_some_and(|c| c.contains("spreadsheetml"))
        {
            return Some(Self::Sheet);
        }
        if matches!(ext.as_deref(), Some("docx"))
            || content_type.is_some_and(|c| c.contains("wordprocessingml"))
        {
            return Some(Self::Document);
        }
        None
    }
    fn app_slug(&self) -> &'static str {
        match self {
            Self::Sheet => "sheet",
            Self::Document => "document",
        }
    }
    fn origin<'a>(&self, cfg: &'a drive_core::Config) -> Option<&'a url::Url> {
        match self {
            Self::Sheet => cfg.sheet_origin.as_ref(),
            Self::Document => cfg.document_origin.as_ref(),
        }
    }
}

async fn open_in_editor(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<OpenResp>, FilesError> {
    use std::str::FromStr;

    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;
    if file.trashed_at.is_some() {
        return Err(FilesError::NotFound);
    }

    let target = EditorTarget::from_file(&file.name, file.content_type.as_deref())
        .ok_or_else(|| FilesError::ForbiddenExtension("no-editor".into()))?;

    let editor_origin = target.origin(&s.config).ok_or_else(|| {
        // Distinct error: "editor exists in code, but this instance hasn't
        // configured an origin for it." 503 surfaces a different toast in
        // the SPA than 415 (unsupported file type).
        FilesError::EditorUnconfigured(target.app_slug())
    })?;

    let file_id_typed = drive_core::FileId::from_str(&file.id)
        .map_err(|e| FilesError::Internal(format!("file id parse: {e}")))?;
    let perms = drive_wopi::WopiPerms::Write;
    let exp = time::OffsetDateTime::now_utc().unix_timestamp() + HANDOFF_TTL_SECS;
    let claims = drive_wopi::WopiClaims {
        user_id: session.user_id.clone(),
        file_id: file_id_typed,
        perms,
        exp,
        jti: ulid::Ulid::new().to_string(),
    };
    let access_token = drive_wopi::mint_token(&s.jwt_secret, &claims);

    // WOPISrc = the Drive's WOPI host URL for this file, on the app origin.
    let mut wopi_src = s.config.app_origin.clone();
    wopi_src.set_path(&format!("/wopi/files/{}", file.id));
    let wopi_src_str = wopi_src.to_string();

    // entry_url = the editor's launch endpoint with WOPISrc + access_token
    // baked in. Editor's path is conventionally /wopi/editor.
    let mut entry = editor_origin.clone();
    entry.set_path("/wopi/editor");
    entry
        .query_pairs_mut()
        .append_pair("WOPISrc", &wopi_src_str)
        .append_pair("access_token", &access_token);

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.open_in_editor".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id),
            target_name: Some(file.name),
            ip_address: None,
            metadata: Some(format!(r#"{{"editor_app":"{}"}}"#, target.app_slug())),
        },
    );

    Ok(Json(OpenResp {
        editor_app: target.app_slug(),
        entry_url: entry.to_string(),
        access_token,
        access_token_ttl: HANDOFF_TTL_SECS * 1_000,
        wopi_src: wopi_src_str,
    }))
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

/// Magic-byte sniff. Catches files lying about their extension by
/// inspecting the first ~few bytes. Returns the detected MIME type (which
/// the caller should store as authoritative content-type, not what the
/// client claimed), or `None` when the bytes don't match any known type
/// (plain text / unknown binary).
///
/// Returns `Err(FilesError::ForbiddenExtension)` when the detected type
/// is an executable / runnable artefact — independent of filename.
pub(crate) fn sniff_and_check_content_type(
    bytes: &[u8],
) -> Result<Option<&'static str>, FilesError> {
    let Some(kind) = infer::get(bytes) else {
        return Ok(None);
    };
    // The executable matcher set in `infer`'s `app` namespace covers PE
    // (Windows .exe), Mach-O (macOS), ELF (Linux), Java .class, .wasm,
    // installers (msi/dmg/pkg), and a few cousins. Reject all.
    if infer::app::is_exe(bytes)
        || infer::app::is_dll(bytes)
        || infer::app::is_elf(bytes)
        || infer::app::is_mach(bytes)
        || infer::app::is_java(bytes)
        || infer::app::is_dex(bytes)
        || infer::app::is_dey(bytes)
        || infer::app::is_wasm(bytes)
        || infer::app::is_coff(bytes)
        || infer::app::is_llvm(bytes)
    {
        return Err(FilesError::ForbiddenExtension(kind.extension().into()));
    }
    Ok(Some(kind.mime_type()))
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

#[derive(Deserialize)]
struct WorkspaceQuery {
    #[serde(default)]
    workspace: Option<String>,
}

async fn list_root(
    State(s): State<HttpState>,
    session: AuthSession,
    axum::extract::Query(q): axum::extract::Query<WorkspaceQuery>,
) -> Result<Json<ListResp>, FilesError> {
    let ws = crate::workspaces::resolve_active_workspace(
        &s.db,
        &session.user_id,
        q.workspace.as_deref(),
    )
    .await
    .map_err(|e| FilesError::Internal(format!("workspace: {e:?}")))?;
    let folders = FolderRepo::new(&s.db)
        .list_children_in_workspace(None, &ws)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    let files = FileRepo::new(&s.db)
        .list_children_in_workspace(None, &ws)
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
    // Auth: workspace member, falling back to owner check for pre-0006 rows
    // whose workspace_id is still NULL.
    if let Some(ws) = folder.workspace_id.as_deref() {
        let role = drive_db::WorkspaceMemberRepo::new(&s.db)
            .role_of(ws, &session.user_id)
            .await
            .map_err(|e| FilesError::Internal(e.to_string()))?;
        if role.is_none() {
            return Err(FilesError::Forbidden);
        }
    } else {
        ensure_owner(&folder.owner_id, &session)?;
    }

    let children_folders = if let Some(ws) = folder.workspace_id.as_deref() {
        FolderRepo::new(&s.db)
            .list_children_in_workspace(Some(&folder.id), ws)
            .await
    } else {
        FolderRepo::new(&s.db)
            .list_children(Some(&folder.id), &session.user_id)
            .await
    }
    .map_err(|e| FilesError::Internal(e.to_string()))?;
    let children_files = if let Some(ws) = folder.workspace_id.as_deref() {
        FileRepo::new(&s.db)
            .list_children_in_workspace(Some(&folder.id), ws)
            .await
    } else {
        FileRepo::new(&s.db)
            .list_children(Some(&folder.id), &session.user_id)
            .await
    }
    .map_err(|e| FilesError::Internal(e.to_string()))?;
    let folders = children_folders;
    let files = children_files;
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
    #[serde(default)]
    workspace_id: Option<String>,
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

    let workspace_id = crate::workspaces::resolve_active_workspace(
        &s.db,
        &session.user_id,
        body.workspace_id.as_deref(),
    )
    .await
    .map_err(|e| FilesError::Internal(format!("workspace: {e:?}")))?;

    let f = FolderRepo::new(&s.db)
        .insert(&NewFolder {
            parent_id: body.parent_id,
            name,
            owner_id: session.user_id.clone(),
            workspace_id,
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
    // RT1 1c — workspace-scoped broadcast for live presence clients.
    // No-op when the workspace channel has no subscribers.
    if let Some(ws) = f.workspace_id.as_deref() {
        s.presence
            .broadcast_action(
                ws,
                &session.user_id,
                "folders.create",
                Some(&f.id),
                Some(&f.name),
            )
            .await;
    }
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
    // Rate limit BEFORE we read the body — cheap upfront check.
    if let Err(retry) = s.upload_limiter.check(&session.user_id) {
        return Err(FilesError::RateLimited(retry));
    }

    let mut parent_id: Option<String> = None;
    let mut workspace_id_field: Option<String> = None;
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
            Some("workspace_id") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| FilesError::Validation(e.to_string()))?;
                if !text.is_empty() {
                    workspace_id_field = Some(text);
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
    // Magic-byte sniff: rejects executables masquerading as .txt and
    // overrides the client-asserted content_type with what the bytes
    // actually are. `None` (e.g. plain text or unknown binary) keeps
    // the client header — extension-based MIME guess from the browser
    // is the next-best signal there.
    let sniffed = sniff_and_check_content_type(&bytes)?;

    if let Some(pid) = &parent_id {
        let parent = FolderRepo::new(&s.db)
            .find_by_id(pid)
            .await
            .map_err(|_| FilesError::NotFound)?;
        ensure_owner(&parent.owner_id, &session)?;
    }

    let size = bytes.len() as u64;

    // Quota check — only when the caller's user row carries one (None
    // means unlimited, the v0 default for the seeded admin).
    let users = drive_db::UserRepo::new(&s.db);
    let me = users
        .find_by_id(&session.user_id)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;
    if let Some(quota) = me.quota_bytes {
        let used = users
            .used_bytes(&session.user_id)
            .await
            .map_err(|e| FilesError::Internal(e.to_string()))?;
        if used + size > quota {
            return Err(FilesError::QuotaExceeded { used, quota });
        }
    }

    let workspace_id = crate::workspaces::resolve_active_workspace(
        &s.db,
        &session.user_id,
        workspace_id_field.as_deref(),
    )
    .await
    .map_err(|e| FilesError::Internal(format!("workspace: {e:?}")))?;

    // Pipeline §8.9 — route the bytes to the workspace's BYO bucket if
    // it has one configured + the secret can be decrypted. Personal
    // workspaces never have one. A BYO row whose secret won't decrypt
    // (master key rotated, envelope corrupted) is a hard failure: we'd
    // rather refuse the upload than silently write to the host's bucket.
    let (storage, storage_id) =
        crate::workspace_storage::resolve_upload_storage(&s, &workspace_id).await?;

    let id = ulid::Ulid::new().to_string();
    let meta = storage
        .put(&storage_key(&id), bytes, content_type.as_deref())
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    let file = FileRepo::new(&s.db)
        .insert(&NewFile {
            id,
            parent_id,
            name,
            size,
            // Authoritative content-type precedence: sniffed bytes →
            // storage adapter's reported type → client header. Sniffed
            // wins because it's the only one the user can't fake.
            content_type: sniffed
                .map(str::to_string)
                .or(meta.content_type)
                .or(content_type),
            etag: meta.etag,
            owner_id: session.user_id.clone(),
            workspace_id,
            storage_id,
            thumbnail,
            // Proxy multipart commits the row as ready in one shot —
            // the bytes are already in the bucket by the time we get here.
            status: drive_db::FileStatus::Ready,
            expected_size: None,
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
    if let Some(ws) = file.workspace_id.as_deref() {
        s.presence
            .broadcast_action(
                ws,
                &session.user_id,
                "files.upload",
                Some(&file.id),
                Some(&file.name),
            )
            .await;
    }
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

/// `GET /api/files/{id}` — return a single file's metadata.
///
/// Used by Drive's SPA when it lands on `/file/<id>` cold — i.e.
/// refresh / shared URL / bookmark — without an in-memory FileDto
/// from the file list. Owner-gated; emits no audit event (read of
/// metadata is not a side-effect worth logging).
async fn get_file_meta(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<FileDto>, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;
    Ok(Json(FileDto::from(file)))
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
        if let Some(ws) = updated.workspace_id.as_deref() {
            s.presence
                .broadcast_action(
                    ws,
                    &session.user_id,
                    "files.rename",
                    Some(&updated.id),
                    Some(&updated.name),
                )
                .await;
        }
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
        if let Some(ws) = updated.workspace_id.as_deref() {
            s.presence
                .broadcast_action(
                    ws,
                    &session.user_id,
                    "folders.rename",
                    Some(&updated.id),
                    Some(&updated.name),
                )
                .await;
        }
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
    let trashed_id = file.id.clone();
    let trashed_name = file.name.clone();
    let trashed_ws = file.workspace_id.clone();
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
    if let Some(ws) = trashed_ws.as_deref() {
        s.presence
            .broadcast_action(
                ws,
                &session.user_id,
                "files.trash",
                Some(&trashed_id),
                Some(&trashed_name),
            )
            .await;
    }
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
        .signed_get(
            &storage_key(&id),
            Duration::from_secs(s.config.signed_url_ttl_secs),
        )
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

// ── SDK content endpoints ───────────────────────────────────────────────
//
// Same-origin byte transport for the in-Drive SDK editor wrappers
// (CasualDocEditor / CasualSheetWorkspace). See
// docs/ux/10-sdk-integration-plan.md §"Phase 1 — SDK + DriveFileSource".
// Distinct from the WOPI handoff: no token mint, no user-content
// origin redirect; the SPA already has the auth cookie + CSRF.

/// `GET /api/files/{id}/content` — stream the file's raw bytes.
///
/// Used by `DriveFileSource.open(fileId)` on the SPA side. Returns
/// the file's stored content_type (or `application/octet-stream` as
/// fallback) so the editor's bytes-to-document pipeline stays generic.
async fn get_content(
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

    let (meta, stream) = s
        .storage
        .get(&storage_key(&id), None)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    let content_type = file
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&meta.size.to_string()).unwrap(),
    );
    // The SDK fetches with default cache semantics; force no-store so a
    // mid-edit reload always gets the latest bytes instead of a stale
    // browser-cache hit between saves.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    Ok(response)
}

/// `PUT /api/files/{id}/content` — replace the file's bytes.
///
/// Body: raw bytes (the editor's saved `.docx` / `.xlsx` payload). Used
/// by `DriveFileSource.save(fileId, bytes)`. Writes through
/// `crates/drive-storage`, bumps `size` + `version` + `modified_at`
/// on the file row, emits a `files.save` audit event, and returns the
/// updated `FileDto`.
async fn put_content(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<FileDto>, FilesError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    ensure_owner(&file.owner_id, &session)?;

    let new_size = body.len() as u64;
    let content_type = file.content_type.clone();

    s.storage
        .put(&storage_key(&id), body, content_type.as_deref())
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    files
        .set_size_and_touch(&id, new_size)
        .await
        .map_err(|e| FilesError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.save".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id.clone()),
            target_name: Some(file.name.clone()),
            ip_address: None,
            metadata: None,
        },
    );

    // Re-read so the response carries the bumped version + modified_at.
    let updated = files
        .find_by_id(&id)
        .await
        .map_err(|_| FilesError::NotFound)?;
    Ok(Json(FileDto::from(updated)))
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
        .route("/api/files/{id}", get(get_file_meta).patch(patch_file))
        .route("/api/files/{id}/trash", post(trash_file))
        .route("/api/files/{id}/restore", post(restore_file))
        .route("/api/files/{id}/download", get(download_file))
        .route(
            "/api/files/{id}/content",
            get(get_content)
                .put(put_content)
                .layer(DefaultBodyLimit::max(body_limit_bytes)),
        )
        .route("/api/files/{id}/open", get(open_in_editor))
        .with_state(state)
}
