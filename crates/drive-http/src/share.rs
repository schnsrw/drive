//! Share-link API + recipient resolution.
//!
//! Owner endpoints (authed, owner-only):
//!   - POST   /api/files/{id}/share        — mint a share link
//!   - GET    /api/files/{id}/shares       — list shares for one file
//!   - DELETE /api/shares/{id}             — revoke
//!
//! Public endpoints (no auth — protected by token + optional password):
//!   - POST   /api/share/{token}           — resolve metadata; password
//!     check happens here
//!   - GET    /api/share/{token}/download  — 302 to a signed download URL
//!
//! Spec: docs/ux/05-sharing-surface.md.

use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::Engine as _;
use drive_auth::{hash_password, verify_password, AuthSession};
use drive_db::{
    AuditRepo, FileRepo, FolderRepo, NewAuditEvent, NewShareLink, ShareLink, ShareLinkRepo,
};
use drive_storage::SignedUrl;
use serde::{Deserialize, Serialize};

use crate::{files::storage_key, HttpState};

// ── Public DTOs ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ShareDto {
    pub id: String,
    pub token: String,
    pub url: String,
    pub permissions: String,
    pub has_password: bool,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub last_accessed_at: Option<String>,
    pub access_count: i64,
}

impl ShareDto {
    fn from_link(link: &ShareLink, app_origin: &url::Url) -> Self {
        let mut url = app_origin.clone();
        url.set_path(&format!("/s/{}", link.token));
        Self {
            id: link.id.clone(),
            token: link.token.clone(),
            url: url.to_string(),
            permissions: link.permissions.clone(),
            has_password: link.password_hash.is_some(),
            expires_at: link.expires_at.map(rfc3339),
            created_at: rfc3339(link.created_at),
            last_accessed_at: link.last_accessed_at.map(rfc3339),
            access_count: link.access_count,
        }
    }
}

// ── Owner-side handlers ────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct CreateShareBody {
    pub permissions: Option<String>,
    pub password: Option<String>,
    pub expires_in_seconds: Option<i64>,
}

async fn create_share(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(file_id): Path<String>,
    Json(body): Json<CreateShareBody>,
) -> Result<(StatusCode, Json<ShareDto>), ShareError> {
    let perms = body.permissions.as_deref().unwrap_or("view");
    if perms != "view" {
        // Edit permission is reserved for v0.2 — when the WOPI handoff for
        // recipients lands. Reject loudly so the SPA can't silently regress.
        return Err(ShareError::Validation(
            "only 'view' permissions ship in v0".into(),
        ));
    }

    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&file_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if file.owner_id != session.user_id {
        return Err(ShareError::Forbidden);
    }

    let password_hash = match body
        .password
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(p) if p.chars().count() < 4 => {
            return Err(ShareError::Validation(
                "share password must be at least 4 characters".into(),
            ));
        }
        Some(p) => Some(hash_password(p).map_err(|e| ShareError::Internal(e.to_string()))?),
        None => None,
    };

    let expires_at = body
        .expires_in_seconds
        .and_then(|secs| if secs > 0 { Some(secs) } else { None })
        .map(|secs| time::OffsetDateTime::now_utc() + time::Duration::seconds(secs));

    let token = mint_token();
    let file_name = file.name.clone();
    let link = ShareLinkRepo::new(&s.db)
        .insert(&NewShareLink {
            token,
            file_id: Some(file.id.clone()),
            folder_id: None,
            password_hash,
            permissions: perms.to_string(),
            expires_at,
            created_by: session.user_id.clone(),
        })
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "share.create".into(),
            target_kind: Some("share_link".into()),
            target_id: Some(link.id.clone()),
            target_name: Some(file_name),
            ip_address: None,
            metadata: Some(format!(
                r#"{{"file_id":"{}","has_password":{}}}"#,
                file.id,
                link.password_hash.is_some()
            )),
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(ShareDto::from_link(&link, &s.config.app_origin)),
    ))
}

#[derive(Serialize)]
struct ListShares {
    shares: Vec<ShareDto>,
}

