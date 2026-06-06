//! WOPI host implementation.
//!
//! See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §"WOPI host +
//! clients" + [`docs/research/01-wopi.md`](../../docs/research/01-wopi.md)
//! for the protocol contract. Phase 0 [`spike-02-wopi-host`]
//! (../../spikes/02-wopi-host/) proved the design.
//!
//! Lock storage in Phase 1 is in-memory (`Arc<Mutex<HashMap<FileId, ..>>>`).
//! Phase 2 moves it to the `wopi_locks` SQL table.

#![forbid(unsafe_code)]

mod error;
mod handlers;
mod router;
mod state;
mod token;

pub use error::WopiError;
pub use handlers::WopiAppState;
pub use router::router;
pub use state::{FileMeta, WopiState};
pub use token::{mint_token, verify_token, WopiClaims, WopiPerms};
