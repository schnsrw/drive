//! Integration tests for the audit-log + /api/activity feed.

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

async fn fixture() -> HttpState {
    let storage = Storage::memory([1u8; 32]).unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: true,
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
        admin_user: "admin".into(),
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
                    r#"{"username":"admin","password":"hunter2hunter2"}"#,
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
async fn activity_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/activity")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_emits_audit_event() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    // The emit is fire-and-forget — give the spawned task a beat to land.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/activity?limit=10")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let events = v["events"].as_array().unwrap();
    assert!(!events.is_empty(), "expected at least one event");
    assert_eq!(events[0]["action"], "auth.sign_in");
    assert_eq!(events[0]["actor_username"], "admin");
    assert_eq!(events[0]["target_kind"], "session");
}

#[tokio::test]
async fn failed_sign_in_emits_no_actor_event() {
    let app = router(fixture().await);

    // Wrong password.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"NOPE"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // Sign in for real so we can read /api/activity.
    let cookie = sign_in(&app).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/activity?limit=20")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let actions: Vec<String> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect();
    assert!(actions.contains(&"auth.sign_in".to_string()));
    assert!(actions.contains(&"auth.sign_in_failed".to_string()));

    let failed = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["action"] == "auth.sign_in_failed")
        .unwrap();
    assert!(failed["actor_id"].is_null());
    assert_eq!(failed["target_name"], "admin");
}

#[tokio::test]
async fn pagination_uses_before_cursor() {
    use drive_db::{AuditRepo, NewAuditEvent};

    let state = fixture().await;
    // Seed 5 events directly so we don't depend on a specific handler's
    // ordering or count.
    for i in 0..5 {
        AuditRepo::new(&state.db)
            .insert(NewAuditEvent {
                actor_id: Some("u".into()),
                actor_username: Some("admin".into()),
                action: format!("test.event_{i}"),
                target_kind: Some("file".into()),
                target_id: Some(format!("id_{i}")),
                target_name: Some(format!("file_{i}")),
                ip_address: None,
                metadata: None,
            })
            .await
            .unwrap();
        // Tiny sleep so created_at strictly increases — sqlite RFC3339
        // has millisecond resolution.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    let app = router(state);
    let cookie = sign_in(&app).await;

    // First page: limit=3, newest first.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/activity?limit=3")
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["events"].as_array().unwrap().len(), 3);
    let next = v["next_before"].as_str().unwrap().to_string();

    // Second page using the cursor: rest of the events.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/activity?limit=10&before={next}"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    // 5 test.event_* plus the auth.sign_in from sign_in() = 6 total; first
    // page took 3, so the rest is 3.
    assert!(v["events"].as_array().unwrap().len() >= 3);
    assert!(
        v["next_before"].is_null(),
        "short page → next_before is null"
    );
}
