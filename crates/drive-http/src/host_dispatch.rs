//! Host-header dispatch middleware. Returns 421 (Misdirected Request) when
//! a route is hit on the wrong origin. Defence-in-depth against reverse-proxy
//! misconfiguration.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::HttpState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Origin {
    App,
    UserContent,
}

pub(crate) async fn host_dispatch(
    State(state): State<HttpState>,
    expected: Origin,
    req: Request,
    next: Next,
) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let want = match expected {
        Origin::App => state.config.app_origin_host(),
        Origin::UserContent => state.config.usercontent_origin_host(),
    };
    if host != want.as_str() {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            format!("Wrong origin for this route (Host={host:?}, expected {want:?})"),
        )
            .into_response();
    }
    next.run(req).await
}
