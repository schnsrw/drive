//! Top-level Drive error type. Specific crates layer their own errors and
//! convert up via `?` when crossing the boundary.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriveError {
    #[error("not found")]
    NotFound,
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("internal: {0}")]
    Internal(String),
}
