//! HTTP layer for Casual Drive. Assembles the Axum router that serves both
//! the app origin (`drive.<host>`) and the user-content origin
//! (`usercontent-drive.<host>`) from one binary.
//!
//! See [`docs/ARCHITECTURE.md`](../../docs/ARCHITECTURE.md) §"Two-origin
//! security model".

#![forbid(unsafe_code)]

mod about;
mod activity;
mod admin;
mod files;
pub mod headers;
mod host_dispatch;
mod rate_limit;
mod raw;
mod search;
mod share;
mod spa;
mod state;
mod workspaces;

pub use rate_limit::{RateLimitConfig, RateLimiter};
pub use state::HttpState;

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    middleware,
    response::IntoResponse,
    routing::get,
    Router,
};
use drive_auth::AuthSession;
use drive_wopi::WopiAppState;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    headers::{APP_CSP, H_CSP, H_PP, H_REF, H_XCTO, PERMISSIONS_POLICY, REFERRER_POLICY, UCN_CSP},
    host_dispatch::{host_dispatch, Origin},
};

/// Top-level Drive router. Assembles both origins.
pub fn router(state: HttpState) -> Router {
    Router::new()
        .merge(app_origin_router(state.clone()))
        .merge(usercontent_router(state))
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

#[derive(serde::Serialize)]
struct Me {
    admin: String,
    backend: String,
    user_id: String,
    is_admin: bool,
    /// Bytes the caller has stored, summed over their non-trashed files.
    used_bytes: u64,
    /// Per-user storage cap; `None` means unlimited.
    quota_bytes: Option<u64>,
}

/// `/api/me` requires an authenticated session — returns 401 for the SPA's
/// initial bootstrap when no cookie is present, so AuthContext falls back
/// to the SignIn page instead of going straight to the shell.
async fn api_me(State(s): State<HttpState>, session: AuthSession) -> axum::Json<Me> {
    let users = drive_db::UserRepo::new(&s.db);
    let used_bytes = users.used_bytes(&session.user_id).await.unwrap_or(0);
    let quota_bytes = users
        .find_by_id(&session.user_id)
        .await
        .ok()
        .and_then(|u| u.quota_bytes);
    axum::Json(Me {
        admin: session.username.clone(),
        backend: format!("{:?}", s.config.backend),
        user_id: session.user_id,
        is_admin: session.is_admin,
        used_bytes,
        quota_bytes,
    })
}

fn app_origin_router(state: HttpState) -> Router {
    let wopi_state = WopiAppState {
        storage: state.storage.clone(),
        wopi: state.wopi.clone(),
        jwt_secret: state.jwt_secret.clone(),
    };
    let wopi_router: Router = drive_wopi::router(wopi_state);
    let auth_router: Router = drive_auth::router(state.auth.clone());
    let body_limit_bytes = (state.config.body_limit_mb as usize)
        .saturating_mul(1024)
        .saturating_mul(1024);
    let files_router: Router = files::router(state.clone(), body_limit_bytes);
    let share_router: Router = share::router(state.clone());
    let workspaces_router: Router = workspaces::router(state.clone());

    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/me", get(api_me))
        .route("/api/about", get(about::about))
        .route("/api/activity", get(activity::list_activity))
        .route("/api/admin/system", get(admin::system))
        .route("/api/search", get(search::search))
        .with_state(state.clone())
        .merge(wopi_router)
        .merge(auth_router)
        .merge(files_router)
        .merge(share_router)
        .merge(workspaces_router)
        // SPA fallback — `/`, `/sign-in`, `/files/...`, hashed asset paths
        // — anything not matched above is served from the embedded `web/dist/`.
        .fallback(spa::serve)
        // Security headers (app-origin profile).
        .layer(SetResponseHeaderLayer::overriding(
            H_CSP,
            HeaderValue::from_static(APP_CSP),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            H_XCTO,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            H_REF,
            HeaderValue::from_static(REFERRER_POLICY),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            H_PP,
            HeaderValue::from_static(PERMISSIONS_POLICY),
        ))
        // Host-header dispatch (421 on wrong origin) — outermost so even
        // wrong-host requests get rejected before any other middleware fires.
        .layer(middleware::from_fn_with_state(
            state,
            |s: State<HttpState>, req, next| host_dispatch(s, Origin::App, req, next),
        ))
}

fn usercontent_router(state: HttpState) -> Router {
    // /healthz lives on the app origin only — probes hit the app side; this
    // origin's health is implied by /raw/{token} working. Avoids the
    // merge-time route-conflict panic from declaring /healthz twice.
    Router::new()
        .route("/raw/{token}", get(raw::raw_get))
        .with_state(state.clone())
        // User-content origin: sandbox CSP, nosniff. Cookies must never be
        // set on this origin — but we don't even mount session middleware,
        // so this is by construction.
        .layer(SetResponseHeaderLayer::overriding(
            H_CSP,
            HeaderValue::from_static(UCN_CSP),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            H_XCTO,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(middleware::from_fn_with_state(
            state,
            |s: State<HttpState>, req, next| host_dispatch(s, Origin::UserContent, req, next),
        ))
}
