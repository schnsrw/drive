//! Shared HTTP layer state. Cheap to clone — everything is `Arc` internally.

use std::sync::Arc;

use drive_core::Config;
use drive_storage::Storage;
use drive_wopi::WopiState;

#[derive(Clone)]
pub struct HttpState {
    pub storage: Storage,
    pub wopi: WopiState,
    pub jwt_secret: Arc<[u8; 32]>,
    pub config: Arc<Config>,
}

impl std::fmt::Debug for HttpState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpState")
            .field("storage", &self.storage)
            .field("backend", &self.config.backend)
            .finish_non_exhaustive()
    }
}
