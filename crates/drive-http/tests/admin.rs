//! Integration tests for `GET /api/admin/system`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{Db, NewUser, UserRepo};
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use url::Url;

const APP: &str = "drive.test";
const UCN: &str = "usercontent-drive.test";

async fn fixture_with(admin: bool) -> HttpState {
    let storage = Storage::memory([1u8; 32]).unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "user".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: admin,
        })
        .await
        .unwrap();
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
        admin_user: "user".into(),
        admin_password_hash: "$argon2id$test".into(),
        recipient_footer: true,
        is_prod: false,
        sheet_origin: None,
        document_origin: None,
    };
    let auth = AuthState::new(db.clone(), false, time::Duration::hours(1));
    let registry = HttpState::default_registry(storage.clone(), [0u8; 32]);
    HttpState {
        storage,
        wopi: WopiState::new(),
        db,
        auth,
        jwt_secret: Arc::new([2u8; 32]),
        config: Arc::new(cfg),
        upload_limiter: HttpState::default_upload_limiter(),
        registry,
        storage_secret_key: None,
    }
}

async fn sign_in(app: &axum::Router) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"user","password":"hunter2hunter2"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    r.headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn system_requires_auth() {
    let app = router(fixture_with(true).await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/system")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn system_forbids_non_admin() {
    let state = fixture_with(false).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/system")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn system_returns_snapshot_for_admin() {
    let state = fixture_with(true).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    // The sign-in we just did is also the only audit event so far.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/system")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();

    assert_eq!(body["version"], "0.0.1");
    assert_eq!(body["license"], "Apache-2.0");
    assert_eq!(body["storage_backend"], "Memory");
    assert_eq!(body["db_backend"], "Sqlite");
    assert_eq!(body["healthy"], true);
    assert!(body["uptime_seconds"].as_u64().is_some());
    assert!(body["active_sessions"].as_i64().unwrap() >= 1);

    let signs = body["recent_sign_ins"].as_array().unwrap();
    assert!(!signs.is_empty());
    assert_eq!(signs[0]["actor_username"], "user");
    assert_eq!(signs[0]["ok"], true);
}

#[tokio::test]
async fn admin_can_create_user_with_quota() {
    let state = fixture_with(true).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"correct-horse-batt","quota_bytes":1048576}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["username"], "alice");
    assert_eq!(body["quota_bytes"], 1_048_576);
}

#[tokio::test]
async fn non_admin_cannot_create_user() {
    let state = fixture_with(false).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/users")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"username":"alice","password":"correct-horse-batt"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_can_set_user_quota_and_then_clear_it() {
    use drive_db::UserRepo;
    let state = fixture_with(true).await;
    let target = UserRepo::new(&state.db)
        .find_by_username("user")
        .await
        .unwrap();
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/admin/users/{}/quota", target.id))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"quota_bytes":5000}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/admin/users/{}/quota", target.id))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"quota_bytes":null}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn quota_upgrade_request_returns_204() {
    let state = fixture_with(false).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/me/quota/request")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"requested_bytes":10737418240,"reason":"need to upload our backups"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn system_records_failed_sign_in_in_recent() {
    let state = fixture_with(true).await;
    let app = router(state);

    // Wrong password.
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"user","password":"NOPE"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    let cookie = sign_in(&app).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/system")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let signs = body["recent_sign_ins"].as_array().unwrap();
    assert!(
        signs.iter().any(|s| s["ok"] == false),
        "should include the failed sign-in: {signs:?}"
    );
}
