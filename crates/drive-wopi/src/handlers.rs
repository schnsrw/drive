//! WOPI endpoint handlers. The seven required endpoints from
//! `docs/research/01-wopi.md` §1.

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use drive_core::FileId;
use drive_storage::Storage;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};

use crate::{
    state::{LockEntry, WopiState},
    token::verify_token,
    WopiError,
};

const H_LOCK: HeaderName = HeaderName::from_static("x-wopi-lock");
const H_OLDLOCK: HeaderName = HeaderName::from_static("x-wopi-oldlock");
const H_OVERRIDE: HeaderName = HeaderName::from_static("x-wopi-override");
const H_ITEMVER: HeaderName = HeaderName::from_static("x-wopi-itemversion");

#[derive(Clone)]
pub struct WopiAppState {
    pub storage: Storage,
    pub wopi: WopiState,
    pub jwt_secret: Arc<[u8; 32]>,
}

impl std::fmt::Debug for WopiAppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WopiAppState")
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
pub(crate) struct TokenQuery {
    pub access_token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CheckFileInfo {
    pub base_file_name: String,
    pub owner_id: String,
    pub size: u64,
    pub user_id: String,
    pub version: String,
    pub user_can_write: bool,
    pub supports_update: bool,
    pub supports_locks: bool,
    pub supports_extended_lock_length: bool,
    pub is_anonymous_user: bool,
}

fn header_str<'a>(h: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok())
}

fn storage_key(id: FileId) -> String {
    format!("files/{id}")
}

// ─── CheckFileInfo (GET /wopi/files/{id}) ─────────────────────────────

pub(crate) async fn check_file_info(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
) -> Result<Response, WopiError> {
    let claims = verify_token(&s.jwt_secret, &access_token, id)?;
    let meta = s.wopi.get(id).await.ok_or(WopiError::NotFound)?;
    let size = match s.storage.stat(&storage_key(id)).await {
        Ok(m) => m.size,
        Err(drive_storage::StorageError::NotFound(_)) => 0,
        Err(e) => return Err(WopiError::Internal(e.to_string())),
    };
    let info = CheckFileInfo {
        base_file_name: meta.name,
        owner_id: "admin".into(),
        size,
        user_id: claims.user_id,
        version: meta.version.to_string(),
        user_can_write: claims.perms.can_write(),
        supports_update: true,
        supports_locks: true,
        supports_extended_lock_length: true,
        is_anonymous_user: false,
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(&info).unwrap(),
    )
        .into_response())
}

// ─── GetFile (GET /wopi/files/{id}/contents) ─────────────────────────

pub(crate) async fn get_file(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
) -> Result<Response, WopiError> {
    verify_token(&s.jwt_secret, &access_token, id)?;
    let meta = s.wopi.get(id).await.ok_or(WopiError::NotFound)?;
    let (_m, stream) = s
        .storage
        .get(&storage_key(id), None)
        .await
        .map_err(|_| WopiError::NotFound)?;
    let body = Body::from_stream(
        stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
    );
    let mut r = Response::new(body);
    r.headers_mut().insert(
        H_ITEMVER,
        HeaderValue::from_str(&meta.version.to_string()).unwrap(),
    );
    Ok(r)
}

// ─── PutFile (POST /wopi/files/{id}/contents) ────────────────────────

