//! `StorageRegistry` — routes file operations to the right backend.
//!
//! Wraps the server-default `Storage` plus a bounded cache of per-workspace
//! BYO adapters. Handlers that touch bytes look up the right adapter via
//! either:
//!
//! - [`StorageRegistry::default_storage`] — the server-wide default.
//! - [`StorageRegistry::for_byo`] — given an already-decrypted config + a
//!   stable cache key, returns (and caches) a workspace adapter.
//!
//! The cache key is `(workspace_storage.id, key_version)` so credential
//! rotation invalidates stale entries automatically — the new row carries
//! a higher key_version, the lookup misses, and a fresh adapter is built.
//!
//! The decryption itself lives in the http crate (where the master key is
//! held in `HttpState`) so this module stays free of DB / config concerns
//! and is unit-testable on its own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{byo::ByoConfig, Storage, StorageError};

/// Cap on cached BYO adapters. Far above any realistic workspace count
/// for v0; the LRU eviction is a placeholder pending a real workspace
/// scale-out story.
const CACHE_MAX: usize = 256;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct CacheKey {
    storage_id: String,
    key_version: i64,
}

pub struct StorageRegistry {
    default: Arc<Storage>,
    sign_key: [u8; 32],
    cache: Mutex<HashMap<CacheKey, Arc<Storage>>>,
}

impl std::fmt::Debug for StorageRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.cache.lock().map(|c| c.len()).unwrap_or(0);
        f.debug_struct("StorageRegistry")
            .field("default", &self.default)
            .field("cached_byo_adapters", &len)
            .finish_non_exhaustive()
    }
}

impl StorageRegistry {
    #[must_use]
    pub fn new(default: Arc<Storage>, sign_key: [u8; 32]) -> Self {
        Self {
            default,
            sign_key,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The server-wide default adapter — Personal-workspace path + the
    /// fallback for workspaces with no BYO configured.
    #[must_use]
    pub fn default_storage(&self) -> Arc<Storage> {
        Arc::clone(&self.default)
    }

    /// Build (or reuse a cached) adapter for a BYO row. Caller passes the
    /// already-decrypted [`ByoConfig`] alongside the row's id +
    /// `key_version`. We don't accept a `&Db` here — DB reads + secret
    /// decryption belong in the calling layer.
    ///
    /// SSRF + shape validation MUST happen before this call. We don't
    /// re-validate here because the row was validated at write time and
    /// re-running it on every read is wasted work.
    pub fn for_byo(
        &self,
        storage_id: &str,
        key_version: i64,
        cfg: &ByoConfig,
    ) -> Result<Arc<Storage>, StorageError> {
        let key = CacheKey {
            storage_id: storage_id.to_string(),
            key_version,
        };

        {
            let cache = self
                .cache
                .lock()
                .map_err(|_| StorageError::Config("registry cache lock poisoned".into()))?;
            if let Some(existing) = cache.get(&key) {
                return Ok(Arc::clone(existing));
            }
        }

        let op = crate::byo::build_operator(cfg)?;
        let storage = Arc::new(Storage::new(op, self.sign_key));

        let mut cache = self
            .cache
            .lock()
            .map_err(|_| StorageError::Config("registry cache lock poisoned".into()))?;
        // Soft cap; bulk-evict if we'd grow past it. Crude but bounded.
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&storage));
        Ok(storage)
    }

    /// Drop the cached adapter for `storage_id` regardless of version.
    /// Called when a workspace removes BYO storage so its next operation
    /// rebuilds against the default.
    pub fn invalidate(&self, storage_id: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|k, _| k.storage_id != storage_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byo::{ByoConfig, Provider};

    fn cfg() -> ByoConfig {
        ByoConfig {
            provider: Provider::S3,
            bucket: "test-bucket".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIATESTING".into(),
            secret_access_key: "secret".into(),
        }
    }

    fn registry() -> StorageRegistry {
        let storage = Storage::memory([7u8; 32]).expect("memory storage");
        StorageRegistry::new(Arc::new(storage), [7u8; 32])
    }

    #[test]
    fn default_returns_same_arc() {
        let r = registry();
        let a = r.default_storage();
        let b = r.default_storage();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn byo_cache_returns_same_arc() {
        let r = registry();
        let a = r.for_byo("ws-storage-1", 1, &cfg()).unwrap();
        let b = r.for_byo("ws-storage-1", 1, &cfg()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn key_version_bump_misses_cache() {
        let r = registry();
        let v1 = r.for_byo("ws-storage-1", 1, &cfg()).unwrap();
        let v2 = r.for_byo("ws-storage-1", 2, &cfg()).unwrap();
        assert!(!Arc::ptr_eq(&v1, &v2));
    }

    #[test]
    fn invalidate_drops_all_versions() {
        let r = registry();
        let v1 = r.for_byo("ws-storage-1", 1, &cfg()).unwrap();
        r.invalidate("ws-storage-1");
        let v1_again = r.for_byo("ws-storage-1", 1, &cfg()).unwrap();
        assert!(!Arc::ptr_eq(&v1, &v1_again));
    }
}