async fn list_shares(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(file_id): Path<String>,
) -> Result<Json<ListShares>, ShareError> {
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(&file_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if file.owner_id != session.user_id {
        return Err(ShareError::Forbidden);
    }

    let links = ShareLinkRepo::new(&s.db)
        .list_for_file(&file.id)
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;
    let shares = links
        .iter()
        .map(|l| ShareDto::from_link(l, &s.config.app_origin))
        .collect();
    Ok(Json(ListShares { shares }))
}

async fn revoke_share(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(share_id): Path<String>,
) -> Result<StatusCode, ShareError> {
    let repo = ShareLinkRepo::new(&s.db);
    let link = repo
        .find_by_id(&share_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if link.created_by != session.user_id {
        // Anti-enumeration: present non-owners with the same 404 as
        // missing links rather than 403 so they can't probe existence.
        return Err(ShareError::NotFound);
    }
    // Look up the target name (denormalised so the audit row survives
    // file/folder deletion). Best-effort — missing target just yields None.
    let target_name = if let Some(fid) = link.file_id.as_deref() {
        FileRepo::new(&s.db)
            .find_by_id(fid)
            .await
            .ok()
            .map(|f| f.name)
    } else if let Some(fid) = link.folder_id.as_deref() {
        FolderRepo::new(&s.db)
            .find_by_id(fid)
            .await
            .ok()
            .map(|f| f.name)
    } else {
        None
    };

    repo.delete(&share_id)
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "share.revoke".into(),
            target_kind: Some("share_link".into()),
            target_id: Some(link.id),
            target_name,
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

// ── Owner-side handlers (folder shares) ────────────────────────────────

async fn create_folder_share(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(folder_id): Path<String>,
    Json(body): Json<CreateShareBody>,
) -> Result<(StatusCode, Json<ShareDto>), ShareError> {
    let perms = body.permissions.as_deref().unwrap_or("view");
    if perms != "view" {
        return Err(ShareError::Validation(
            "only 'view' permissions ship in v0".into(),
        ));
    }

    let folders = FolderRepo::new(&s.db);
    let folder = folders
        .find_by_id(&folder_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if folder.owner_id != session.user_id {
        return Err(ShareError::Forbidden);
    }
    if folder.trashed_at.is_some() {
        return Err(ShareError::NotFound);
    }

    let password_hash = match body
        .password
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(p) if p.chars().count() < 4 => {
            return Err(ShareError::Validation(
                "share password must be at least 4 characters".into(),
            ));
        }
        Some(p) => Some(hash_password(p).map_err(|e| ShareError::Internal(e.to_string()))?),
        None => None,
    };

    let expires_at = body
        .expires_in_seconds
        .and_then(|secs| if secs > 0 { Some(secs) } else { None })
        .map(|secs| time::OffsetDateTime::now_utc() + time::Duration::seconds(secs));

    let token = mint_token();
    let folder_name = folder.name.clone();
    let link = ShareLinkRepo::new(&s.db)
        .insert(&NewShareLink {
            token,
            file_id: None,
            folder_id: Some(folder.id.clone()),
            password_hash,
            permissions: perms.to_string(),
            expires_at,
            created_by: session.user_id.clone(),
        })
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "share.create".into(),
            target_kind: Some("share_link".into()),
            target_id: Some(link.id.clone()),
            target_name: Some(folder_name),
            ip_address: None,
            metadata: Some(format!(
                r#"{{"folder_id":"{}","has_password":{}}}"#,
                folder.id,
                link.password_hash.is_some()
            )),
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(ShareDto::from_link(&link, &s.config.app_origin)),
    ))
}

async fn list_folder_shares(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(folder_id): Path<String>,
) -> Result<Json<ListShares>, ShareError> {
    let folders = FolderRepo::new(&s.db);
    let folder = folders
        .find_by_id(&folder_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if folder.owner_id != session.user_id {
        return Err(ShareError::Forbidden);
    }

    let links = ShareLinkRepo::new(&s.db)
        .list_for_folder(&folder.id)
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;
    let shares = links
        .iter()
        .map(|l| ShareDto::from_link(l, &s.config.app_origin))
        .collect();
    Ok(Json(ListShares { shares }))
}

// ── Recipient-side handlers ────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub(crate) struct ResolveBody {
    pub password: Option<String>,
}

#[derive(Serialize)]
struct RecipientFile {
    /// `id` is needed by the recipient page to call the per-file
    /// download endpoint (`/api/share/{token}/download?file_id=…`).
    /// Safe to expose because the share token already gates access.
    id: String,
    name: String,
    size: u64,
    content_type: Option<String>,
    modified_at: String,
}

#[derive(Serialize)]
struct RecipientFolder {
    id: String,
    name: String,
    modified_at: String,
}

/// `kind: "file"` carries the legacy single-file payload; `kind: "folder"`
/// adds a `files` + `folders` listing for the depth-1 children of the
/// shared folder. The SPA branches on `kind` to render the right view.
/// Serialised flat (no nested `data:` wrapper) so the SPA TS type can
/// be a discriminated union without restructuring existing call sites.
#[derive(Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
enum Resolved {
    File {
        file: RecipientFile,
        download_url: String,
        permissions: String,
    },
    Folder {
        folder: RecipientFolder,
        files: Vec<RecipientFile>,
        folders: Vec<RecipientFolder>,
        permissions: String,
    },
}

async fn resolve_share(
    State(s): State<HttpState>,
    Path(token): Path<String>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Resolved>, ShareError> {
    let repo = ShareLinkRepo::new(&s.db);
    let link = repo
        .find_by_token(&token)
        .await
        .map_err(|_| ShareError::NotFound)?;

    if link.is_expired() {
        return Err(ShareError::Expired);
    }

    if let Some(hash) = link.password_hash.as_deref() {
        let candidate = body.password.as_deref().unwrap_or("");
        if candidate.is_empty() || !verify_password(hash, candidate).unwrap_or(false) {
            return Err(ShareError::PasswordRequired);
        }
    }

    if let Some(folder_id) = link.folder_id.as_deref() {
        let folders = FolderRepo::new(&s.db);
        let folder = folders
            .find_by_id(folder_id)
            .await
            .map_err(|_| ShareError::NotFound)?;
        if folder.trashed_at.is_some() {
            return Err(ShareError::NotFound);
        }
        // Depth-1 listing — recursive descent is a Phase-2 polish.
        // Scope by the folder's owner so the recipient sees what the
        // sharer would see, regardless of who's anonymously visiting.
        let child_files = FileRepo::new(&s.db)
            .list_children(Some(&folder.id), &folder.owner_id)
            .await
            .unwrap_or_default();
        let child_folders = folders
            .list_children(Some(&folder.id), &folder.owner_id)
            .await
            .unwrap_or_default();

        let _ = repo.touch(&link.id).await;

        AuditRepo::emit(
            &s.db,
            NewAuditEvent {
                actor_id: None,
                actor_username: None,
                action: "share.access".into(),
                target_kind: Some("share_link".into()),
                target_id: Some(link.id.clone()),
                target_name: Some(folder.name.clone()),
                ip_address: None,
                metadata: Some(format!(
                    r#"{{"token":"{}","folder_id":"{}"}}"#,
                    link.token, folder.id
                )),
            },
        );

        return Ok(Json(Resolved::Folder {
            folder: RecipientFolder {
                id: folder.id.clone(),
                name: folder.name,
                modified_at: rfc3339(folder.modified_at),
            },
            files: child_files
                .into_iter()
                .map(|f| RecipientFile {
                    id: f.id,
                    name: f.name,
                    size: f.size,
                    content_type: f.content_type,
                    modified_at: rfc3339(f.modified_at),
                })
                .collect(),
            folders: child_folders
                .into_iter()
                .map(|f| RecipientFolder {
                    id: f.id,
                    name: f.name,
                    modified_at: rfc3339(f.modified_at),
                })
                .collect(),
            permissions: link.permissions,
        }));
    }

    let file_id = link.file_id.as_deref().ok_or(ShareError::NotFound)?;
    let files = FileRepo::new(&s.db);
    let file = files
        .find_by_id(file_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if file.trashed_at.is_some() {
        return Err(ShareError::NotFound);
    }

    // Best-effort touch — failure is non-fatal.
    let _ = repo.touch(&link.id).await;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: None, // recipient is anonymous
            actor_username: None,
            action: "share.access".into(),
            target_kind: Some("share_link".into()),
            target_id: Some(link.id.clone()),
            target_name: Some(file.name.clone()),
            ip_address: None,
            metadata: Some(format!(r#"{{"token":"{}"}}"#, link.token)),
        },
    );

    Ok(Json(Resolved::File {
        file: RecipientFile {
            id: file.id,
            name: file.name,
            size: file.size,
            content_type: file.content_type,
            modified_at: rfc3339(file.modified_at),
        },
        download_url: format!("/api/share/{}/download", link.token),
        permissions: link.permissions,
    }))
}

#[derive(Deserialize, Default)]
pub(crate) struct DownloadQuery {
    /// Folder-share recipients pass `?file_id=…` to download a single
    /// child of the shared folder; the server validates the child
    /// actually belongs to that folder before signing the URL. File
    /// shares ignore this — they always serve the link's `file_id`.
    pub file_id: Option<String>,
}

async fn download_share(
    State(s): State<HttpState>,
    Path(token): Path<String>,
    Query(query): Query<DownloadQuery>,
) -> Result<Response, ShareError> {
    let repo = ShareLinkRepo::new(&s.db);
    let link = repo
        .find_by_token(&token)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if link.is_expired() {
        return Err(ShareError::Expired);
    }
    // Password-gated links require the password to flow through
    // POST /api/share/{token} first — once the SPA has shown a download
    // button to the user, password-gating the byte fetch as well would be
    // hostile UX. The token + active link is the access control here.

    let files = FileRepo::new(&s.db);
    let file_id = if let Some(fid) = link.file_id.as_deref() {
        // Single-file share: the link's file is the only valid target.
        // A stray ?file_id=… query is ignored.
        fid.to_string()
    } else if let Some(folder_id) = link.folder_id.as_deref() {
        // Folder share: ?file_id=… must reference a direct child of
        // the shared folder, owned by the folder's owner. Anything
        // else is a 404 — anti-enumeration is the same posture as
        // revoke_share.
        let requested = query.file_id.as_deref().ok_or(ShareError::NotFound)?;
        let folder = FolderRepo::new(&s.db)
            .find_by_id(folder_id)
            .await
            .map_err(|_| ShareError::NotFound)?;
        let file = files
            .find_by_id(requested)
            .await
            .map_err(|_| ShareError::NotFound)?;
        if file.parent_id.as_deref() != Some(&folder.id) || file.owner_id != folder.owner_id {
            return Err(ShareError::NotFound);
        }
        file.id
    } else {
        return Err(ShareError::NotFound);
    };

    let file = files
        .find_by_id(&file_id)
        .await
        .map_err(|_| ShareError::NotFound)?;
    if file.trashed_at.is_some() {
        return Err(ShareError::NotFound);
    }

    let signed = s
        .storage
        .signed_get(&storage_key(&file_id), Duration::from_secs(120))
        .await
        .map_err(|e| ShareError::Internal(e.to_string()))?;

    let target = match signed {
        SignedUrl::Native { url, .. } => url.to_string(),
        SignedUrl::Token { token, .. } => {
            let mut base = s.config.usercontent_origin.clone();
            base.set_path(&format!("/raw/{token}"));
            base.to_string()
        }
    };

    let _ = repo.touch(&link.id).await;

    let mut r = (StatusCode::FOUND, ()).into_response();
    r.headers_mut()
        .insert(header::LOCATION, HeaderValue::from_str(&target).unwrap());
    Ok(r)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn mint_token() -> String {
    // 128 bits of entropy from OsRng → URL-safe base64. 22 chars without
    // padding. Same OsRng channel as drive-auth's session/CSRF tokens.
    use argon2::password_hash::rand_core::{OsRng, RngCore};
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// ── Errors ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub(crate) enum ShareError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("validation: {0}")]
    Validation(String),
    #[error("password required")]
    PasswordRequired,
    #[error("expired")]
    Expired,
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrBody<'a> {
    error: &'a str,
}

impl IntoResponse for ShareError {
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
            Self::PasswordRequired => {
                // 401 + WWW-Authenticate signals to the SPA that a password
                // is needed without having to disambiguate inside the body.
                let mut r = (
                    StatusCode::UNAUTHORIZED,
                    Json(ErrBody {
                        error: "password required",
                    }),
                )
                    .into_response();
                r.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    HeaderValue::from_static("x-share-password"),
                );
                r
            }
            Self::Expired => (StatusCode::GONE, Json(ErrBody { error: "expired" })).into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "share internal error");
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

// ── Router ──────────────────────────────────────────────────────────────

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/files/{id}/share", post(create_share))
        .route("/api/files/{id}/shares", get(list_shares))
        .route("/api/folders/{id}/share", post(create_folder_share))
        .route("/api/folders/{id}/shares", get(list_folder_shares))
        .route("/api/shares/{id}", delete(revoke_share))
        .route("/api/share/{token}", post(resolve_share))
        .route("/api/share/{token}/download", get(download_share))
        .with_state(state)
}
