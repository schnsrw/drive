//! Phase 3 §12 — OIDC sign-in.
//!
//! Pulls together: IdP discovery + key cache (via `openidconnect` 4.x),
//! PKCE verifier generation, state + nonce minting + verification, and
//! the claims-to-`User` mapping after a successful callback.
//!
//! HTTP wiring lives in `drive-http::oidc`. This module is the pure
//! protocol layer — no axum types leak in.
//!
//! Spec: docs/research/12-oidc.md.

use std::collections::HashSet;

use drive_core::OidcConfig;
use drive_db::{Db, DbError, NewOidcFlowState, OidcFlowStateRepo, User, UserRepo};
use openidconnect::{
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest as oidc_reqwest, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl,
    Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse,
};
use thiserror::Error;

const FLOW_TTL: time::Duration = time::Duration::minutes(10);

/// Claims extracted from a verified ID token, ready to map onto a `User`.
#[derive(Debug, Clone)]
pub struct OidcClaims {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub preferred_username: Option<String>,
    pub name: Option<String>,
    pub groups: HashSet<String>,
}

#[derive(Debug, Error)]
pub enum OidcError {
    #[error("OIDC is not configured on this Drive instance")]
    NotConfigured,
    #[error("discovery failed: {0}")]
    Discovery(String),
    #[error("invalid flow state — expired or replayed")]
    InvalidState,
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("ID token validation failed: {0}")]
    Validation(String),
    #[error("subject is unknown and auto-create is disabled")]
    UnknownSubject,
    #[error("internal: {0}")]
    Internal(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Build an HTTP client with the SSRF-safe redirect policy the spec
/// requires. Reuse this across discovery + token exchange — cheap to
/// pass by reference.
pub fn build_http_client() -> Result<oidc_reqwest::Client, OidcError> {
    oidc_reqwest::ClientBuilder::new()
        // OWASP SSRF prevention — never follow redirects in the OIDC client.
        .redirect(oidc_reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| OidcError::Internal(format!("reqwest build: {e}")))
}

/// Discover the IdP's metadata + build a `CoreClient` ready to use.
/// The `openidconnect` 4.x typestate system makes carrying the typed
/// client across function boundaries awkward, so `begin` + `complete`
/// each rebuild it inline. Discovery is a single HTTP round-trip with
/// reqwest's own caching upstream.
async fn build_client(
    cfg: &OidcConfig,
    http: &oidc_reqwest::Client,
) -> Result<
    CoreClient<
        openidconnect::EndpointSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointNotSet,
        openidconnect::EndpointMaybeSet,
        openidconnect::EndpointMaybeSet,
    >,
    OidcError,
> {
    let issuer = IssuerUrl::new(cfg.issuer.as_str().trim_end_matches('/').to_string())
        .map_err(|e| OidcError::Discovery(format!("issuer url: {e}")))?;
    let metadata = CoreProviderMetadata::discover_async(issuer, http)
        .await
        .map_err(|e| OidcError::Discovery(e.to_string()))?;
    let redirect = RedirectUrl::new(cfg.redirect_url.as_str().to_string())
        .map_err(|e| OidcError::Discovery(format!("redirect url: {e}")))?;
    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(cfg.client_id.clone()),
        Some(ClientSecret::new(cfg.client_secret.clone())),
    )
    .set_redirect_uri(redirect))
}

/// Begin a sign-in flow: persist PKCE + nonce + state, return the IdP
/// authorization URL the SPA should redirect to.
///
/// The state token is the lookup key for the persisted row; it's echoed
/// back as a query param by the IdP at callback time. PKCE challenge
/// derives from a verifier that NEVER leaves Drive — even a successful
/// auth-code interception can't be redeemed without it.
pub async fn begin(
    cfg: &OidcConfig,
    db: &Db,
    http: &oidc_reqwest::Client,
) -> Result<url::Url, OidcError> {
    let client = build_client(cfg, http).await?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let mut builder = client.authorize_url(
        CoreAuthenticationFlow::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );
    for scope in &cfg.scopes {
        builder = builder.add_scope(Scope::new(scope.clone()));
    }
    let (auth_url, csrf_token, nonce) = builder.set_pkce_challenge(pkce_challenge).url();

    OidcFlowStateRepo::new(db)
        .insert(&NewOidcFlowState {
            state: csrf_token.secret().clone(),
            pkce_verifier: pkce_verifier.into_secret(),
            nonce: nonce.secret().clone(),
            ttl: FLOW_TTL,
        })
        .await?;

    Ok(auth_url)
}

