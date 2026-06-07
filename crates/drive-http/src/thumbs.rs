//! Pipeline §5.4 — server-side multi-size thumbnails.
//! Spec: docs/research/11-server-thumbnails.md.
//!
//! Two routes:
//!   - `GET  /api/files/{id}/thumb/{size}` — 302 to a signed URL for the
//!     requested size; triggers generation on first miss (lazy).
//!   - `POST /api/files/{id}/thumb/regenerate` — owner-only force.
//!
//! Generation runs in-process via the `ImageOnlyWorker` (image kinds
//! only in v0; PDF/video deferred to v0.2 — they need a sandboxed
//! subprocess per the security brief).

use axum::{
    extract::{Path, State},
    http::{header, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use drive_auth::AuthSession;
use drive_db::{FileRepo, ThumbsState};
use drive_storage::{ImageOnlyWorker, ThumbSize, ThumbnailError, ThumbnailKind};
use futures::TryStreamExt;
use serde::Serialize;
use std::time::Duration;

use crate::HttpState;

#[derive(Debug)]
pub(crate) enum ThumbError {
    NotFound,
    Forbidden,
    Unsupported,
    NotReadyYet,
    Internal(String),
}

#[derive(Serialize)]
struct Err<'a> {
    error: &'a str,
}

impl IntoResponse for ThumbError {
    fn into_response(self) -> Response {
        match self {
            Self::NotFound => {
                (StatusCode::NOT_FOUND, Json(Err { error: "not found" })).into_response()
            }
            Self::Forbidden => {
                (StatusCode::FORBIDDEN, Json(Err { error: "forbidden" })).into_response()
            }
            Self::Unsupported => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(Err {
                    error: "file type doesn't have a server-side thumbnail",
                }),
            )
                .into_response(),
            // 202 — SPA falls back to inline `thumbnail` data URI while
            // the worker catches up.
            Self::NotReadyYet => (StatusCode::ACCEPTED, "generating\n").into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "thumb handler error");
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

/// `GET /api/files/{id}/thumb/{size}` — return a 302 to the signed URL
/// for the requested cached thumbnail. On `pending` rows we kick the
/// worker (lazy generation), return 202, and the SPA retries.
pub(crate) async fn get_thumb(
    State(s): State<HttpState>,
    session: AuthSession,
    Path((id, size)): Path<(String, String)>,
) -> Result<Response, ThumbError> {
    let size = parse_size(&size).ok_or(ThumbError::NotFound)?;

    let repo = FileRepo::new(&s.db);
    let row = repo
        .find_by_id(&id)
        .await
        .map_err(|_| ThumbError::NotFound)?;
    require_membership(&s, &session, &row).await?;

    match row.thumbs_state {
        ThumbsState::Ready => {
            let (storage, _) = crate::workspace_storage::resolve_upload_storage(
                &s,
                row.workspace_id.as_deref().unwrap_or(""),
            )
            .await
            .map_err(|e| ThumbError::Internal(format!("storage: {e:?}")))?;
            let signed = storage
                .signed_get(
                    &size.key_for(&row.id),
                    Duration::from_secs(s.config.signed_url_ttl_secs),
                )
                .await
                .map_err(|e| ThumbError::Internal(format!("signed_get: {e}")))?;
            let url = match signed {
                drive_storage::SignedUrl::Native { url, .. } => url.to_string(),
                drive_storage::SignedUrl::Token { token, .. } => {
                    // fs/memory backends — issue against the user-content origin.
                    format!("{}raw/{}", s.config.usercontent_origin.as_str(), token,)
                }
            };
            Ok((
                StatusCode::FOUND,
                [(
                    HeaderName::from_static("location"),
                    HeaderValue::from_str(&url).map_err(|e| ThumbError::Internal(e.to_string()))?,
                )],
            )
                .into_response())
        }
        ThumbsState::Unsupported => Err(ThumbError::Unsupported),
        ThumbsState::Failed | ThumbsState::Pending => {
            // Kick the worker for pending rows. Failed rows are retried by
            // the regenerate endpoint to avoid hammering broken decoders.
            if matches!(row.thumbs_state, ThumbsState::Pending) {
                schedule_generation(
                    s.clone(),
                    row.id.clone(),
                    row.workspace_id.clone(),
                    row.content_type.clone(),
                );
            }
            Err(ThumbError::NotReadyYet)
        }
    }
}

/// `POST /api/files/{id}/thumb/regenerate` — owner-only force. Re-runs
/// even on `failed` rows. Returns 202 + the row's new state.
pub(crate) async fn regenerate(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<RegenerateResp>), ThumbError> {
    let repo = FileRepo::new(&s.db);
    let row = repo
        .find_by_id(&id)
        .await
        .map_err(|_| ThumbError::NotFound)?;
    require_membership(&s, &session, &row).await?;

    // Owner-only: mirror the existing rename/trash gates.
    if row.owner_id != session.user_id {
        return Err(ThumbError::Forbidden);
    }

    repo.set_thumbs_state(&row.id, ThumbsState::Pending, false)
        .await
        .map_err(|e| ThumbError::Internal(e.to_string()))?;
    schedule_generation(
        s.clone(),
        row.id.clone(),
        row.workspace_id.clone(),
        row.content_type.clone(),
    );
    Ok((
        StatusCode::ACCEPTED,
        Json(RegenerateResp { state: "pending" }),
    ))
}

#[derive(Serialize)]
pub(crate) struct RegenerateResp {
    state: &'static str,
}

/// Spawn a tokio task that generates all three sizes and updates the
/// row. Errors flip `thumbs_state` to `failed`; the caller bears no
/// responsibility for backpressure (the worker pool is bounded via the
/// runtime's own scheduler).
fn schedule_generation(
    state: HttpState,
    file_id: String,
    workspace_id: Option<String>,
    content_type: Option<String>,
) {
    let kind = match content_type.as_deref() {
        Some(ct) if ct.starts_with("image/") => ThumbnailKind::Image,
        Some("application/pdf") => ThumbnailKind::Pdf,
        Some(ct) if ct.starts_with("video/") => ThumbnailKind::Video,
        _ => {
            // Not eligible — mark unsupported synchronously.
            tokio::spawn(async move {
                let _ = FileRepo::new(&state.db)
                    .set_thumbs_state(&file_id, ThumbsState::Unsupported, true)
                    .await;
            });
            return;
        }
    };

    tokio::spawn(async move {
        let next = match run_generation(&state, &file_id, workspace_id.as_deref(), kind).await {
            Ok(()) => ThumbsState::Ready,
            Err(ThumbnailError::Unsupported(_)) => ThumbsState::Unsupported,
            Err(e) => {
                tracing::warn!(file_id, error = %e, "thumbnail generation failed");
                ThumbsState::Failed
            }
        };
        let _ = FileRepo::new(&state.db)
            .set_thumbs_state(&file_id, next, matches!(next, ThumbsState::Ready))
            .await;
    });
}

async fn run_generation(
    state: &HttpState,
    file_id: &str,
    workspace_id: Option<&str>,
    kind: ThumbnailKind,
) -> Result<(), ThumbnailError> {
    let ws = workspace_id.ok_or_else(|| ThumbnailError::Decode("no workspace_id on row".into()))?;
    let (storage, _) = crate::workspace_storage::resolve_upload_storage(state, ws)
        .await
        .map_err(|e| ThumbnailError::Decode(format!("storage resolve: {e:?}")))?;

    // Pull the bytes once + render all three sizes.
    let key = crate::files::storage_key(file_id);
    let (_meta, stream) = storage
        .get(&key, None)
        .await
        .map_err(|e| ThumbnailError::Decode(format!("get: {e}")))?;
    let body: bytes::BytesMut = stream
        .try_fold(bytes::BytesMut::new(), |mut acc, chunk| async move {
            acc.extend_from_slice(&chunk);
            Ok(acc)
        })
        .await
        .map_err(|e| ThumbnailError::Decode(format!("stream: {e}")))?;
    let bytes = Bytes::from(body);

    let worker = ImageOnlyWorker;
    for size in ThumbSize::all() {
        let png = worker
            .generate(kind, bytes.clone(), size.px(), size.fit_mode())
            .await?;
        storage
            .put(&size.key_for(file_id), Bytes::from(png), Some("image/png"))
            .await
            .map_err(|e| ThumbnailError::Encode(format!("put: {e}")))?;
    }
    Ok(())
}

fn parse_size(s: &str) -> Option<ThumbSize> {
    match s {
        "small" => Some(ThumbSize::Small),
        "medium" => Some(ThumbSize::Medium),
        "large" => Some(ThumbSize::Large),
        _ => None,
    }
}

async fn require_membership(
    s: &HttpState,
    session: &AuthSession,
    row: &drive_db::File,
) -> Result<(), ThumbError> {
    let Some(workspace_id) = row.workspace_id.as_deref() else {
        if row.owner_id != session.user_id {
            return Err(ThumbError::Forbidden);
        }
        return Ok(());
    };
    let role = drive_db::WorkspaceMemberRepo::new(&s.db)
        .role_of(workspace_id, &session.user_id)
        .await
        .map_err(|e| ThumbError::Internal(e.to_string()))?;
    if role.is_none() {
        return Err(ThumbError::Forbidden);
    }
    Ok(())
}

#[allow(dead_code)]
const _USE_HEADER: HeaderName = header::CONTENT_TYPE;

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/files/{id}/thumb/{size}", get(get_thumb))
        .route("/api/files/{id}/thumb/regenerate", post(regenerate))
        .with_state(state)
}
