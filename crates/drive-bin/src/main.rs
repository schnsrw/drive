//! `drive` — the Casual Drive binary entry point.

#![forbid(unsafe_code)]

use std::sync::Arc;

use drive_core::Config;
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = Config::from_env()?;
    let bind = cfg.bind;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        app_origin = %cfg.app_origin,
        usercontent_origin = %cfg.usercontent_origin,
        backend = ?cfg.backend,
        is_prod = cfg.is_prod,
        "starting Casual Drive",
    );

    let storage = Storage::from_config(&cfg)?;
    let state = HttpState {
        storage,
        wopi: WopiState::new(),
        jwt_secret: Arc::new(cfg.wopi_hmac_secret),
        config: Arc::new(cfg),
    };

    let app = router(state).layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(addr = %bind, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,drive=debug".into()))
        .with(fmt::layer())
        .init();
}