pub(crate) async fn put_file(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, WopiError> {
    let claims = verify_token(&s.jwt_secret, &access_token, id)?;
    if !claims.perms.can_write() {
        return Err(WopiError::Unauthorized);
    }
    let lock_header = header_str(&headers, &H_LOCK);

    let current_lock_opt: Option<String> = {
        let meta = s.wopi.get(id).await.ok_or(WopiError::NotFound)?;
        meta.lock.filter(|l| !l.expired()).map(|l| l.id)
    };
    let stat_size = match s.storage.stat(&storage_key(id)).await {
        Ok(m) => m.size,
        Err(drive_storage::StorageError::NotFound(_)) => 0,
        Err(e) => return Err(WopiError::Internal(e.to_string())),
    };
    match (current_lock_opt.as_deref(), lock_header) {
        (Some(cur), Some(h)) if cur == h => {}
        (Some(cur), _) => return Err(WopiError::LockConflict(cur.to_string())),
        // createnew: PutFile-without-lock allowed only on a 0-byte file.
        (None, _) if stat_size == 0 && body.is_empty() => {}
        (None, _) => return Err(WopiError::LockConflict(String::new())),
    }

    s.storage
        .put(&storage_key(id), body, None)
        .await
        .map_err(|e| WopiError::Internal(e.to_string()))?;

    let new_version = s
        .wopi
        .with_mut(id, |m| {
            m.version += 1;
            m.version
        })
        .await
        .ok_or(WopiError::NotFound)?;

    let mut r = Response::new(Body::empty());
    r.headers_mut().insert(
        H_ITEMVER,
        HeaderValue::from_str(&new_version.to_string()).unwrap(),
    );
    Ok(r)
}

// ─── Lock family dispatch (POST /wopi/files/{id}) ────────────────────

pub(crate) async fn lock_dispatch(
    state: State<WopiAppState>,
    path: Path<FileId>,
    query: Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, WopiError> {
    match header_str(&headers, &H_OVERRIDE) {
        Some("LOCK") => {
            if header_str(&headers, &H_OLDLOCK).is_some() {
                unlock_and_relock(state, path, query, headers).await
            } else {
                lock(state, path, query, headers).await
            }
        }
        Some("UNLOCK") => unlock(state, path, query, headers).await,
        Some("REFRESH_LOCK") => refresh_lock(state, path, query, headers).await,
        _ => Err(WopiError::BadRequest),
    }
}

async fn lock(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, WopiError> {
    verify_token(&s.jwt_secret, &access_token, id)?;
    let new_lock = header_str(&headers, &H_LOCK)
        .ok_or(WopiError::BadRequest)?
        .to_string();

    let current = s
        .wopi
        .get(id)
        .await
        .ok_or(WopiError::NotFound)?
        .lock
        .filter(|l| !l.expired())
        .map(|l| l.id);
    match current {
        Some(cur) if cur == new_lock => {
            // Lock with matching id = RefreshLock (spec §4).
            s.wopi
                .with_mut(id, |m| {
                    m.lock = Some(LockEntry {
                        id: new_lock,
                        acquired_at: time::OffsetDateTime::now_utc(),
                    });
                })
                .await;
        }
        Some(cur) => return Err(WopiError::LockConflict(cur)),
        None => {
            s.wopi
                .with_mut(id, |m| {
                    m.lock = Some(LockEntry {
                        id: new_lock,
                        acquired_at: time::OffsetDateTime::now_utc(),
                    });
                })
                .await;
        }
    }
    Ok(StatusCode::OK.into_response())
}

async fn unlock(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, WopiError> {
    verify_token(&s.jwt_secret, &access_token, id)?;
    let req = header_str(&headers, &H_LOCK)
        .ok_or(WopiError::BadRequest)?
        .to_string();
    let current = s
        .wopi
        .get(id)
        .await
        .ok_or(WopiError::NotFound)?
        .lock
        .filter(|l| !l.expired())
        .map(|l| l.id);
    match current {
        Some(cur) if cur == req => {
            s.wopi.with_mut(id, |m| m.lock = None).await;
            Ok(StatusCode::OK.into_response())
        }
        Some(cur) => Err(WopiError::LockConflict(cur)),
        None => Err(WopiError::LockConflict(String::new())),
    }
}

async fn refresh_lock(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, WopiError> {
    verify_token(&s.jwt_secret, &access_token, id)?;
    let req = header_str(&headers, &H_LOCK)
        .ok_or(WopiError::BadRequest)?
        .to_string();
    let current = s
        .wopi
        .get(id)
        .await
        .ok_or(WopiError::NotFound)?
        .lock
        .filter(|l| !l.expired())
        .map(|l| l.id);
    match current {
        Some(cur) if cur == req => {
            s.wopi
                .with_mut(id, |m| {
                    m.lock = Some(LockEntry {
                        id: req,
                        acquired_at: time::OffsetDateTime::now_utc(),
                    });
                })
                .await;
            Ok(StatusCode::OK.into_response())
        }
        Some(cur) => Err(WopiError::LockConflict(cur)),
        None => Err(WopiError::LockConflict(String::new())),
    }
}

async fn unlock_and_relock(
    State(s): State<WopiAppState>,
    Path(id): Path<FileId>,
    Query(TokenQuery { access_token }): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, WopiError> {
    verify_token(&s.jwt_secret, &access_token, id)?;
    let new_lock = header_str(&headers, &H_LOCK)
        .ok_or(WopiError::BadRequest)?
        .to_string();
    let old_lock = header_str(&headers, &H_OLDLOCK)
        .ok_or(WopiError::BadRequest)?
        .to_string();
    let current = s
        .wopi
        .get(id)
        .await
        .ok_or(WopiError::NotFound)?
        .lock
        .filter(|l| !l.expired())
        .map(|l| l.id);
    match current {
        Some(cur) if cur == old_lock => {
            s.wopi
                .with_mut(id, |m| {
                    m.lock = Some(LockEntry {
                        id: new_lock,
                        acquired_at: time::OffsetDateTime::now_utc(),
                    });
                })
                .await;
            Ok(StatusCode::OK.into_response())
        }
        Some(cur) => Err(WopiError::LockConflict(cur)),
        None => Err(WopiError::LockConflict(String::new())),
    }
}
