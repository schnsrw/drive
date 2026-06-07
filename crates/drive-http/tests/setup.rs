//! Integration tests for the first-run admin-setup wizard endpoints —
//! `GET /api/setup/status` and `POST /api/setup/admin`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::AuthState;
use drive_core::{Backend, Config};
use drive_db::{Db, UserRepo};
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use url::Url;

const APP: &str = "drive.test";
const UCN: &str = "usercontent-drive.test";

/// Fixture with an empty `users` table — the wizard precondition.
async fn bare_fixture() -> (HttpState, Db) {
    let storage = Storage::memory([1u8; 32]).unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    let cfg = Config {
        app_origin: Url::parse(&format!("http://{APP}")).unwrap(),
        usercontent_origin: Url::parse(&format!("http://{UCN}")).unwrap(),
        bind: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        backend: Backend::Memory,
        fs_root: None,
        s3_bucket: None,
        s3_region: None,
        s3_endpoint: None,
        aws_access_key_id: None,
        aws_secret_access_key: None,
        db_url: "sqlite::memory:".into(),
        body_limit_mb: 100,
        session_secret: vec![0u8; 32],
        wopi_hmac_secret: [2u8; 32],
        signed_url_hmac_secret: [1u8; 32],
        admin_user: "admin".into(),
        admin_password_hash: "$argon2id$test".into(),
        recipient_footer: true,
        is_prod: false,
        sheet_origin: None,
        document_origin: None,
    };
    let auth = AuthState::new(db.clone(), false, time::Duration::hours(1));
    let registry = HttpState::default_registry(storage.clone(), [0u8; 32]);
    let state = HttpState {
        storage,
        wopi: WopiState::new(),
        db: db.clone(),
        auth,
        jwt_secret: Arc::new([2u8; 32]),
        config: Arc::new(cfg),
        upload_limiter: HttpState::default_upload_limiter(),
        registry,
        storage_secret_key: None,
    };
    (state, db)
}

async fn get_status(app: &axum::Router) -> Value {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/setup/status")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post_setup(app: &axum::Router, body: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/setup/admin")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn fresh_install_needs_setup() {
    let (state, _db) = bare_fixture().await;
    let app = router(state);
    let v = get_status(&app).await;
    assert_eq!(v["needs_setup"], true);
}

#[tokio::test]
async fn setup_admin_creates_user_and_signs_in() {
    let (state, db) = bare_fixture().await;
    let app = router(state);

    // Pre-condition.
    assert_eq!(get_status(&app).await["needs_setup"], true);

    let r = post_setup(
        &app,
        r#"{"username":"owner","password":"correct-horse-battery-staple"}"#,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    // Cookie + CSRF returned.
    let set_cookie = r
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(set_cookie.contains("cd_sid=") || set_cookie.contains("__Host-cd_sid="));
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["csrf_token"].as_str().unwrap().len() > 8);

    // User now exists.
    let user = UserRepo::new(&db).find_by_username("owner").await.unwrap();
    assert!(user.is_admin);

    // Status flips permanently.
    assert_eq!(get_status(&app).await["needs_setup"], false);

    // The freshly-minted cookie authenticates against /api/me.
    let pair = set_cookie.split(';').next().unwrap();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("host", APP)
                .header("cookie", pair)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn second_setup_call_is_409() {
    let (state, _db) = bare_fixture().await;
    let app = router(state);

    let first = post_setup(
        &app,
        r#"{"username":"first","password":"correct-horse-battery-staple"}"#,
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);

    let second = post_setup(
        &app,
        r#"{"username":"second","password":"another-12-char-pw-here"}"#,
    )
    .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn setup_rejects_short_username() {
    let (state, _db) = bare_fixture().await;
    let app = router(state);
    let r = post_setup(
        &app,
        r#"{"username":"ab","password":"correct-horse-battery-staple"}"#,
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn setup_rejects_short_password() {
    let (state, _db) = bare_fixture().await;
    let app = router(state);
    let r = post_setup(&app, r#"{"username":"owner","password":"short"}"#).await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn status_endpoint_is_public_no_session_required() {
    let (state, _db) = bare_fixture().await;
    let app = router(state);
    // No cookie, no anything — still returns 200.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/setup/status")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
