//! Pipeline §13.6 — direct-to-storage upload endpoints.
//! Spec: docs/research/10-direct-upload.md.
//!
//! Three routes:
//!   - `POST /api/files/upload-url` — presign + create row as `uploading`
//!   - `POST /api/files/{id}/complete` — stat + flip to `ready`
//!   - `POST /api/files/{id}/abort` — drop row + best-effort delete object
//!
//! The proxy multipart path at `POST /api/files` (in `files.rs`) is
//! unchanged. Direct upload is opt-in from the client side.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use drive_auth::AuthSession;
use drive_db::{AuditRepo, FileRepo, FileStatus, NewAuditEvent, NewFile};
use drive_storage::SignedUrl;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::{files::FileDto, HttpState};

/// 15 minutes — long enough for a flaky mobile connection on a multi-GB
/// PUT, short enough that a leaked URL stops working before the next
/// coffee. Spec calls this out.
const PRESIGN_TTL: Duration = Duration::from_secs(15 * 60);

/// 8 MiB — the SPA opts into direct upload at this threshold. We
/// don't enforce it server-side (the proxy path keeps working at any
/// size) but we *do* document it so the SPA's branching stays in sync.
#[allow(dead_code)]
pub(crate) const DIRECT_UPLOAD_THRESHOLD_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum DirectError {
    Forbidden,
    NotFound,
    Validation(String),
    AdapterCannotPresign,
    QuotaExceeded { used: u64, quota: u64 },
    NotUploading,
    Internal(String),
}

#[derive(Serialize)]
struct Err<'a> {
    error: &'a str,
}

impl IntoResponse for DirectError {
    fn into_response(self) -> Response {
        match self {
            Self::Forbidden => {
                (StatusCode::FORBIDDEN, Json(Err { error: "forbidden" })).into_response()
            }
            Self::NotFound => {
                (StatusCode::NOT_FOUND, Json(Err { error: "not found" })).into_response()
            }
            Self::Validation(m) => {
                (StatusCode::BAD_REQUEST, Json(Err { error: &m })).into_response()
            }
            // 409 — SPA branches to proxy upload when it sees this.
            Self::AdapterCannotPresign => (
                StatusCode::CONFLICT,
                Json(Err {
                    error: "this workspace's storage adapter doesn't support direct upload",
                }),
            )
                .into_response(),
            Self::QuotaExceeded { used, quota } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "error": "workspace quota would be exceeded",
                    "used_bytes": used,
                    "quota_bytes": quota,
                })),
            )
                .into_response(),
            Self::NotUploading => (
                StatusCode::CONFLICT,
                Json(Err {
                    error: "file is not in 'uploading' state",
                }),
            )
                .into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "direct_upload handler error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(Err {
                        error: "internal error",
                    }),
                )
                    .into_response()
            }
        }
    }
}

// ── Bodies ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct PresignBody {
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct PresignResp {
    pub file_id: String,
    pub upload_url: String,
    pub expires_at: String,
    pub method: &'static str,
    pub required_headers: serde_json::Value,
}

// ── Handlers ──────────────────────────────────────────────────────────

pub(crate) async fn presign(
    State(s): State<HttpState>,
    session: AuthSession,
    Json(body): Json<PresignBody>,
) -> Result<(StatusCode, Json<PresignResp>), DirectError> {
    let name = crate::files::sanitise_display_name(&body.name)
        .map_err(|e| DirectError::Validation(e.to_string()))?;
    crate::files::check_upload_extension(&name)
        .map_err(|e| DirectError::Validation(e.to_string()))?;

    if body.size == 0 {
        return Err(DirectError::Validation("size must be > 0".into()));
    }

    let workspace_id = crate::workspaces::resolve_active_workspace(
        &s.db,
        &session.user_id,
        body.workspace_id.as_deref(),
    )
    .await
    .map_err(|e| match e {
        crate::workspaces::WsError::Forbidden => DirectError::Forbidden,
        crate::workspaces::WsError::NotFound => DirectError::NotFound,
        other => DirectError::Internal(format!("workspace: {other:?}")),
    })?;

    // Resolve adapter for this workspace. BYO when set, default otherwise.
    let (storage, storage_id) = crate::workspace_storage::resolve_upload_storage(&s, &workspace_id)
        .await
        .map_err(|e| DirectError::Internal(format!("storage: {e:?}")))?;

    // Quota gate. Counts uploading rows against the cap via
    // `workspace_used_bytes`'s CASE clause.
    let users = drive_db::UserRepo::new(&s.db);
    let me = users
        .find_by_id(&session.user_id)
        .await
        .map_err(|e| DirectError::Internal(e.to_string()))?;
    if let Some(quota) = me.quota_bytes {
        let used = FileRepo::new(&s.db)
            .workspace_used_bytes(&workspace_id)
            .await
            .map_err(|e| DirectError::Internal(e.to_string()))?;
        if used + body.size > quota {
            return Err(DirectError::QuotaExceeded { used, quota });
        }
    }

    let id = ulid::Ulid::new().to_string();
    let key = crate::files::storage_key(&id);

    // Mint the signed PUT now — if the adapter can't presign (fs / memory)
    // we return 409 and the SPA falls back to the proxy path.
    let signed = storage
        .signed_put(&key, PRESIGN_TTL)
        .await
        .map_err(|e| DirectError::Internal(format!("signed_put: {e}")))?;
    let (url, expires_at) = match signed {
        SignedUrl::Native { url, expires_at } => (url.to_string(), expires_at),
        SignedUrl::Token { .. } => return Err(DirectError::AdapterCannotPresign),
    };

    // Insert as `uploading` only AFTER the presign succeeds. If we
    // inserted earlier and presign failed, we'd leak rows.
    let file = FileRepo::new(&s.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: body.parent_id.clone(),
            name: name.clone(),
            size: 0,
            content_type: body.content_type.clone(),
            etag: None,
            owner_id: session.user_id.clone(),
            workspace_id,
            storage_id,
            thumbnail: None,
            status: FileStatus::Uploading,
            expected_size: Some(body.size),
        })
        .await
        .map_err(|e| DirectError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.upload_url_minted".into(),
            target_kind: Some("file".into()),
            target_id: Some(file.id.clone()),
            target_name: Some(file.name.clone()),
            ip_address: None,
            metadata: Some(format!(
                r#"{{"size":{},"content_type":{}}}"#,
                body.size,
                serde_json::to_string(body.content_type.as_deref().unwrap_or(""))
                    .unwrap_or_else(|_| "\"\"".into())
            )),
        },
    );

    let mut required_headers = serde_json::Map::new();
    if let Some(ct) = body.content_type.as_deref() {
        required_headers.insert("Content-Type".into(), serde_json::Value::String(ct.into()));
    }

    Ok((
        StatusCode::CREATED,
        Json(PresignResp {
            file_id: file.id,
            upload_url: url,
            expires_at: expires_at
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            method: "PUT",
            required_headers: serde_json::Value::Object(required_headers),
        }),
    ))
}

