//! Workspaces API. Spec: docs/ux/13-workspaces-surface.md.
//!
//! Phase 1: list, create, rename, delete, transfer-owner. File scoping
//! by workspace_id + the per-action permission model lands in Phase 2.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use drive_auth::AuthSession;
use drive_db::{
    AuditRepo, NewAuditEvent, WorkspaceKind, WorkspaceMemberRepo, WorkspaceRepo, WorkspaceRole,
};
use serde::{Deserialize, Serialize};

use crate::HttpState;

#[derive(Debug)]
pub(crate) enum WsError {
    NotFound,
    Forbidden,
    Validation(String),
    Personal,
    NotAMember,
    Internal(String),
}

#[derive(Serialize)]
struct ErrBody<'a> {
    error: &'a str,
}

impl IntoResponse for WsError {
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
            Self::Personal => (
                StatusCode::CONFLICT,
                Json(ErrBody {
                    error: "personal workspaces cannot be renamed, transferred, or deleted",
                }),
            )
                .into_response(),
            Self::NotAMember => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ErrBody {
                    error: "target user is not a member of this workspace",
                }),
            )
                .into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "workspaces handler error");
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
pub(crate) struct WorkspaceDto {
    id: String,
    name: String,
    kind: WorkspaceKind,
    owner_id: String,
    role: WorkspaceRole,
    member_count: i64,
    created_at: String,
}

#[derive(Serialize)]
pub(crate) struct ListResp {
    current_id: String,
    workspaces: Vec<WorkspaceDto>,
}

async fn list_workspaces(
    State(s): State<HttpState>,
    session: AuthSession,
) -> Result<Json<ListResp>, WsError> {
    let repo = WorkspaceRepo::new(&s.db);
    let mine = repo
        .list_for_user(&session.user_id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;
    // The Personal workspace is the default "current" hint. SPA persists
    // the user's actual choice in localStorage and overrides this.
    let current_id = mine
        .iter()
        .find(|w| matches!(w.kind, WorkspaceKind::Personal))
        .map(|w| w.id.clone())
        .or_else(|| mine.first().map(|w| w.id.clone()))
        .unwrap_or_default();
    Ok(Json(ListResp {
        current_id,
        workspaces: mine
            .into_iter()
            .map(|w| WorkspaceDto {
                id: w.id,
                name: w.name,
                kind: w.kind,
                owner_id: w.owner_id,
                role: w.role,
                member_count: w.member_count,
                created_at: rfc3339(w.created_at),
            })
            .collect(),
    }))
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
}

async fn create_workspace(
    State(s): State<HttpState>,
    session: AuthSession,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<WorkspaceDto>), WsError> {
    let name = sanitise_name(&body.name)?;
    let repo = WorkspaceRepo::new(&s.db);
    let w = repo
        .insert(&name, WorkspaceKind::Team, &session.user_id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace.create".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(w.id.clone()),
            target_name: Some(w.name.clone()),
            ip_address: None,
            metadata: None,
        },
    );

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceDto {
            id: w.id,
            name: w.name,
            kind: w.kind,
            owner_id: w.owner_id,
            role: WorkspaceRole::Owner,
            member_count: 1,
            created_at: rfc3339(w.created_at),
        }),
    ))
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

async fn rename_workspace(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<StatusCode, WsError> {
    let name = sanitise_name(&body.name)?;
    let repo = WorkspaceRepo::new(&s.db);
    let w = repo.find_by_id(&id).await.map_err(|_| WsError::NotFound)?;
    if w.owner_id != session.user_id {
        return Err(WsError::Forbidden);
    }
    if matches!(w.kind, WorkspaceKind::Personal) {
        return Err(WsError::Personal);
    }
    repo.rename(&id, &name)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace.rename".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id),
            target_name: Some(name),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_workspace(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, WsError> {
    let repo = WorkspaceRepo::new(&s.db);
    let w = repo.find_by_id(&id).await.map_err(|_| WsError::NotFound)?;
    if w.owner_id != session.user_id {
        return Err(WsError::Forbidden);
    }
    if matches!(w.kind, WorkspaceKind::Personal) {
        return Err(WsError::Personal);
    }
    repo.delete(&id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace.delete".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id),
            target_name: Some(w.name),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TransferBody {
    new_owner_id: String,
}

async fn transfer_ownership(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<TransferBody>,
) -> Result<StatusCode, WsError> {
    let repo = WorkspaceRepo::new(&s.db);
    let w = repo.find_by_id(&id).await.map_err(|_| WsError::NotFound)?;
    if w.owner_id != session.user_id {
        return Err(WsError::Forbidden);
    }
    if matches!(w.kind, WorkspaceKind::Personal) {
        return Err(WsError::Personal);
    }
    if body.new_owner_id == session.user_id {
        return Err(WsError::Validation("cannot transfer to yourself".into()));
    }
    let members = WorkspaceMemberRepo::new(&s.db);
    if members
        .role_of(&id, &body.new_owner_id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?
        .is_none()
    {
        return Err(WsError::NotAMember);
    }
    repo.transfer_owner(&id, &session.user_id, &body.new_owner_id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "workspace.transfer_owner".into(),
            target_kind: Some("workspace".into()),
            target_id: Some(id),
            target_name: Some(w.name),
            ip_address: None,
            metadata: Some(format!(
                r#"{{"from_user_id":"{}","to_user_id":"{}"}}"#,
                session.user_id, body.new_owner_id
            )),
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct MembersResp {
    members: Vec<MemberDto>,
}

#[derive(Serialize)]
struct MemberDto {
    user_id: String,
    role: WorkspaceRole,
    joined_at: String,
}

async fn list_members(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<MembersResp>, WsError> {
    let role = WorkspaceMemberRepo::new(&s.db)
        .role_of(&id, &session.user_id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?
        .ok_or(WsError::Forbidden)?;
    let _ = role; // any member can list; future RBAC may restrict.
    let mems = WorkspaceMemberRepo::new(&s.db)
        .list(&id)
        .await
        .map_err(|e| WsError::Internal(e.to_string()))?;
    Ok(Json(MembersResp {
        members: mems
            .into_iter()
            .map(|m| MemberDto {
                user_id: m.user_id,
                role: m.role,
                joined_at: rfc3339(m.joined_at),
            })
            .collect(),
    }))
}

fn sanitise_name(s: &str) -> Result<String, WsError> {
    let t = s.trim();
    if t.chars().count() < 2 || t.chars().count() > 60 {
        return Err(WsError::Validation(
            "workspace name must be 2–60 characters".into(),
        ));
    }
    Ok(t.to_string())
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route(
            "/api/workspaces/{id}",
            axum::routing::patch(rename_workspace).delete(delete_workspace),
        )
        .route("/api/workspaces/{id}/transfer", post(transfer_ownership))
        .route("/api/workspaces/{id}/members", get(list_members))
        .with_state(state)
        // Silences a clippy nag on the unused `delete` re-export when
        // every route uses the inline qualifier instead.
        .layer(tower::ServiceBuilder::new())
}

#[allow(dead_code)]
fn _hint(_d: fn()) {}
// Keep the `delete` import touched so rustc doesn't warn even though we
// resolved it via the `axum::routing::patch` path qualifier above.
#[allow(dead_code)]
const _USE_DELETE: fn() = || {
    let _: fn(axum::routing::MethodRouter<HttpState>) -> axum::routing::MethodRouter<HttpState> =
        delete;
};
