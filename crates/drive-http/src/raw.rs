//! `/raw/{token}` on the user-content origin. Verifies the HMAC token
//! (issued by `Storage::signed_get` for fs/memory backends) and streams the
//! requested bytes with the documented security headers.

use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{header, header::HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use drive_storage::StorageError;
use futures::TryStreamExt;
use thiserror::Error;

use crate::HttpState;

#[derive(Debug, Error)]
pub(crate) enum RawError {
    #[error("invalid token")]
    InvalidToken,
    #[error("expired token")]
    ExpiredToken,
    #[error("method not allowed")]
    MethodNotAllowed,
    #[error("not found")]
    NotFound,
}

impl IntoResponse for RawError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidToken | Self::ExpiredToken => StatusCode::UNAUTHORIZED.into_response(),
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

pub(crate) async fn raw_get(
    State(state): State<HttpState>,
    Path(token): Path<String>,
    req: Request,
) -> Result<Response, RawError> {
    if req.method() != Method::GET {
        return Err(RawError::MethodNotAllowed);
    }
    let (key, method) = state.storage.verify_token(&token).map_err(map_storage)?;
    if method != "GET" {
        return Err(RawError::MethodNotAllowed);
    }
    let (meta, stream) = state
        .storage
        .get(&key, None)
        .await
        .map_err(|_| RawError::NotFound)?;

    let content_type = meta
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream")
        .to_string();
    let filename = key.rsplit('/').next().unwrap_or("file").to_string();
    let body = Body::from_stream(
        stream.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
    );

    let mut r = Response::new(body);
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename*=UTF-8''{}",
            url_encode(&filename)
        ))
        .unwrap_or(HeaderValue::from_static("attachment")),
    );
    h.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        crate::headers::H_CORP,
        HeaderValue::from_static("same-site"),
    );
    Ok(r)
}

fn map_storage(e: StorageError) -> RawError {
    match e {
        StorageError::InvalidToken => RawError::InvalidToken,
        StorageError::ExpiredToken => RawError::ExpiredToken,
        _ => RawError::InvalidToken,
    }
}

fn url_encode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c.to_string()]
            } else {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                bytes.bytes().map(|b| format!("%{b:02X}")).collect()
            }
        })
        .collect()
}
