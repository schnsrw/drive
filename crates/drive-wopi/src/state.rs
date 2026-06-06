//! In-memory WOPI state: per-file metadata + lock entries. Phase 2 moves
//! this into SQL tables (`files`, `wopi_locks`).

use std::{collections::HashMap, sync::Arc};

use drive_core::FileId;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub name: String,
    pub version: u32,
    pub lock: Option<LockEntry>,
}

#[derive(Debug, Clone)]
pub struct LockEntry {
    pub id: String,
    pub acquired_at: time::OffsetDateTime,
}

impl LockEntry {
    /// WOPI spec: locks auto-expire after 30 minutes unless refreshed.
    #[must_use]
    pub fn expired(&self) -> bool {
        time::OffsetDateTime::now_utc() - self.acquired_at > time::Duration::minutes(30)
    }
}

/// Phase-1 metadata store. Wraps a `Mutex<HashMap>`.
#[derive(Debug, Default, Clone)]
pub struct WopiState {
    inner: Arc<Mutex<HashMap<FileId, FileMeta>>>,
}

impl WopiState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a file's metadata. Called when the file is registered
    /// with the WOPI layer (Phase 1: in tests; Phase 2: by the file-API on upload).
    pub async fn register(&self, id: FileId, name: String) {
        self.inner.lock().await.insert(
            id,
            FileMeta {
                name,
                version: 1,
                lock: None,
            },
        );
    }

    pub async fn get(&self, id: FileId) -> Option<FileMeta> {
        self.inner.lock().await.get(&id).cloned()
    }

    pub(crate) async fn with_mut<F, R>(&self, id: FileId, f: F) -> Option<R>
    where
        F: FnOnce(&mut FileMeta) -> R,
    {
        let mut guard = self.inner.lock().await;
        guard.get_mut(&id).map(f)
    }
}
