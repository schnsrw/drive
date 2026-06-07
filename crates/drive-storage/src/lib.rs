//! Storage facade over `opendal::Operator`.
//!
//! See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §"Storage facade"
//! for the contract. Phase 0 [`spike-01-storage`](../../spikes/01-storage/)
//! proved the design; this crate is its Phase 1 home.

#![forbid(unsafe_code)]

pub mod byo;
pub mod registry;
pub mod secret_box;

pub use byo::{
    build_operator, ssrf_guard, test_connection, validate_shape as validate_shape_, ByoConfig,
    ByoError, Provider,
};
pub use registry::StorageRegistry;
pub use secret_box::{
    open as open_secret, parse_master_key_hex, seal as seal_secret, SecretBoxError,
};

use std::{ops::Range, sync::Arc, time::Duration as StdDuration};

use bytes::Bytes;
use drive_core::{Backend, Config};
use futures::{stream::BoxStream, TryStreamExt};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

pub type ByteStream = BoxStream<'static, Result<Bytes, StorageError>>;

#[derive(Debug, Clone)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub etag: Option<String>,
    pub modified: time::OffsetDateTime,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ListPage {
    pub entries: Vec<ObjectMeta>,
    pub next_token: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SignedUrl {
    /// Backend issued a native presigned URL (S3 / `MinIO`).
    Native {
        url: url::Url,
        expires_at: time::OffsetDateTime,
    },
    /// Self-minted HMAC token; serve via `/raw/{token}` on the user-content origin.
    Token {
        token: String,
        expires_at: time::OffsetDateTime,
    },
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid signed token")]
    InvalidToken,
    #[error("expired signed token")]
    ExpiredToken,
    #[error("backend error: {0}")]
    Backend(#[from] opendal::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("time error: {0}")]
    Time(#[from] time::error::ComponentRange),
    #[error("configuration error: {0}")]
    Config(String),
}

#[derive(Clone)]
pub struct Storage {
    op: opendal::Operator,
    sign_key: Arc<[u8; 32]>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("backend", &self.op.info().scheme())
            .finish_non_exhaustive()
    }
}

impl Storage {
    pub fn new(op: opendal::Operator, sign_key: [u8; 32]) -> Self {
        Self {
            op,
            sign_key: Arc::new(sign_key),
        }
    }

    /// Construct the right backend from a `Config`. The single entry point
    /// the binary uses; everything else goes through the methods below.
    pub fn from_config(cfg: &Config) -> Result<Self, StorageError> {
        let op = match cfg.backend {
            Backend::Fs => {
                let root = cfg
                    .fs_root
                    .as_deref()
                    .ok_or_else(|| StorageError::Config("DRIVE_FS_ROOT missing for fs".into()))?;
                opendal::Operator::new(opendal::services::Fs::default().root(root))?.finish()
            }
            Backend::Memory => {
                opendal::Operator::new(opendal::services::Memory::default())?.finish()
            }
            Backend::S3 | Backend::Minio => {
                let bucket = cfg
                    .s3_bucket
                    .as_deref()
                    .ok_or_else(|| StorageError::Config("DRIVE_S3_BUCKET missing".into()))?;
                let region = cfg.s3_region.as_deref().unwrap_or("auto");
                let mut builder = opendal::services::S3::default()
                    .bucket(bucket)
                    .region(region);
                if let Some(ep) = cfg.s3_endpoint.as_deref() {
                    builder = builder.endpoint(ep);
                }
                if let Some(k) = cfg.aws_access_key_id.as_deref() {
                    builder = builder.access_key_id(k);
                }
                if let Some(k) = cfg.aws_secret_access_key.as_deref() {
                    builder = builder.secret_access_key(k);
                }
                opendal::Operator::new(builder)?.finish()
            }
        };
        Ok(Self::new(op, cfg.signed_url_hmac_secret))
    }

    /// Convenience: filesystem at `root` with explicit signing key.
    pub fn fs(root: impl Into<String>, sign_key: [u8; 32]) -> Result<Self, StorageError> {
        let op =
            opendal::Operator::new(opendal::services::Fs::default().root(&root.into()))?.finish();
        Ok(Self::new(op, sign_key))
    }

    /// Convenience: in-memory storage (tests / ephemeral).
    pub fn memory(sign_key: [u8; 32]) -> Result<Self, StorageError> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?.finish();
        Ok(Self::new(op, sign_key))
    }

    pub fn capabilities(&self) -> opendal::Capability {
        self.op.info().full_capability()
    }

    pub async fn put(
        &self,
        key: &str,
        bytes: Bytes,
        _content_type: Option<&str>,
    ) -> Result<ObjectMeta, StorageError> {
        validate_key(key)?;
        self.op.write(key, bytes).await?;
        self.stat(key).await
    }

    pub async fn get(
        &self,
        key: &str,
        range: Option<Range<u64>>,
    ) -> Result<(ObjectMeta, ByteStream), StorageError> {
        validate_key(key)?;
        let meta = self.stat(key).await?;
        let reader = self.op.reader(key).await?;
        let stream = match range {
            Some(r) => reader.into_bytes_stream(r).await?,
            None => reader.into_bytes_stream(0..meta.size).await?,
        };
        let mapped: ByteStream = Box::pin(stream.map_err(StorageError::from));
        Ok((meta, mapped))
    }

    pub async fn stat(&self, key: &str) -> Result<ObjectMeta, StorageError> {
        validate_key(key)?;
        let m = self.op.stat(key).await.map_err(|e| match e.kind() {
            opendal::ErrorKind::NotFound => StorageError::NotFound(key.to_string()),
            _ => StorageError::Backend(e),
        })?;
        Ok(ObjectMeta {
            key: key.to_string(),
            size: m.content_length(),
            etag: m.etag().map(str::to_string),
            modified: m
                .last_modified()
                .map_or_else(time::OffsetDateTime::now_utc, |dt| {
                    time::OffsetDateTime::from_unix_timestamp(dt.timestamp())
                        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
                }),
            content_type: m.content_type().map(str::to_string),
        })
    }

    pub async fn delete(&self, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.op.delete(key).await?;
        Ok(())
    }

    /// Copy with capability-gated synthesis. Memory backend doesn't support
    /// native copy in `OpenDAL` 0.54; we read-then-write as fallback.
    pub async fn copy(&self, src: &str, dst: &str) -> Result<(), StorageError> {
        validate_key(src)?;
        validate_key(dst)?;
        if self.capabilities().copy {
            self.op.copy(src, dst).await?;
        } else {
            let body = self.op.read(src).await?;
            self.op.write(dst, body.to_bytes()).await?;
        }
        Ok(())
    }

    /// Rename = copy + delete on backends without native rename.
    pub async fn rename(&self, src: &str, dst: &str) -> Result<(), StorageError> {
        validate_key(src)?;
        validate_key(dst)?;
        if self.capabilities().rename {
            self.op.rename(src, dst).await?;
        } else {
            self.copy(src, dst).await?;
            self.op.delete(src).await?;
        }
        Ok(())
    }

    pub async fn list(
        &self,
        prefix: &str,
        _page_token: Option<&str>,
    ) -> Result<ListPage, StorageError> {
        // Phase 1: eager listing. Page-token support added once UI lazy-loads.
        let entries = self
            .op
            .list(prefix)
            .await?
            .into_iter()
            .map(|e| {
                let m = e.metadata();
                ObjectMeta {
                    key: e.path().to_string(),
                    size: m.content_length(),
                    etag: m.etag().map(str::to_string),
                    modified: m
                        .last_modified()
                        .map_or_else(time::OffsetDateTime::now_utc, |dt| {
                            time::OffsetDateTime::from_unix_timestamp(dt.timestamp())
                                .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
                        }),
                    content_type: m.content_type().map(str::to_string),
                }
            })
            .collect();
        Ok(ListPage {
            entries,
            next_token: None,
        })
    }

    pub async fn signed_get(&self, key: &str, ttl: StdDuration) -> Result<SignedUrl, StorageError> {
        validate_key(key)?;
        let expires_at = time::OffsetDateTime::now_utc() + ttl;
        if self.capabilities().presign_read {
            let req = self.op.presign_read(key, ttl).await?;
            let url = url::Url::parse(&req.uri().to_string()).map_err(|_| {
                StorageError::Backend(opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    "presign returned non-URL",
                ))
            })?;
            Ok(SignedUrl::Native { url, expires_at })
        } else {
            let token = self.mint_token(key, expires_at, "GET");
            Ok(SignedUrl::Token { token, expires_at })
        }
    }

    pub async fn signed_put(&self, key: &str, ttl: StdDuration) -> Result<SignedUrl, StorageError> {
        validate_key(key)?;
        let expires_at = time::OffsetDateTime::now_utc() + ttl;
        if self.capabilities().presign_write {
            let req = self.op.presign_write(key, ttl).await?;
            let url = url::Url::parse(&req.uri().to_string()).map_err(|_| {
                StorageError::Backend(opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    "presign returned non-URL",
                ))
            })?;
            Ok(SignedUrl::Native { url, expires_at })
        } else {
            let token = self.mint_token(key, expires_at, "PUT");
            Ok(SignedUrl::Token { token, expires_at })
        }
    }

    /// Verify a self-minted HMAC token. Returns `(key, method)` on success.
    pub fn verify_token(&self, token: &str) -> Result<(String, String), StorageError> {
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, token)
                .map_err(|_| StorageError::InvalidToken)?;
        if bytes.len() < 32 {
            return Err(StorageError::InvalidToken);
        }
        let (payload_bytes, tag) = bytes.split_at(bytes.len() - 32);
        let payload = std::str::from_utf8(payload_bytes).map_err(|_| StorageError::InvalidToken)?;
        let mut mac = HmacSha256::new_from_slice(self.sign_key.as_ref())
            .map_err(|_| StorageError::InvalidToken)?;
        mac.update(payload_bytes);
        let expected = mac.finalize().into_bytes();
        if expected.ct_eq(tag).unwrap_u8() != 1 {
            return Err(StorageError::InvalidToken);
        }
        let mut parts = payload.splitn(3, '\n');
        let method = parts.next().ok_or(StorageError::InvalidToken)?.to_string();
        let key = parts.next().ok_or(StorageError::InvalidToken)?.to_string();
        let exp_unix: i64 = parts
            .next()
            .ok_or(StorageError::InvalidToken)?
            .parse()
            .map_err(|_| StorageError::InvalidToken)?;
        let exp = time::OffsetDateTime::from_unix_timestamp(exp_unix)?;
        if exp < time::OffsetDateTime::now_utc() {
            return Err(StorageError::ExpiredToken);
        }
        Ok((key, method))
    }

    fn mint_token(&self, key: &str, expires_at: time::OffsetDateTime, method: &str) -> String {
        let payload = format!("{method}\n{key}\n{}", expires_at.unix_timestamp());
        let mut mac = HmacSha256::new_from_slice(self.sign_key.as_ref()).unwrap();
        mac.update(payload.as_bytes());
        let tag = mac.finalize().into_bytes();
        let mut combined = Vec::with_capacity(payload.len() + 32);
        combined.extend_from_slice(payload.as_bytes());
        combined.extend_from_slice(&tag);
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, combined)
    }
}

type HmacSha256 = Hmac<Sha256>;

fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty()
        || key.contains("..")
        || key.contains('\0')
        || key.starts_with('/')
        || key.contains('\\')
    {
        return Err(StorageError::InvalidKey(key.to_string()));
    }
    Ok(())
}
