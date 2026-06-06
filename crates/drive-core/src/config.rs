//! Runtime configuration loaded from environment variables.
//!
//! See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §"Configuration"
//! for the full env-var contract, mirrored in `.env.example`.

use std::net::SocketAddr;

use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Filesystem-backed storage rooted at a configured directory.
    Fs,
    /// In-process storage. Tests and ephemeral demos only — never prod.
    Memory,
    /// AWS S3 (or S3-protocol compatible service like Cloudflare R2).
    S3,
    /// MinIO — S3-protocol with a custom endpoint.
    Minio,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// e.g. `https://drive.casualoffice.org`.
    pub app_origin: Url,
    /// e.g. `https://usercontent-drive.casualoffice.org`. Must differ from
    /// `app_origin` in production (boot refuses to start otherwise).
    pub usercontent_origin: Url,
    pub bind: SocketAddr,
    pub backend: Backend,
    pub fs_root: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_endpoint: Option<String>,
    pub aws_access_key_id: Option<String>,
    pub aws_secret_access_key: Option<String>,
    pub db_url: String,
    pub body_limit_mb: u64,
    pub session_secret: Vec<u8>,
    pub wopi_hmac_secret: [u8; 32],
    pub signed_url_hmac_secret: [u8; 32],
    pub admin_user: String,
    pub admin_password_hash: String,
    pub recipient_footer: bool,
    pub is_prod: bool,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(&'static str),
    #[error("invalid {0}: {1}")]
    Invalid(&'static str, String),
    #[error("origins must differ in production (app == usercontent == {0})")]
    OriginsMatch(String),
    #[error("secret {0} too short — need 32 bytes (raw or base64)")]
    SecretTooShort(&'static str),
    #[error("secret {0} appears to be a development default — refuse to start in prod")]
    SecretIsDevDefault(&'static str),
    #[error("fs backend selected but DRIVE_FS_ROOT is missing")]
    FsRootMissing,
    #[error("S3/MinIO backend selected but {0} is missing")]
    S3FieldMissing(&'static str),
}

impl Config {
    /// Build a Config from the environment. Returns `ConfigError` on the
    /// first invariant violation. See `.env.example` for the contract.
    pub fn from_env() -> Result<Self, ConfigError> {
        let is_prod = env_bool("DRIVE_PROD").unwrap_or(false);

        let app_origin = env_url("DRIVE_APP_ORIGIN")?;
        let usercontent_origin = env_url("DRIVE_USERCONTENT_ORIGIN")?;
        if is_prod && app_origin == usercontent_origin {
            return Err(ConfigError::OriginsMatch(app_origin.to_string()));
        }

        let bind: SocketAddr = std::env::var("DRIVE_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                ConfigError::Invalid("DRIVE_BIND", e.to_string())
            })?;

        let backend = match std::env::var("DRIVE_BACKEND").as_deref() {
            Ok("fs") => Backend::Fs,
            Ok("memory") => Backend::Memory,
            Ok("s3") => Backend::S3,
            Ok("minio") => Backend::Minio,
            Ok(other) => return Err(ConfigError::Invalid("DRIVE_BACKEND", other.into())),
            Err(_) => Backend::Fs,
        };

        let fs_root = std::env::var("DRIVE_FS_ROOT").ok();
        let s3_bucket = std::env::var("DRIVE_S3_BUCKET").ok();
        let s3_region = std::env::var("DRIVE_S3_REGION").ok();
        let s3_endpoint = std::env::var("DRIVE_S3_ENDPOINT").ok();
        let aws_access_key_id = std::env::var("AWS_ACCESS_KEY_ID").ok();
        let aws_secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();

        match backend {
            Backend::Fs if fs_root.is_none() => return Err(ConfigError::FsRootMissing),
            Backend::S3 | Backend::Minio => {
                if s3_bucket.is_none() {
                    return Err(ConfigError::S3FieldMissing("DRIVE_S3_BUCKET"));
                }
                if aws_access_key_id.is_none() {
                    return Err(ConfigError::S3FieldMissing("AWS_ACCESS_KEY_ID"));
                }
                if aws_secret_access_key.is_none() {
                    return Err(ConfigError::S3FieldMissing("AWS_SECRET_ACCESS_KEY"));
                }
            }
            _ => {}
        }

        let db_url = std::env::var("DRIVE_DB_URL").unwrap_or_else(|_| "sqlite::memory:".into());

        let body_limit_mb: u64 = std::env::var("DRIVE_BODY_LIMIT_MB")
            .unwrap_or_else(|_| "100".into())
            .parse()
            .map_err(|e: std::num::ParseIntError| {
                ConfigError::Invalid("DRIVE_BODY_LIMIT_MB", e.to_string())
            })?;

        let session_secret = env_secret_bytes("DRIVE_SESSION_SECRET", is_prod)?;
        let wopi_hmac_secret = env_secret_32("DRIVE_WOPI_HMAC_SECRET", is_prod)?;
        let signed_url_hmac_secret = env_secret_32("DRIVE_SIGNED_URL_HMAC_SECRET", is_prod)?;

        let admin_user = std::env::var("DRIVE_ADMIN_USER").unwrap_or_else(|_| "admin".into());
        let admin_password_hash = std::env::var("DRIVE_ADMIN_PASSWORD_HASH")
            .map_err(|_| ConfigError::Missing("DRIVE_ADMIN_PASSWORD_HASH"))?;

        let recipient_footer = env_bool("DRIVE_RECIPIENT_FOOTER").unwrap_or(true);

        Ok(Self {
            app_origin,
            usercontent_origin,
            bind,
            backend,
            fs_root,
            s3_bucket,
            s3_region,
            s3_endpoint,
            aws_access_key_id,
            aws_secret_access_key,
            db_url,
            body_limit_mb,
            session_secret,
            wopi_hmac_secret,
            signed_url_hmac_secret,
            admin_user,
            admin_password_hash,
            recipient_footer,
            is_prod,
        })
    }

    /// The bare host (`host:port` for non-default ports) extracted from
    /// `app_origin`. Used by the Host-dispatch middleware.
    #[must_use]
    pub fn app_origin_host(&self) -> String {
        origin_host(&self.app_origin)
    }

    #[must_use]
    pub fn usercontent_origin_host(&self) -> String {
        origin_host(&self.usercontent_origin)
    }
}

fn origin_host(u: &Url) -> String {
    match (u.host_str(), u.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => String::new(),
    }
}

fn env_url(name: &'static str) -> Result<Url, ConfigError> {
    let raw = std::env::var(name).map_err(|_| ConfigError::Missing(name))?;
    Url::parse(&raw).map_err(|e| ConfigError::Invalid(name, e.to_string()))
}

fn env_bool(name: &str) -> Option<bool> {
    match std::env::var(name).ok()?.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_secret_bytes(name: &'static str, is_prod: bool) -> Result<Vec<u8>, ConfigError> {
    let raw = std::env::var(name).map_err(|_| ConfigError::Missing(name))?;
    if raw.len() < 32 {
        return Err(ConfigError::SecretTooShort(name));
    }
    if is_prod && is_dev_default(&raw) {
        return Err(ConfigError::SecretIsDevDefault(name));
    }
    Ok(raw.into_bytes())
}

fn env_secret_32(name: &'static str, is_prod: bool) -> Result<[u8; 32], ConfigError> {
    let bytes = env_secret_bytes(name, is_prod)?;
    let mut out = [0u8; 32];
    // Take the first 32 bytes; longer secrets are accepted but truncated for
    // fixed-width HMAC keys.
    out.copy_from_slice(&bytes[..32]);
    Ok(out)
}

fn is_dev_default(s: &str) -> bool {
    const KNOWN_BAD: &[&str] = &[
        "changeme",
        "change-me",
        "default",
        "dev-only-",
        "REPLACE_BEFORE_PROD",
    ];
    KNOWN_BAD.iter().any(|bad| s.contains(bad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_host_strips_default_port() {
        let u = Url::parse("https://drive.example.org").unwrap();
        assert_eq!(origin_host(&u), "drive.example.org");
    }

    #[test]
    fn origin_host_keeps_nondefault_port() {
        let u = Url::parse("http://127.0.0.1:8080").unwrap();
        assert_eq!(origin_host(&u), "127.0.0.1:8080");
    }

    #[test]
    fn dev_default_detection() {
        assert!(is_dev_default("dev-only-32-byte-secret-DO-NOT-USE-aa"));
        assert!(is_dev_default("changeme"));
        assert!(is_dev_default("REPLACE_BEFORE_PROD"));
        assert!(!is_dev_default("aZkP9wQ8r3X2nF7Yv5L1bH4mT0jC6dE9"));
    }
}
