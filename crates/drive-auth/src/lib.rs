//! Single-tenant admin auth + session management.
//!
//! See [`docs/research/02-auth.md`](../../docs/research/02-auth.md) and
//! [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §"Three-token
//! identity model".
//!
//! - Sessions persisted via `drive-db`'s `SessionRepo`.
//! - Cookie shape: `__Host-cd_sid=<id>; Path=/; Secure; HttpOnly; SameSite=Lax`.
//! - Password hashing: argon2id, OWASP minimum (`m=19 MiB, t=2, p=1`).
//! - CSRF: per-session token in the DB, sent in the sign-in response, expected
//!   in `X-CSRF-Token` header on state-changing requests (enforced by callers).

#![forbid(unsafe_code)]

mod error;
mod extractor;
mod handlers;
pub mod oidc;
mod password;
mod router;
mod state;
mod token;

pub use error::AuthError;
pub use extractor::{AuthSession, OptionalAuthSession};
pub use handlers::build_session_cookie;
pub use oidc::{OidcClaims, OidcError};
pub use password::{hash_password, verify_password, OWASP_PARAMS};
pub use router::router;
pub use state::AuthState;
pub use token::{generate_csrf_token, generate_session_id};
