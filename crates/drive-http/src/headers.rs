//! Security header constants per origin. See ARCHITECTURE.md §"Two-origin
//! security model". Layers are constructed inline in `lib.rs` for type
//! ergonomics (tower's Stack types are unwieldy as return types).

use axum::http::header::HeaderName;

pub const APP_CSP: &str = "default-src 'self'; \
                            script-src 'self'; \
                            object-src 'none'; \
                            base-uri 'none'; \
                            frame-ancestors 'none'";

pub const UCN_CSP: &str = "sandbox; default-src 'none'";

pub const REFERRER_POLICY: &str = "strict-origin-when-cross-origin";
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), interest-cohort=()";

pub const H_CSP: HeaderName = HeaderName::from_static("content-security-policy");
pub const H_XCTO: HeaderName = HeaderName::from_static("x-content-type-options");
pub const H_REF: HeaderName = HeaderName::from_static("referrer-policy");
pub const H_PP: HeaderName = HeaderName::from_static("permissions-policy");
pub const H_CORP: HeaderName = HeaderName::from_static("cross-origin-resource-policy");
