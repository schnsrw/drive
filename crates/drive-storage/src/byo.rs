//! Bring-your-own storage primitives.
//!
//! - [`Provider`] — wire-format enum, one variant per supported backend.
//! - [`ByoConfig`] — the values the SPA + handler pass to test or save.
//! - [`build_operator`] — assembles an OpenDAL operator from a `ByoConfig`.
//! - [`ssrf_guard`] — enforces the endpoint allow-list before any test request.
//! - [`test_connection`] — round-trips a 1-byte put/stat/delete; returns latency.
//!
//! Spec: docs/research/08-byo-storage.md §"Threat model" + §"Test-connection".

use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::StorageError;

/// Stored in the database. The wire format must stay stable — bump a
/// migration if a new provider needs different fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Amazon S3.
    S3,
    /// MinIO. Uses S3 client path; `endpoint` is required.
    Minio,
    /// Cloudflare R2. Uses S3 client path; `endpoint` is required.
    R2,
    /// Backblaze B2 (S3-compatible API).
    B2,
}

impl Provider {
    /// Endpoint is required for everything except plain AWS S3.
    #[must_use]
    pub fn endpoint_required(&self) -> bool {
        !matches!(self, Provider::S3)
    }

    /// Returns the canonical name used in audit metadata + UI.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::S3 => "s3",
            Provider::Minio => "minio",
            Provider::R2 => "r2",
            Provider::B2 => "b2",
        }
    }
}

