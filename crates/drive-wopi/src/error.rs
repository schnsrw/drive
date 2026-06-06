//! WOPI errors map directly to HTTP responses per the spec. The asymmetric
//! 409 + `X-WOPI-Lock` response header lives here — `LockConflict(String)`
//! carries the current lock id, and the `IntoResponse` impl emits the
//! mandatory header on 409 (and never on 200).

use axum::{
    body::Body,
    http::{header::HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use thiserror::Error;

const H_LOCK: HeaderName = HeaderName::from_static("x-wopi-lock");

#[derive(Debug, Error)]
pub enum WopiError {
    #[error("bad request")]
    BadRequest,
    #[error("unauthorized")]
    Unauthorized,
    #[error("not found")]
    NotFound,
    /// 409 + `X-WOPI-Lock: <current>` — mandatory + asymmetric per spec §4.
    #[error("lock conflict")]
    LockConflict(String),
    #[error("precondition failed")]
    PreconditionFailed,
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for WopiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST.into_response(),
            Self::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::PreconditionFailed => StatusCode::PRECONDITION_FAILED.into_response(),
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
            Self::Internal(msg) => {
                tracing::error!(error = %msg, "WOPI internal error");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
            Self::LockConflict(current) => {
                let mut r = Response::new(Body::empty());
                *r.status_mut() = StatusCode::CONFLICT;
                r.headers_mut().insert(
                    H_LOCK,
                    HeaderValue::from_str(&current).unwrap_or(HeaderValue::from_static("")),
                );
                r
            }
        }
    }
}
