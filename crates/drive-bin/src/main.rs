//! `drive` — the Casual Drive binary entry point.

#![forbid(unsafe_code)]

use std::sync::Arc;

use drive_auth::AuthState;
use drive_core::Config;
use drive_db::{Db, DbError, NewUser, UserRepo};
use drive_http::{router, HttpState};
use drive_storage::{parse_master_key_hex, Storage};
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
        db_url_scheme = %cfg.db_url.split(':').next().unwrap_or("?"),
        is_prod = cfg.is_prod,
        "starting Casual Drive",
    );

    let db = Db::connect(&cfg.db_url).await?;
    tracing::info!(backend = ?db.backend(), "metadata db connected, migrations applied");

    seed_admin_if_missing(&db, &cfg).await?;

    let storage = Storage::from_config(&cfg)?;

    let cookie_secure = cfg.app_origin.scheme() == "https";
    let auth = AuthState::new(db.clone(), cookie_secure, time::Duration::hours(24));

    // BYO storage master key. Optional in v0 — workspaces with BYO can't be
    // configured without it, but the rest of the app runs fine. The /api
    // handler that saves a BYO config refuses with 503 if the key is absent.
    let storage_secret_key = match std::env::var("DRIVE_STORAGE_SECRET_KEY") {
        Ok(hex) => {
            Some(Arc::new(parse_master_key_hex(&hex).map_err(|e| {
                anyhow::anyhow!("DRIVE_STORAGE_SECRET_KEY: {e}")
            })?))
        }
        Err(_) => {
            if cfg.is_prod {
                tracing::warn!(
                    "DRIVE_STORAGE_SECRET_KEY not set — bring-your-own storage \
                     will be rejected. Generate with `openssl rand -hex 32`."
                );
            }
            None
        }
    };

    let registry = HttpState::default_registry(storage.clone(), cfg.signed_url_hmac_secret);

    let state = HttpState {
        storage,
        wopi: WopiState::new(),
        db,
        auth,
        jwt_secret: Arc::new(cfg.wopi_hmac_secret),
        config: Arc::new(cfg),
        upload_limiter: HttpState::default_upload_limiter(),
        registry,
        storage_secret_key,
    };

    let app = router(state).layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(addr = %bind, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Seed the admin user from env if no row matches the configured username.
/// The env carries an already-hashed Argon2id PHC string — we don't accept
/// raw passwords here.
async fn seed_admin_if_missing(db: &Db, cfg: &Config) -> Result<(), DbError> {
    let users = UserRepo::new(db);
    match users.find_by_username(&cfg.admin_user).await {
        Ok(_) => {
            tracing::info!(username = %cfg.admin_user, "admin user present in DB, skipping seed");
            Ok(())
        }
        Err(DbError::NotFound) => {
            users
                .insert(&NewUser {
                    username: cfg.admin_user.clone(),
                    password_hash: cfg.admin_password_hash.clone(),
                    is_admin: true,
                })
                .await?;
            tracing::info!(username = %cfg.admin_user, "admin user seeded from env");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,drive=debug".into()))
        .with(fmt::layer())
        .init();
}
