//! Phase 3 §12 — OIDC HTTP endpoints.
//!
//! Three routes:
//!   - `GET /api/auth/oidc/metadata`  — public; SPA discovery for the IdP button.
//!   - `GET /api/auth/oidc/login`     — 302 to the IdP authorization URL.
//!   - `GET /api/auth/oidc/callback`  — exchange code → mint Drive session → 302 /.
//!
//! Spec: docs/research/12-oidc.md.

use axum::{
    extract::{Query, State},
    http::{header, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use drive_auth::{
    oidc::{begin, build_http_client, complete, upsert_user, OidcError},
    AuthState,
};
use drive_db::{AuditRepo, NewAuditEvent, NewSession, SessionRepo};
use serde::{Deserialize, Serialize};

use crate::HttpState;

#[derive(Serialize)]
pub(crate) struct MetadataResp {
    pub enabled: bool,
    /// When enabled, the human-readable label the SPA renders on the
    /// "Sign in with X" button. `null` when disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_label: Option<String>,
    /// When false, the SPA's password sign-in form is hidden too.
    pub allow_password_auth: bool,
}

/// `GET /api/auth/oidc/metadata` — public. The SPA hits this on first
/// load to decide whether to render the IdP button.
pub(crate) async fn metadata(State(s): State<HttpState>) -> Json<MetadataResp> {
    Json(MetadataResp {
        enabled: s.config.oidc.is_some(),
        provider_label: s.config.oidc.as_ref().map(|c| c.provider_label.clone()),
        allow_password_auth: s.config.allow_password_auth,
    })
}

#[derive(Deserialize)]
pub(crate) struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    /// IdP error code if the user denied consent or something else went wrong.
    pub error: Option<String>,
}

/// `GET /api/auth/oidc/login` — kick off a sign-in by 302'ing the
/// browser to the IdP. The `state` + PKCE verifier + nonce are persisted
/// in `oidc_flow_state` for the matching `/callback` to read.
pub(crate) async fn login(State(s): State<HttpState>) -> Result<Response, OidcRoute> {
    let cfg = s.config.oidc.as_ref().ok_or(OidcRoute::NotConfigured)?;
    let http = build_http_client().map_err(OidcRoute::from)?;
    let auth_url = begin(cfg, &s.db, &http).await.map_err(OidcRoute::from)?;
    redirect(auth_url.as_str())
}

/// `GET /api/auth/oidc/callback` — receive `code` + `state` from the IdP,
/// exchange for tokens, validate the ID token, find-or-create the user,
/// mint a Drive session cookie, 302 to `/`.
pub(crate) async fn callback(
    State(s): State<HttpState>,
    Query(q): Query<CallbackQuery>,
) -> Result<Response, OidcRoute> {
    let cfg = s.config.oidc.as_ref().ok_or(OidcRoute::NotConfigured)?;

    if let Some(err) = q.error.as_deref() {
        return Err(OidcRoute::IdpError(err.to_string()));
    }
    let code = q.code.ok_or(OidcRoute::MissingCode)?;
    let state = q.state.ok_or(OidcRoute::MissingState)?;

    let http = build_http_client().map_err(OidcRoute::from)?;
    let claims = complete(cfg, &s.db, &http, &state, &code)
        .await
        .map_err(OidcRoute::from)?;
    let user = upsert_user(cfg, &s.db, &claims)
        .await
        .map_err(OidcRoute::from)?;

    // Mint a Drive session — same shape the password handler uses so the
    // SPA can't tell which path the user came in via.
    let auth = AuthState::from_ref(&s);
    let sid = drive_auth::generate_session_id();
    let csrf = drive_auth::generate_csrf_token();
    SessionRepo::new(&s.db)
        .insert(
            &sid,
            &NewSession {
                user_id: user.id.clone(),
                csrf_token: csrf.clone(),
                ttl: auth.session_ttl,
            },
        )
        .await
        .map_err(|e| OidcRoute::Internal(e.to_string()))?;

    AuditRepo::emit(
        &s.db,
        NewAuditEvent {
            actor_id: Some(user.id.clone()),
            actor_username: Some(user.username.clone()),
            action: "auth.sign_in_oidc".into(),
            target_kind: Some("session".into()),
            target_id: Some(sid.clone()),
            target_name: None,
            ip_address: None,
            metadata: Some(format!(
                r#"{{"provider_id":{}}}"#,
                serde_json::to_string(&cfg.provider_id).unwrap_or_else(|_| "\"\"".into())
            )),
        },
    );

    let cookie = build_session_cookie(&sid, auth.cookie_secure, auth.session_ttl);
    let mut resp = redirect("/")?;
    resp.headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    // Also expose the CSRF token in a non-HttpOnly cookie so the SPA can
    // read it for state-changing requests (same pattern as the password
    // sign-in path's response body, but as a cookie because the user
    // never sees a JSON response on the redirect path).
    let csrf_cookie = format!(
        "cd_csrf={csrf}; Path=/; Max-Age={}; SameSite=Lax{secure}",
        auth.session_ttl.whole_seconds().max(0),
        secure = if auth.cookie_secure { "; Secure" } else { "" },
    );
    resp.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie).unwrap(),
    );
    Ok(resp)
}

