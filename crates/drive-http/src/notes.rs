//! Notes / Wiki HTTP routes. Pipeline §8.11.
//! Spec: docs/research/09-notes-wiki.md, docs/ux/16-notes-surface.md.
//!
//! Every route is workspace-membership-gated. Body cap is 1 MiB; titles
//! 1–200 chars. Each body save re-indexes `[[wiki-link]]` tokens into
//! `note_links`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use drive_auth::AuthSession;
use drive_db::{
    order_key_between, parse_wiki_links, AuditRepo, NewAuditEvent, NewNote, NoteLinksRepo,
    NotesRepo,
};
use serde::{Deserialize, Serialize};

use crate::HttpState;

const MAX_BODY_BYTES: usize = 1_048_576; // 1 MiB
const MIN_TITLE_CHARS: usize = 1;
const MAX_TITLE_CHARS: usize = 200;

#[derive(Debug)]
pub(crate) enum NotesError {
    Forbidden,
    NotFound,
    Validation(String),
    BodyTooLarge { limit: usize },
    Internal(String),
}

#[derive(Serialize)]
struct ErrBody<'a> {
    error: &'a str,
}

impl IntoResponse for NotesError {
    fn into_response(self) -> Response {
        match self {
            Self::Forbidden => {
                (StatusCode::FORBIDDEN, Json(ErrBody { error: "forbidden" })).into_response()
            }
            Self::NotFound => {
                (StatusCode::NOT_FOUND, Json(ErrBody { error: "not found" })).into_response()
            }
            Self::Validation(m) => {
                (StatusCode::BAD_REQUEST, Json(ErrBody { error: &m })).into_response()
            }
            Self::BodyTooLarge { limit } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(serde_json::json!({
                    "error": "note body too large",
                    "limit_bytes": limit,
                })),
            )
                .into_response(),
            Self::Internal(m) => {
                tracing::error!(error = %m, "notes handler error");
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

// ── Request / response bodies ────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct WorkspaceQuery {
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    pub q: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct CreateBody {
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
}

#[derive(Deserialize)]
pub(crate) struct PatchBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// `Some(None)` means JSON `null` — move to root. Custom deserialiser
    /// follows the same pattern as files.rs PatchBody.
    #[serde(default, deserialize_with = "deser_opt_opt_string")]
    pub parent_id: Option<Option<String>>,
    #[serde(default)]
    pub order_key: Option<String>,
}

fn deser_opt_opt_string<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<String>>, D::Error> {
    let v: Option<Option<String>> = Option::deserialize(d)?;
    Ok(v)
}

#[derive(Serialize)]
pub(crate) struct NoteDto {
    pub id: String,
    pub workspace_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: String,
    pub order_key: String,
    pub created_at: String,
    pub modified_at: String,
    pub backlinks: Vec<BacklinkDto>,
}

#[derive(Serialize)]
pub(crate) struct BacklinkDto {
    pub id: String,
    pub title: String,
}

#[derive(Serialize)]
pub(crate) struct NodeDto {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub order_key: String,
}

#[derive(Serialize)]
pub(crate) struct TreeResp {
    pub workspace_id: String,
    pub nodes: Vec<NodeDto>,
    pub trashed: Vec<NodeDto>,
}

// ── Handlers ─────────────────────────────────────────────────────────

async fn tree(
    State(s): State<HttpState>,
    session: AuthSession,
    Query(q): Query<WorkspaceQuery>,
) -> Result<Json<TreeResp>, NotesError> {
    let ws = resolve_workspace(&s, &session, q.workspace.as_deref()).await?;
    let repo = NotesRepo::new(&s.db);
    let nodes = repo
        .list_tree(&ws)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?
        .into_iter()
        .map(node_to_dto)
        .collect();
    let trashed = repo
        .list_trashed(&ws)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?
        .into_iter()
        .map(node_to_dto)
        .collect();
    Ok(Json(TreeResp {
        workspace_id: ws,
        nodes,
        trashed,
    }))
}

async fn search(
    State(s): State<HttpState>,
    session: AuthSession,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<NodeDto>>, NotesError> {
    let trimmed = q.q.as_deref().map_or("", str::trim);
    if trimmed.is_empty() {
        return Ok(Json(vec![]));
    }
    let ws = resolve_workspace(&s, &session, q.workspace.as_deref()).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = NotesRepo::new(&s.db)
        .search(&ws, trimmed, limit)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(node_to_dto).collect()))
}

async fn get_note(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<Json<NoteDto>, NotesError> {
    let notes = NotesRepo::new(&s.db);
    let note = notes
        .find_by_id(&id)
        .await
        .map_err(|_| NotesError::NotFound)?;
    require_membership(&s, &session, &note.workspace_id).await?;

    let backlinks = NoteLinksRepo::new(&s.db)
        .backlinks_for(&note.id, &note.title)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?
        .into_iter()
        .map(|b| BacklinkDto {
            id: b.note_id,
            title: b.title,
        })
        .collect();

    Ok(Json(to_dto(note, backlinks)))
}

async fn create(
    State(s): State<HttpState>,
    session: AuthSession,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<NoteDto>), NotesError> {
    let title = sanitise_title(&body.title)?;
    let ws = resolve_workspace(&s, &session, body.workspace_id.as_deref()).await?;
    let notes = NotesRepo::new(&s.db);

    if let Some(pid) = &body.parent_id {
        let parent = notes
            .find_by_id(pid)
            .await
            .map_err(|_| NotesError::NotFound)?;
        if parent.workspace_id != ws {
            return Err(NotesError::Validation(
                "parent_id is in a different workspace".into(),
            ));
        }
    }

    // Append to the end of the sibling list: pick a key strictly greater
    // than the current max under this parent.
    let last_key = notes
        .list_tree(&ws)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?
        .into_iter()
        .filter(|n| n.parent_id == body.parent_id)
        .map(|n| n.order_key)
        .max();
    let order_key = order_key_between(last_key.as_deref(), None);

    let note = notes
        .insert(&NewNote {
            workspace_id: ws,
            parent_id: body.parent_id.clone(),
            title: title.clone(),
            owner_id: session.user_id.clone(),
            order_key,
        })
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;

    // Newly created notes resolve any previously-dangling [[Title]]
    // references to this title within the workspace.
    NoteLinksRepo::new(&s.db)
        .reresolve_dangling(&note.title, &note.id, &note.workspace_id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "notes.create".into(),
            target_kind: Some("note".into()),
            target_id: Some(note.id.clone()),
            target_name: Some(note.title.clone()),
            ip_address: None,
            metadata: None,
        },
    );

    Ok((StatusCode::CREATED, Json(to_dto(note, vec![]))))
}

async fn patch_note(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
    Json(body): Json<PatchBody>,
) -> Result<Json<NoteDto>, NotesError> {
    let notes = NotesRepo::new(&s.db);
    let current = notes
        .find_by_id(&id)
        .await
        .map_err(|_| NotesError::NotFound)?;
    require_membership(&s, &session, &current.workspace_id).await?;

    // Validation.
    let new_title = if let Some(t) = body.title.as_deref() {
        Some(sanitise_title(t)?)
    } else {
        None
    };
    if let Some(ref new_body) = body.body {
        if new_body.len() > MAX_BODY_BYTES {
            return Err(NotesError::BodyTooLarge {
                limit: MAX_BODY_BYTES,
            });
        }
    }
    // parent_id sanity: target must be in the same workspace + must not
    // create a cycle. v0 cycle check is shallow — refuse moving a node
    // under itself or a known descendant via list_tree.
    if let Some(Some(ref pid)) = body.parent_id {
        let target = notes
            .find_by_id(pid)
            .await
            .map_err(|_| NotesError::NotFound)?;
        if target.workspace_id != current.workspace_id {
            return Err(NotesError::Validation(
                "parent_id is in a different workspace".into(),
            ));
        }
        if pid == &id || is_descendant(&notes, &current.workspace_id, &id, pid).await? {
            return Err(NotesError::Validation(
                "cannot move a note under itself or its descendant".into(),
            ));
        }
    }

    let updated = notes
        .update(
            &id,
            new_title.as_deref(),
            body.body.as_deref(),
            body.parent_id.as_ref().map(|p| p.as_deref()),
            body.order_key.as_deref(),
        )
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;

    // Re-index outgoing wiki-links whenever the body changed.
    if let Some(ref new_body) = body.body {
        let titles = parse_wiki_links(new_body);
        let resolved = notes
            .resolve_titles(&updated.workspace_id, &titles)
            .await
            .map_err(|e| NotesError::Internal(e.to_string()))?;
        let entries: Vec<(String, Option<String>)> = titles
            .into_iter()
            .map(|t| {
                let id = resolved.get(&t).cloned();
                (t, id)
            })
            .collect();
        NoteLinksRepo::new(&s.db)
            .replace_for_note(&updated.id, &entries)
            .await
            .map_err(|e| NotesError::Internal(e.to_string()))?;
    }

    // If the title changed, dangling links pointing at the new title
    // now resolve to this note.
    if let Some(ref new_title) = new_title {
        if new_title != &current.title {
            NoteLinksRepo::new(&s.db)
                .reresolve_dangling(new_title, &updated.id, &updated.workspace_id)
                .await
                .map_err(|e| NotesError::Internal(e.to_string()))?;
            AuditRepo::emit(
                &s.db,
                NewAuditEvent {
                    actor_id: Some(session.user_id.clone()),
                    actor_username: Some(session.username.clone()),
                    action: "notes.rename".into(),
                    target_kind: Some("note".into()),
                    target_id: Some(updated.id.clone()),
                    target_name: Some(new_title.clone()),
                    ip_address: None,
                    metadata: Some(format!(
                        r#"{{"old_title":{}}}"#,
                        serde_json::to_string(&current.title).unwrap_or_else(|_| "\"\"".into())
                    )),
                },
            );
        }
    }

    if body.body.is_some() {
        let delta =
            (body.body.as_ref().map_or(0, String::len) as i64) - (current.body.len() as i64);
        AuditRepo::emit(
            &s.db,
            NewAuditEvent {
                actor_id: Some(session.user_id.clone()),
                actor_username: Some(session.username.clone()),
                action: "notes.edit".into(),
                target_kind: Some("note".into()),
                target_id: Some(updated.id.clone()),
                target_name: Some(updated.title.clone()),
                ip_address: None,
                metadata: Some(format!(r#"{{"byte_delta":{delta}}}"#)),
            },
        );
    }

    let backlinks = NoteLinksRepo::new(&s.db)
        .backlinks_for(&updated.id, &updated.title)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?
        .into_iter()
        .map(|b| BacklinkDto {
            id: b.note_id,
            title: b.title,
        })
        .collect();
    Ok(Json(to_dto(updated, backlinks)))
}

async fn trash(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, NotesError> {
    let notes = NotesRepo::new(&s.db);
    let n = notes
        .find_by_id(&id)
        .await
        .map_err(|_| NotesError::NotFound)?;
    require_membership(&s, &session, &n.workspace_id).await?;
    notes
        .trash(&id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "notes.trash".into(),
            target_kind: Some("note".into()),
            target_id: Some(id),
            target_name: Some(n.title),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn restore(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, NotesError> {
    let notes = NotesRepo::new(&s.db);
    let n = notes
        .find_by_id(&id)
        .await
        .map_err(|_| NotesError::NotFound)?;
    require_membership(&s, &session, &n.workspace_id).await?;
    notes
        .restore(&id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "notes.restore".into(),
            target_kind: Some("note".into()),
            target_id: Some(id),
            target_name: Some(n.title),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_note(
    State(s): State<HttpState>,
    session: AuthSession,
    Path(id): Path<String>,
) -> Result<StatusCode, NotesError> {
    let notes = NotesRepo::new(&s.db);
    let n = notes
        .find_by_id(&id)
        .await
        .map_err(|_| NotesError::NotFound)?;
    require_membership(&s, &session, &n.workspace_id).await?;
    notes
        .delete(&id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(session.user_id.clone()),
            actor_username: Some(session.username.clone()),
            action: "notes.delete".into(),
            target_kind: Some("note".into()),
            target_id: Some(id),
            target_name: Some(n.title),
            ip_address: None,
            metadata: None,
        },
    );
    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ──────────────────────────────────────────────────────────

async fn resolve_workspace(
    s: &HttpState,
    session: &AuthSession,
    candidate: Option<&str>,
) -> Result<String, NotesError> {
    crate::workspaces::resolve_active_workspace(&s.db, &session.user_id, candidate)
        .await
        .map_err(|e| match e {
            crate::workspaces::WsError::Forbidden => NotesError::Forbidden,
            crate::workspaces::WsError::NotFound => NotesError::NotFound,
            other => NotesError::Internal(format!("workspace: {other:?}")),
        })
}

async fn require_membership(
    s: &HttpState,
    session: &AuthSession,
    workspace_id: &str,
) -> Result<(), NotesError> {
    let role = drive_db::WorkspaceMemberRepo::new(&s.db)
        .role_of(workspace_id, &session.user_id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    if role.is_none() {
        return Err(NotesError::Forbidden);
    }
    Ok(())
}

fn sanitise_title(s: &str) -> Result<String, NotesError> {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = t.chars().count();
    if len < MIN_TITLE_CHARS {
        return Err(NotesError::Validation("title is required".into()));
    }
    if len > MAX_TITLE_CHARS {
        return Err(NotesError::Validation(format!(
            "title must be ≤ {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(t)
}

async fn is_descendant(
    repo: &NotesRepo<'_>,
    workspace_id: &str,
    ancestor: &str,
    candidate: &str,
) -> Result<bool, NotesError> {
    // Walk parents of `candidate` up to root; return true if we encounter
    // `ancestor`. Bounded by tree depth.
    let nodes = repo
        .list_tree(workspace_id)
        .await
        .map_err(|e| NotesError::Internal(e.to_string()))?;
    let mut cur = Some(candidate.to_string());
    let mut hops = 0;
    while let Some(id) = cur {
        if id == ancestor {
            return Ok(true);
        }
        if hops > 1000 {
            break;
        }
        let next = nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.parent_id.clone());
        cur = next;
        hops += 1;
    }
    Ok(false)
}

fn rfc3339(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn node_to_dto(n: drive_db::NoteNode) -> NodeDto {
    NodeDto {
        id: n.id,
        parent_id: n.parent_id,
        title: n.title,
        order_key: n.order_key,
    }
}

fn to_dto(n: drive_db::Note, backlinks: Vec<BacklinkDto>) -> NoteDto {
    NoteDto {
        id: n.id,
        workspace_id: n.workspace_id,
        parent_id: n.parent_id,
        title: n.title,
        body: n.body,
        order_key: n.order_key,
        created_at: rfc3339(n.created_at),
        modified_at: rfc3339(n.modified_at),
        backlinks,
    }
}

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/notes", post(create))
        .route("/api/notes/tree", get(tree))
        .route("/api/notes/search", get(search))
        .route("/api/notes/{id}", get(get_note).patch(patch_note))
        .route("/api/notes/{id}", axum::routing::delete(delete_note))
        .route("/api/notes/{id}/trash", post(trash))
        .route("/api/notes/{id}/restore", post(restore))
        .with_state(state)
}

// Touch unused-imports to silence rustc on the patch import.
#[allow(dead_code)]
const _USE_PATCH: fn() = || {
    let _: fn(axum::routing::MethodRouter<HttpState>) -> axum::routing::MethodRouter<HttpState> =
        patch;
};