/// Caller-supplied storage config. `secret_access_key` is plaintext on
/// the wire — handlers must redact it from logs (see
/// `crates/drive-http/src/audit.rs::scrub_secrets`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ByoConfig {
    pub provider: Provider,
    pub bucket: String,
    pub region: String,
    /// Required when [`Provider::endpoint_required`].
    #[serde(default)]
    pub endpoint: Option<String>,
    pub access_key_id: String,
    pub secret_access_key: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ByoError {
    #[error("invalid config: {0}")]
    Invalid(&'static str),
    #[error("endpoint blocked by SSRF guard: {0}")]
    SsrfBlocked(&'static str),
    #[error("test connection failed: {0}")]
    TestFailed(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Validates basic shape before any further work. Cheap. Run BEFORE
/// `ssrf_guard` so callers see the most specific error first.
pub fn validate_shape(cfg: &ByoConfig) -> Result<(), ByoError> {
    if cfg.bucket.trim().is_empty() {
        return Err(ByoError::Invalid("bucket is required"));
    }
    if cfg.region.trim().is_empty() {
        return Err(ByoError::Invalid("region is required"));
    }
    if cfg.access_key_id.trim().is_empty() {
        return Err(ByoError::Invalid("access_key_id is required"));
    }
    if cfg.secret_access_key.trim().is_empty() {
        return Err(ByoError::Invalid("secret_access_key is required"));
    }
    if cfg.provider.endpoint_required() {
        let ep = cfg
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(ByoError::Invalid("endpoint is required for this provider"))?;
        // Parsing here catches typos early — full SSRF check happens in
        // `ssrf_guard`.
        url::Url::parse(ep).map_err(|_| ByoError::Invalid("endpoint must be a valid URL"))?;
    }
    Ok(())
}

/// Server-side request forgery guard. Refuses to point the storage adapter
/// at: non-HTTP(S) schemes, the AWS instance metadata service, link-local
/// or loopback hosts, RFC1918 private ranges. Insecure HTTP is rejected
/// unless the host is loopback / private AND the operator opted in via
/// `DRIVE_ALLOW_INSECURE_BYO=true`.
///
/// Called BEFORE any DNS lookup or HTTP request leaves the process. The
/// downstream OpenDAL client may still resolve hostnames that pass this
/// check to one of the blocked IPs — a second pass at the socket level
/// is in scope for v0.2, called out in the brief.
pub fn ssrf_guard(endpoint: Option<&str>, allow_insecure: bool) -> Result<(), ByoError> {
    let Some(raw) = endpoint else {
        // No endpoint = AWS S3 default endpoint, which is well-known + safe.
        return Ok(());
    };
    let url =
        url::Url::parse(raw).map_err(|_| ByoError::Invalid("endpoint must be a valid URL"))?;

    // Scheme.
    let scheme = url.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(ByoError::SsrfBlocked("only http(s) endpoints are allowed"));
    }
    let is_https = scheme == "https";

    let host = url
        .host_str()
        .ok_or(ByoError::SsrfBlocked("endpoint host is missing"))?
        .to_ascii_lowercase();

    // Static block list — well-known cloud metadata IPs + obvious anti-patterns.
    // Refused regardless of `allow_insecure`.
    for bad in BLOCKED_HOSTS {
        if host == *bad {
            return Err(ByoError::SsrfBlocked(
                "endpoint host is on the metadata block list",
            ));
        }
    }

    let private_like = is_private_or_loopback_host(&host);
    if private_like && !allow_insecure {
        return Err(ByoError::SsrfBlocked(
            "loopback / private endpoints require DRIVE_ALLOW_INSECURE_BYO=true",
        ));
    }
    if !is_https && !private_like {
        return Err(ByoError::SsrfBlocked(
            "non-https endpoint requires a loopback / private host",
        ));
    }

    Ok(())
}

/// Hosts we never let storage talk to. Add new entries here, not in a
/// runtime config — operators shouldn't be able to permit these.
const BLOCKED_HOSTS: &[&str] = &[
    "169.254.169.254", // AWS / GCP / Azure metadata
    "metadata.google.internal",
    "metadata",
    "::ffff:169.254.169.254",
];

fn is_private_or_loopback_host(host: &str) -> bool {
    use std::net::IpAddr;
    // Hostname form: anything that ends in .localhost or is exactly the
    // text 'localhost' is loopback by convention. Real DNS resolution
    // happens later; this is the cheap upfront check.
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        // Real hostname (not an IP literal) — pass; downstream operators
        // can apply finer-grained DNS-time filtering if needed.
        return false;
    };
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // 100.64.0.0/10 — carrier-grade NAT, sometimes used internally
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // ULA fc00::/7
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

/// Builds an OpenDAL S3 operator from the config — does not touch the
/// network. Used by both the cached adapter path and the one-shot
/// test-connection path. Caller MUST have run [`ssrf_guard`] first.
pub fn build_operator(cfg: &ByoConfig) -> Result<opendal::Operator, StorageError> {
    let mut builder = opendal::services::S3::default()
        .bucket(cfg.bucket.trim())
        .region(cfg.region.trim())
        .access_key_id(cfg.access_key_id.trim())
        .secret_access_key(cfg.secret_access_key.trim());
    if let Some(ep) = cfg
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.endpoint(ep);
    }
    Ok(opendal::Operator::new(builder)?.finish())
}

/// Round-trip put → stat → delete on a random key. Returns wall-clock
/// latency in milliseconds. Times out via OpenDAL's default; callers
/// should additionally wrap in `tokio::time::timeout` if a stricter
/// budget matters (the test-connection handler imposes 12s).
pub async fn test_connection(cfg: &ByoConfig) -> Result<u64, ByoError> {
    let op = build_operator(cfg)?;
    let key = format!("drive-test-{}", ulid::Ulid::new());
    let payload = b"ok".to_vec();
    let started = Instant::now();
    op.write(&key, payload)
        .await
        .map_err(|e| ByoError::TestFailed(format!("write: {e}")))?;
    op.stat(&key)
        .await
        .map_err(|e| ByoError::TestFailed(format!("stat: {e}")))?;
    op.delete(&key)
        .await
        .map_err(|e| ByoError::TestFailed(format!("delete: {e}")))?;
    Ok(started.elapsed().as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssrf_blocks_metadata_ip() {
        let err = ssrf_guard(Some("http://169.254.169.254/latest/meta-data"), true).unwrap_err();
        assert!(matches!(err, ByoError::SsrfBlocked(_)));
    }

    #[test]
    fn ssrf_blocks_loopback_without_opt_in() {
        let err = ssrf_guard(Some("http://127.0.0.1:9000"), false).unwrap_err();
        assert!(matches!(err, ByoError::SsrfBlocked(_)));
    }

    #[test]
    fn ssrf_allows_loopback_with_opt_in() {
        ssrf_guard(Some("http://127.0.0.1:9000"), true).unwrap();
        ssrf_guard(Some("http://localhost:9000"), true).unwrap();
    }

    #[test]
    fn ssrf_rejects_http_for_public_hosts() {
        let err = ssrf_guard(Some("http://s3.example.com"), true).unwrap_err();
        assert!(matches!(err, ByoError::SsrfBlocked(_)));
    }

    #[test]
    fn ssrf_allows_https_for_public_hosts() {
        ssrf_guard(Some("https://s3.amazonaws.com"), false).unwrap();
        ssrf_guard(Some("https://minio.example.com:9000"), false).unwrap();
    }

    #[test]
    fn ssrf_rejects_private_ranges() {
        for ep in [
            "http://10.0.0.1",
            "http://192.168.1.5:9000",
            "http://172.16.4.4",
        ] {
            assert!(matches!(
                ssrf_guard(Some(ep), false).unwrap_err(),
                ByoError::SsrfBlocked(_)
            ));
        }
    }

    #[test]
    fn ssrf_rejects_unknown_scheme() {
        assert!(matches!(
            ssrf_guard(Some("file:///etc/passwd"), true).unwrap_err(),
            ByoError::SsrfBlocked(_)
        ));
        assert!(matches!(
            ssrf_guard(Some("gopher://example.com"), true).unwrap_err(),
            ByoError::SsrfBlocked(_)
        ));
    }

    #[test]
    fn ssrf_no_endpoint_is_ok() {
        ssrf_guard(None, false).unwrap();
    }

    #[test]
    fn shape_validation_catches_empties() {
        let mut cfg = ByoConfig {
            provider: Provider::S3,
            bucket: "b".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIA".into(),
            secret_access_key: "SHH".into(),
        };
        validate_shape(&cfg).unwrap();

        cfg.bucket = "".into();
        assert!(validate_shape(&cfg).is_err());

        cfg.bucket = "b".into();
        cfg.provider = Provider::Minio;
        // MinIO needs an endpoint.
        assert!(validate_shape(&cfg).is_err());
        cfg.endpoint = Some("https://minio.local:9000".into());
        validate_shape(&cfg).unwrap();
    }
}