pub(crate) async fn complete(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<FileDto>, DirectError> {
    let repo = FileRepo::new(&s.db);
    let row = repo
        .find_by_id(&id)
        .await
        .map_err(|_| DirectError::NotFound)?;

    require_membership(&s, &session, &row).await?;

    if row.status != FileStatus::Uploading {
        return Err(DirectError::NotUploading);
    }

    // Pick the right adapter (this also handles BYO rows whose key
    // version may have been bumped since presign — read fresh).
    let workspace_id = row
        .workspace_id
        .clone()
        .ok_or_else(|| DirectError::Internal("file row has no workspace_id".into()))?;
    let (storage, _) = crate::workspace_storage::resolve_upload_storage(&s, &workspace_id)
        .await
        .map_err(|e| DirectError::Internal(format!("storage: {e:?}")))?;

    let meta = storage
        .stat(&crate::files::storage_key(&row.id))
        .await
        .map_err(|e| match e {
            drive_storage::StorageError::NotFound(_) => DirectError::NotFound,
            other => DirectError::Internal(format!("stat: {other}")),
        })?;

    let finalized = repo
        .mark_uploaded(
            &row.id,
            meta.size,
            meta.etag.as_deref(),
            meta.content_type.as_deref().or(row.content_type.as_deref()),
        )
        .await
        .map_err(|e| DirectError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.upload_completed".into(),
            target_kind: Some("file".into()),
            target_id: Some(finalized.id.clone()),
            target_name: Some(finalized.name.clone()),
            ip_address: None,
            metadata: Some(format!(r#"{{"size":{},"direct":true}}"#, finalized.size)),
        },
    );

    Ok(Json(FileDto::from(finalized)))
}

pub(crate) async fn abort(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, DirectError> {
    let repo = FileRepo::new(&s.db);
    let row = match repo.find_by_id(&id).await {
        Ok(r) => r,
        // Idempotent: already gone is fine.
        Err(_) => return Ok(StatusCode::NO_CONTENT),
    };
    require_membership(&s, &session, &row).await?;

    if row.status != FileStatus::Uploading {
        // Only abort uploading rows — refuse to nuke ready files via
        // this endpoint.
        return Err(DirectError::NotUploading);
    }

    // Best-effort delete of the object. We swallow errors because the
    // bucket may not have received any bytes (or the PUT may have
    // already failed) — either way, the row going away is what matters.
    if let Some(workspace_id) = row.workspace_id.clone() {
        if let Ok((storage, _)) =
            crate::workspace_storage::resolve_upload_storage(&s, &workspace_id).await
        {
            let _ = storage.delete(&crate::files::storage_key(&row.id)).await;
        }
    }

    repo.delete_by_id(&row.id)
        .await
        .map_err(|e| DirectError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "files.upload_aborted".into(),
            target_kind: Some("file".into()),
            target_id: Some(row.id),
            target_name: Some(row.name),
            ip_address: None,
            metadata: None,
        },
    );

    Ok(StatusCode::NO_CONTENT)
}

async fn require_membership(
    s: &HttpState,
    session: &AuthSession,
    row: &drive_db::File,
) -> Result<(), DirectError> {
    let Some(workspace_id) = row.workspace_id.as_deref() else {
        // Legacy row (pre-§8.8). Fall back to owner check.
        if row.owner_id != session.user_id {
            return Err(DirectError::Forbidden);
        }
        return Ok(());
    };
    let role = drive_db::WorkspaceMemberRepo::new(&s.db)
        .role_of(workspace_id, &session.user_id)
        .await
        .map_err(|e| DirectError::Internal(e.to_string()))?;
    if role.is_none() {
        return Err(DirectError::Forbidden);
    }
    Ok(())
}

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/files/upload-url", post(presign))
        .route("/api/files/{id}/complete", post(complete))
        .route("/api/files/{id}/abort", post(abort))
        .with_state(state)
}
