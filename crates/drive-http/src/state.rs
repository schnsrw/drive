//! Shared HTTP layer state. Cheap to clone — everything is `Arc` internally.

use std::{sync::Arc, time::Instant};

use axum::extract::FromRef;
use drive_auth::AuthState;
use drive_core::Config;
use drive_db::Db;
use drive_storage::{Storage, StorageRegistry};
use drive_wopi::WopiState;

use crate::rate_limit::{RateLimitConfig, RateLimiter};

/// Process start instant, captured at first state construction. Drives the
/// Admin → System → Uptime readout. Static so we get "real" uptime even
/// across cheap HttpState clones in tests.
fn process_started_at() -> Instant {
    use std::sync::OnceLock;
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

#[derive(Clone)]
pub struct HttpState {
    pub storage: Storage,
    pub wopi: WopiState,
    pub db: Db,
    pub auth: AuthState,
    pub jwt_secret: Arc<[u8; 32]>,
    pub config: Arc<Config>,
    /// Upload-throttle bucket per user (pipeline §6.5). Cheap to clone
    /// — the limiter is `Arc<Mutex<HashMap>>`. Constructed via
    /// `HttpState::with_default_upload_limit` so call sites don't have
    /// to know the numbers.
    pub upload_limiter: Arc<RateLimiter>,
    /// Resolves the right storage adapter per file (pipeline §8.9).
    /// Wraps `storage` as the default + caches per-workspace BYO adapters.
    pub registry: Arc<StorageRegistry>,
    /// AES-256-GCM master key for sealing workspace storage secrets.
    /// `None` in tests + the dev path; required at boot in production
    /// when any workspace has BYO storage configured.
    pub storage_secret_key: Option<Arc<[u8; 32]>>,
}

impl HttpState {
    /// Default upload limiter: 30 uploads per minute per user (burst of
    /// 30, refill at 0.5 / sec). Adjust via the constructor below when
    /// the operator dials it down for shared instances.
    #[must_use]
    pub fn default_upload_limiter() -> Arc<RateLimiter> {
        Arc::new(RateLimiter::new(RateLimitConfig {
            capacity: 30.0,
            refill_per_sec: 0.5,
        }))
    }

    /// Build a `StorageRegistry` wrapping the given default storage.
    /// Tests that don't exercise BYO can use this — they get a registry
    /// with the same signing key, no cached BYO adapters.
    ///
    /// In production the binary builds this once at boot and stores it
    /// in `HttpState`; handlers go through `state.registry.for_byo(...)`
    /// (when a file's `storage_id` is set) or
    /// `state.registry.default_storage()` (when it isn't).
    #[must_use]
    pub fn default_registry(storage: Storage, sign_key: [u8; 32]) -> Arc<StorageRegistry> {
        Arc::new(StorageRegistry::new(Arc::new(storage), sign_key))
    }
}

impl HttpState {
    /// Seconds since the process started. Capped at `u64`.
    #[must_use]
    pub fn uptime_seconds(&self) -> u64 {
        process_started_at().elapsed().as_secs()
    }
}

impl std::fmt::Debug for HttpState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpState")
            .field("storage", &self.storage)
            .field("backend", &self.config.backend)
            .field("db_backend", &self.db.backend())
            .finish_non_exhaustive()
    }
}

// `FromRef` lets the AuthSession extractor pull AuthState out of HttpState
// at request time without forcing every handler to take both.
impl FromRef<HttpState> for AuthState {
    fn from_ref(state: &HttpState) -> Self {
        state.auth.clone()
    }
}