/// Finalize a sign-in: validate state, exchange code → tokens, verify
/// ID-token signature + nonce + audience, return parsed claims.
pub async fn complete(
    cfg: &OidcConfig,
    db: &Db,
    http: &oidc_reqwest::Client,
    state: &str,
    code: &str,
) -> Result<OidcClaims, OidcError> {
    let client = build_client(cfg, http).await?;
    let flow = OidcFlowStateRepo::new(db)
        .take(state)
        .await
        .map_err(|_| OidcError::InvalidState)?;

    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .map_err(|e| OidcError::TokenExchange(e.to_string()))?
        .set_pkce_verifier(PkceCodeVerifier::new(flow.pkce_verifier))
        .request_async(http)
        .await
        .map_err(|e| OidcError::TokenExchange(e.to_string()))?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| OidcError::Validation("no id_token in response".into()))?;

    let verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, &Nonce::new(flow.nonce.clone()))
        .map_err(|e| OidcError::Validation(e.to_string()))?;

    let subject = claims.subject().to_string();
    let email = claims.email().map(|e| e.to_string());
    let email_verified = claims.email_verified().unwrap_or(false);
    let preferred_username = claims.preferred_username().map(|u| u.as_str().to_string());
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|n| n.as_str().to_string());

    // The standard openidconnect crate types don't expose `groups` as a
    // first-class claim (it's an IdP-specific extension). Pull it via the
    // additional-claims JSON. Authentik / Keycloak / Entra all emit
    // `groups` as an array of strings.
    let additional = claims.additional_claims();
    let groups: HashSet<String> = serde_json::to_value(additional)
        .ok()
        .and_then(|v| v.get("groups").cloned())
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Discard the refresh token deliberately — spec calls this out. Drive
    // mints its own session cookie below; we never use the IdP's refresh
    // path. _vars to silence "unused" lints.
    let _ = token_response.access_token();

    Ok(OidcClaims {
        subject,
        email,
        email_verified,
        preferred_username,
        name,
        groups,
    })
}

/// Map OIDC claims onto a `User` row: find by (provider_id, subject),
/// else auto-create when allowed, else error. Updates `is_admin` from
/// the `admin_group` claim on every sign-in so role drift in the IdP
/// propagates immediately.
pub async fn upsert_user(
    cfg: &OidcConfig,
    db: &Db,
    claims: &OidcClaims,
) -> Result<User, OidcError> {
    let users = UserRepo::new(db);
    let is_admin = cfg
        .admin_group
        .as_deref()
        .is_some_and(|g| claims.groups.contains(g));

    match users.find_by_oidc(&cfg.provider_id, &claims.subject).await {
        Ok(user) => {
            if user.is_admin != is_admin {
                users.set_admin(&user.id, is_admin).await?;
            }
            Ok(user)
        }
        Err(DbError::NotFound) => {
            if !cfg.auto_create_users {
                return Err(OidcError::UnknownSubject);
            }
            let username = derive_username(claims);
            let created = users
                .insert_oidc(
                    &username,
                    is_admin,
                    &cfg.provider_id,
                    &claims.subject,
                    claims.email_verified,
                )
                .await?;
            Ok(created)
        }
        Err(e) => Err(OidcError::Db(e)),
    }
}

/// Pick a display username from the claims. Preference order:
/// `preferred_username` → email local-part → slugified name + short
/// subject → `oidc-<short subject>`.
fn derive_username(claims: &OidcClaims) -> String {
    if let Some(u) = claims
        .preferred_username
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        return u.to_string();
    }
    if let Some(email) = claims.email.as_deref() {
        if let Some((local, _)) = email.split_once('@') {
            if !local.is_empty() {
                return local.to_string();
            }
        }
    }
    if let Some(n) = claims.name.as_deref().filter(|s| !s.is_empty()) {
        return format!("{}-{}", slug(n), short_subject(&claims.subject));
    }
    format!("oidc-{}", short_subject(&claims.subject))
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn short_subject(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(s.as_bytes());
    h.iter()
        .take(3)
        .fold(String::with_capacity(6), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(&mut acc, "{b:02x}");
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn claims_with(
        subject: &str,
        email: Option<&str>,
        preferred: Option<&str>,
        name: Option<&str>,
    ) -> OidcClaims {
        OidcClaims {
            subject: subject.into(),
            email: email.map(str::to_string),
            email_verified: false,
            preferred_username: preferred.map(str::to_string),
            name: name.map(str::to_string),
            groups: HashSet::new(),
        }
    }

    #[test]
    fn derive_prefers_preferred_username() {
        let c = claims_with("s1", Some("a@b.com"), Some("alice"), Some("Alice"));
        assert_eq!(derive_username(&c), "alice");
    }

    #[test]
    fn derive_falls_back_to_email_local() {
        let c = claims_with("s1", Some("alice@example.com"), None, None);
        assert_eq!(derive_username(&c), "alice");
    }

    #[test]
    fn derive_falls_back_to_slugified_name() {
        let c = claims_with("subject-1", None, None, Some("Alice Doe"));
        let suffix = short_subject("subject-1");
        assert_eq!(derive_username(&c), format!("alice-doe-{suffix}"));
    }

    #[test]
    fn derive_final_fallback_is_oidc_short() {
        let c = claims_with("subject-1", None, None, None);
        let suffix = short_subject("subject-1");
        assert_eq!(derive_username(&c), format!("oidc-{suffix}"));
    }

    #[test]
    fn slug_strips_specials() {
        assert_eq!(slug("Alice O'Brien"), "alice-o-brien");
        assert_eq!(slug("--leading--"), "leading");
    }
}