// ── Errors ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum OidcRoute {
    NotConfigured,
    MissingCode,
    MissingState,
    IdpError(String),
    Oidc(OidcError),
    Internal(String),
}

impl From<OidcError> for OidcRoute {
    fn from(e: OidcError) -> Self {
        Self::Oidc(e)
    }
}

#[derive(Serialize)]
struct Err<'a> {
    error: &'a str,
}

impl IntoResponse for OidcRoute {
    fn into_response(self) -> Response {
        match self {
            Self::NotConfigured => (
                StatusCode::NOT_FOUND,
                Json(Err {
                    error: "OIDC is not enabled on this Drive",
                }),
            )
                .into_response(),
            Self::MissingCode => (
                StatusCode::BAD_REQUEST,
                Json(Err {
                    error: "callback missing ?code",
                }),
            )
                .into_response(),
            Self::MissingState => (
                StatusCode::BAD_REQUEST,
                Json(Err {
                    error: "callback missing ?state",
                }),
            )
                .into_response(),
            Self::IdpError(s) => {
                tracing::warn!(error = %s, "OIDC IdP reported an error on callback");
                redirect("/?oidc_error=idp").unwrap_or_else(|e| e.into_response())
            }
            Self::Oidc(OidcError::InvalidState) => {
                redirect("/?oidc_error=expired").unwrap_or_else(|e| e.into_response())
            }
            Self::Oidc(OidcError::Validation(m)) => {
                tracing::warn!(error = %m, "OIDC ID-token validation failed");
                redirect("/?oidc_error=token").unwrap_or_else(|e| e.into_response())
            }
            Self::Oidc(OidcError::UnknownSubject) => {
                redirect("/?oidc_error=unknown_subject").unwrap_or_else(|e| e.into_response())
            }
            Self::Oidc(other) => {
                tracing::warn!(error = %other, "OIDC handler error");
                (
                    StatusCode::BAD_GATEWAY,
                    Json(Err {
                        error: "couldn't reach the identity provider",
                    }),
                )
                    .into_response()
            }
            Self::Internal(m) => {
                tracing::error!(error = %m, "OIDC handler internal");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(Err {
                        error: "internal error",
                    }),
                )
                    .into_response()
            }
        }
    }
}

fn redirect(to: &str) -> Result<Response, OidcRoute> {
    let mut resp = (StatusCode::FOUND, ()).into_response();
    resp.headers_mut().insert(
        HeaderName::from_static("location"),
        HeaderValue::from_str(to).map_err(|e| OidcRoute::Internal(e.to_string()))?,
    );
    Ok(resp)
}

fn build_session_cookie(sid: &str, secure: bool, ttl: time::Duration) -> String {
    let name = if secure { "__Host-cd_sid" } else { "cd_sid" };
    let max_age = ttl.whole_seconds().max(0);
    let secure_part = if secure { "; Secure" } else { "" };
    format!("{name}={sid}; Path=/; HttpOnly{secure_part}; SameSite=Lax; Max-Age={max_age}")
}

pub(crate) fn router(state: HttpState) -> Router {
    Router::new()
        .route("/api/auth/oidc/metadata", get(metadata))
        .route("/api/auth/oidc/login", get(login))
        .route("/api/auth/oidc/callback", get(callback))
        .with_state(state)
}

// Used by lib.rs to convert HttpState into AuthState for the cookie
// shape — already wired via FromRef in state.rs.
use axum::extract::FromRef;
