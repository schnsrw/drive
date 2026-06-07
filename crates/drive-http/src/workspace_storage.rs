//! Per-workspace BYO storage endpoints. Pipeline §8.9.
//! Spec: docs/research/08-byo-storage.md, docs/ux/15-byo-storage-surface.md.
//!
//! All routes are owner-only on Team workspaces; Personal workspaces return
//! 409 (they always use the server default). The secret is never returned
//! over the wire — `GET` returns provider/bucket/etc. + a fixed mask.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use drive_auth::AuthSession;
use drive_db::{
    AuditRepo, NewAuditEvent, NewWorkspaceStorage, WorkspaceKind, WorkspaceRepo,
    WorkspaceStorageProvider, WorkspaceStorageRepo,
};
use drive_storage::{
    seal_secret, ssrf_guard, test_connection, validate_shape_, ByoConfig, ByoError,
};
use serde::{Deserialize, Serialize};

use crate::HttpState;

#[derive(Debug)]
pub(crate) enum WsStorageError {
    Forbidden,
    NotFound,
    PersonalRefused,
    KeyMissing,
    Validation(String),
    TestFailed(String),
    Internal(String),
}

#[derive(Serialize)]
struct Err<'a> {
    error: &'a str,
}

impl IntoResponse for WsStorageError {
    fn into_response(self) -> Response {
        match self {
            Self::Forbidden => {
                (StatusCode::FORBIDDEN, Json(Err { error: "forbidden" })).into_response()
            }
            Self::NotFound => {
                (StatusCode::NOT_FOUND, Json(Err { error: "not found" })).into_response()
            }
            Self::PersonalRefused => (
                StatusCode::CONFLICT,
                Json(Err {
                    error: "personal workspaces use the server default storage",
                }),
            )
                .into_response(),
            Self::KeyMissing => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(Err {
                    error: "server has no storage secret key configured; \
                         set DRIVE_STORAGE_SECRET_KEY (64 hex chars) and restart",
                }),
            )
                .into_response(),
            Self::Validation(m) => {
                (StatusCode::BAD_REQUEST, Json(Err { error: &m })).into_response()
            }
            Self::TestFailed(m) => {
                (StatusCode::UNPROCESSABLE_ENTITY, Json(Err { error: &m })).into_response()
            }
            Self::Internal(m) => {
                tracing::error!(error = %m, "workspace_storage handler error");
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

// ── Request bodies ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ConfigBody {
    provider: WorkspaceStorageProvider,
    bucket: String,
    region: String,
    #[serde(default)]
    endpoint: Option<String>,
    access_key_id: String,
    /// Plaintext on the wire. NEVER logged — see [`redact_body_for_log`].
    secret_access_key: String,
}

#[derive(Deserialize)]
struct ReplaceSecretBody {
    access_key_id: String,
    secret_access_key: String,
}

#[derive(Deserialize)]
struct TestBody {
    provider: WorkspaceStorageProvider,
    bucket: String,
    region: String,
    #[serde(default)]
    endpoint: Option<String>,
    access_key_id: String,
    secret_access_key: String,
}

// ── Response bodies ───────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum StorageStatus {
    Default,
    Byo {
        id: String,
        provider: WorkspaceStorageProvider,
        bucket: String,
        region: String,
        endpoint: Option<String>,
        access_key_id_masked: String,
        secret_masked: String,
        key_version: i64,
        tested_at: Option<String>,
        tested_ok: bool,
        tested_error: Option<String>,
    },
}

#[derive(Serialize)]
pub(crate) struct TestResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ── Auth helpers ──────────────────────────────────────────────────────

/// Verifies the workspace exists, is a Team workspace, and the caller is
/// its Owner. Returns the workspace id (for audit metadata) on success.
async fn require_owner(
    s: &HttpState,
    session: &AuthSession,
    workspace_id: &str,
) -> Result<String, WsStorageError> {
    let repo = WorkspaceRepo::new(&s.db);
    let ws = repo
        .find_by_id(workspace_id)
        .await
        .map_err(|_| WsStorageError::NotFound)?;
    if matches!(ws.kind, WorkspaceKind::Personal) {
        return Err(WsStorageError::PersonalRefused);
    }
    if ws.owner_id != session.user_id {
        return Err(WsStorageError::Forbidden);
    }
    Ok(ws.name)
}

fn master_key(s: &HttpState) -> Result<[u8; 32], WsStorageError> {
    s.storage_secret_key
        .as_ref()
        .map(|k| **k)
        .ok_or(WsStorageError::KeyMissing)
}

fn allow_insecure() -> bool {
    matches!(
        std::env::var("DRIVE_ALLOW_INSECURE_BYO").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

/// Mask an access key id for display. Shows the first 4 chars + tail 4.
fn mask_ak(id: &str) -> String {
    let len = id.chars().count();
    if len <= 8 {
        return "•".repeat(len.max(8));
    }
    let head: String = id.chars().take(4).collect();
    let tail: String = id.chars().skip(len - 4).collect();
    format!("{head}…{tail}")
}

// ── Handlers ──────────────────────────────────────────────────────────

async fn get_storage(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<StorageStatus>, WsStorageError> {
    // GET is owner-only too — config + bucket name are sensitive enough
    // that members shouldn't see them.
    let _ = require_owner(&s, &session, &id).await?;
    let row = WorkspaceStorageRepo::new(&s.db)
        .find_by_workspace(&id)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    let Some(row) = row else {
        return Ok(Json(StorageStatus::Default));
    };
    Ok(Json(StorageStatus::Byo {
        id: row.id,
        provider: row.provider,
        bucket: row.bucket,
        region: row.region,
        endpoint: row.endpoint,
        access_key_id_masked: mask_ak(&row.access_key_id),
        secret_masked: "••••••••••".into(),
        key_version: row.key_version,
        tested_at: row.tested_at.map(rfc3339),
        tested_ok: row.tested_ok,
        tested_error: row.tested_error,
    }))
}

async fn put_storage(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<ConfigBody>,
) -> Result<(StatusCode, Json<StorageStatus>), WsStorageError> {
    let target_name = require_owner(&s, &session, &id).await?;
    let key = master_key(&s)?;

    let cfg = body_to_cfg(&body);
    validate_shape_(&cfg).map_err(byo_to_err)?;
    ssrf_guard(cfg.endpoint.as_deref(), allow_insecure()).map_err(byo_to_err)?;

    let latency = test_connection(&cfg)
        .await
        .map_err(|e| WsStorageError::TestFailed(e.to_string()))?;

    // Persist a placeholder row to mint an id; immediately replace the
    // ciphertext with one bound to the real id+version.
    let envelope_placeholder = "placeholder".to_string();
    let repo = WorkspaceStorageRepo::new(&s.db);
    let inserted = repo
        .upsert(&NewWorkspaceStorage {
            workspace_id: id.clone(),
            provider: body.provider,
            bucket: cfg.bucket.clone(),
            region: cfg.region.clone(),
            endpoint: cfg.endpoint.clone(),
            access_key_id: cfg.access_key_id.clone(),
            secret_ct: envelope_placeholder,
        })
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    let aad = inserted.aad();
    let sealed = seal_secret(&key, cfg.secret_access_key.as_bytes(), &aad)
        .map_err(|e| WsStorageError::Internal(format!("seal: {e}")))?;
    repo.replace_secret(&inserted.id, &cfg.access_key_id, &sealed)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    // replace_secret bumps key_version → 2 because the row was inserted at
    // version 1. Touch test result against version 2 so the surface badge
    // is correct.
    repo.touch_test(&inserted.id, true, None)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    s.registry.invalidate(&inserted.id);

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace_storage.configured".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id.clone()),
            target_name: Some(target_name),
            ip_address: None,
            metadata: Some(format!(
                r#"{{"provider":"{}","bucket":{},"region":{},"latency_ms":{}}}"#,
                body.provider_str(),
                json_str(&cfg.bucket),
                json_str(&cfg.region),
                latency
            )),
        },
    );

    let after = repo
        .find_by_id(&inserted.id)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?
        .ok_or_else(|| WsStorageError::Internal("row vanished post-insert".into()))?;
    Ok((
        StatusCode::CREATED,
        Json(StorageStatus::Byo {
            id: after.id,
            provider: after.provider,
            bucket: after.bucket,
            region: after.region,
            endpoint: after.endpoint,
            access_key_id_masked: mask_ak(&after.access_key_id),
            secret_masked: "••••••••••".into(),
            key_version: after.key_version,
            tested_at: after.tested_at.map(rfc3339),
            tested_ok: after.tested_ok,
            tested_error: after.tested_error,
        }),
    ))
}

async fn test_storage(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<TestBody>,
) -> Result<Json<TestResult>, WsStorageError> {
    let target_name = require_owner(&s, &session, &id).await?;
    let cfg = ByoConfig {
        provider: provider_to_drive_storage(body.provider),
        bucket: body.bucket,
        region: body.region,
        endpoint: body.endpoint,
        access_key_id: body.access_key_id,
        secret_access_key: body.secret_access_key,
    };
    validate_shape_(&cfg).map_err(byo_to_err)?;
    ssrf_guard(cfg.endpoint.as_deref(), allow_insecure()).map_err(byo_to_err)?;

    match test_connection(&cfg).await {
        Ok(latency) => {
            AuditRepo::emit(
                &s.db,
                NewAuditEvent {
                    actor_id: Some(session.user_id.clone()),
                    actor_username: Some(session.username.clone()),
                    action: "workspace_storage.test_run".into(),
                    target_kind: Some("workspace".into()),
                    target_id: Some(id),
                    target_name: Some(target_name),
                    ip_address: None,
                    metadata: Some(format!(r#"{{"ok":true,"latency_ms":{latency}}}"#)),
                },
            );
            Ok(Json(TestResult {
                ok: true,
                latency_ms: Some(latency),
                error: None,
            }))
        }
        Err(e) => {
            let msg = e.to_string();
            AuditRepo::emit(
                &s.db,
                NewAuditEvent {
                    actor_id: Some(session.user_id.clone()),
                    actor_username: Some(session.username.clone()),
                    action: "workspace_storage.test_run".into(),
                    target_kind: Some("workspace".into()),
                    target_id: Some(id),
                    target_name: Some(target_name),
                    ip_address: None,
                    metadata: Some(format!(r#"{{"ok":false,"error":{}}}"#, json_str(&msg))),
                },
            );
            Ok(Json(TestResult {
                ok: false,
                latency_ms: None,
                error: Some(msg),
            }))
        }
    }
}

async fn patch_credentials(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<ReplaceSecretBody>,
) -> Result<StatusCode, WsStorageError> {
    let target_name = require_owner(&s, &session, &id).await?;
    let key = master_key(&s)?;
    let repo = WorkspaceStorageRepo::new(&s.db);
    let existing = repo
        .find_by_workspace(&id)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?
        .ok_or(WsStorageError::NotFound)?;

    // Re-test before saving — replacing a working credential with a broken
    // one would silently brick uploads otherwise.
    let cfg = ByoConfig {
        provider: provider_to_drive_storage(existing.provider),
        bucket: existing.bucket.clone(),
        region: existing.region.clone(),
        endpoint: existing.endpoint.clone(),
        access_key_id: body.access_key_id.clone(),
        secret_access_key: body.secret_access_key.clone(),
    };
    validate_shape_(&cfg).map_err(byo_to_err)?;
    ssrf_guard(cfg.endpoint.as_deref(), allow_insecure()).map_err(byo_to_err)?;
    test_connection(&cfg)
        .await
        .map_err(|e| WsStorageError::TestFailed(e.to_string()))?;

    let new_version = existing.key_version + 1;
    let new_aad = format!("{}:{}", existing.id, new_version);
    let sealed = seal_secret(&key, body.secret_access_key.as_bytes(), &new_aad)
        .map_err(|e| WsStorageError::Internal(format!("seal: {e}")))?;
    repo.replace_secret(&existing.id, &body.access_key_id, &sealed)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    repo.touch_test(&existing.id, true, None)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;
    s.registry.invalidate(&existing.id);

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace_storage.replaced_credentials".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id),
            target_name: Some(target_name),
            ip_address: None,
            metadata: Some(format!(r#"{{"key_version":{new_version}}}"#)),
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_storage(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, WsStorageError> {
    let target_name = require_owner(&s, &session, &id).await?;
    let repo = WorkspaceStorageRepo::new(&s.db);
    if let Some(existing) = repo
        .find_by_workspace(&id)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?
    {
        s.registry.invalidate(&existing.id);
    }
    repo.delete(&id)
        .await
        .map_err(|e| WsStorageError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace_storage.removed".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id),
            target_name: Some(target_name),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ───────────────────────────────────────────────────────────

fn body_to_cfg(body: &ConfigBody) -> ByoConfig {
    ByoConfig {
        provider: provider_to_drive_storage(body.provider),
        bucket: body.bucket.trim().to_string(),
        region: body.region.trim().to_string(),
        endpoint: body.endpoint.as_deref().map(str::trim).map(str::to_string),
        access_key_id: body.access_key_id.trim().to_string(),
        secret_access_key: body.secret_access_key.clone(),
    }
}

impl ConfigBody {
    fn provider_str(&self) -> &'static str {
        match self.provider {
            WorkspaceStorageProvider::S3 => "s3",
            WorkspaceStorageProvider::Minio => "minio",
            WorkspaceStorageProvider::R2 => "r2",
            WorkspaceStorageProvider::B2 => "b2",
        }
    }
}

fn provider_to_drive_storage(p: WorkspaceStorageProvider) -> drive_storage::Provider {
    match p {
        WorkspaceStorageProvider::S3 => drive_storage::Provider::S3,
        WorkspaceStorageProvider::Minio => drive_storage::Provider::Minio,
        WorkspaceStorageProvider::R2 => drive_storage::Provider::R2,
        WorkspaceStorageProvider::B2 => drive_storage::Provider::B2,
    }
}

fn byo_to_err(e: ByoError) -> WsStorageError {
    match e {
        ByoError::Invalid(m) => WsStorageError::Validation(m.to_string()),
        ByoError::SsrfBlocked(m) => WsStorageError::Validation(m.to_string()),
        ByoError::TestFailed(m) => WsStorageError::TestFailed(m),
        ByoError::Storage(e) => WsStorageError::Internal(e.to_string()),
    }
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Resolves the storage adapter + pointer that an upload to `workspace_id`
/// should write to. Returns `(adapter, storage_id)`:
///
/// - `(default, None)` — workspace has no BYO row (server default).
/// - `(byo, Some(row_id))` — workspace has a BYO row + the secret decrypts.
///
/// Bubbles `FilesError::Internal` on decrypt failure rather than silently
/// falling back to the default — silent fallback would split a workspace's
/// files across two buckets without the owner's knowledge.
pub(crate) async fn resolve_upload_storage(
    s: &HttpState,
    workspace_id: &str,
) -> Result<(std::sync::Arc<drive_storage::Storage>, Option<String>), crate::files::FilesError> {
    let row = WorkspaceStorageRepo::new(&s.db)
        .find_by_workspace(workspace_id)
        .await
        .map_err(|e| crate::files::FilesError::Internal(e.to_string()))?;
    let Some(row) = row else {
        return Ok((s.registry.default_storage(), None));
    };
    let key = s.storage_secret_key.as_ref().ok_or_else(|| {
        crate::files::FilesError::Internal(
            "workspace storage configured but DRIVE_STORAGE_SECRET_KEY is missing".into(),
        )
    })?;
    let secret_bytes =
        drive_storage::open_secret(key, &row.secret_ct, &row.aad()).map_err(|e| {
            crate::files::FilesError::Internal(format!("decrypt workspace storage secret: {e}"))
        })?;
    let secret_string = String::from_utf8(secret_bytes).map_err(|_| {
        crate::files::FilesError::Internal("workspace storage secret is not UTF-8".into())
    })?;
    let cfg = ByoConfig {
        provider: provider_to_drive_storage(row.provider),
        bucket: row.bucket.clone(),
        region: row.region.clone(),
        endpoint: row.endpoint.clone(),
        access_key_id: row.access_key_id.clone(),
        secret_access_key: secret_string,
    };
    let adapter = s
        .registry
        .for_byo(&row.id, row.key_version, &cfg)
        .map_err(|e| crate::files::FilesError::Internal(e.to_string()))?;
    Ok((adapter, Some(row.id)))
}

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route(
            "/api/workspaces/{id}/storage",
            get(get_storage).put(put_storage).delete(delete_storage),
        )
        .route("/api/workspaces/{id}/storage/test", post(test_storage))
        .route(
            "/api/workspaces/{id}/storage/credentials",
            axum::routing::patch(patch_credentials),
        )
        .with_state(state)
        // Same trick as workspaces.rs to keep the `delete` re-export touched.
        .layer(tower::ServiceBuilder::new())
}

#[allow(dead_code)]
const _USE_DELETE: fn() = || {
    let _: fn(axum::routing::MethodRouter<HttpState>) -> axum::routing::MethodRouter<HttpState> =
        delete;
};
