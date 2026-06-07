//! End-to-end: two-origin assembly + the /raw/{token} handler + the merged
//! /wopi router. Carries over the spike-04 surface with real `HttpState`.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bytes::Bytes;
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{Db, NewUser, UserRepo};
use drive_http::{router, HttpState};
use drive_storage::{SignedUrl, Storage};
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use tower::ServiceExt;
use url::Url;

const APP: &str = "drive.test";
const UCN: &str = "usercontent-drive.test";

async fn fixture() -> HttpState {
    let storage = Storage::memory([1u8; 32]).unwrap();
    storage
        .put("foo/bar.txt", Bytes::from_static(b"hello two-origin"), None)
        .await
        .unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    // Seed the admin user so sign-in tests have something to log in as.
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: hash_password("hunter2").unwrap(),
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

#[tokio::test]
async fn healthz_on_app_origin() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn healthz_on_usercontent_returns_421() {
    // /healthz lives on the app router only. Hitting it with the UCN host
    // matches the app-router path but trips the host-dispatch middleware →
    // 421. The "wrong origin" reading is more informative than 404, and
    // matches the defence-in-depth posture.
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header("host", UCN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn api_on_usercontent_returns_421() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("host", UCN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn raw_on_app_returns_421() {
    let st = fixture().await;
    let SignedUrl::Token { token, .. } = st
        .storage
        .signed_get("foo/bar.txt", Duration::from_secs(60))
        .await
        .unwrap()
    else {
        panic!()
    };
    let app = router(st);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/raw/{token}"))
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn raw_on_usercontent_returns_bytes_and_security_headers() {
    let st = fixture().await;
    let SignedUrl::Token { token, .. } = st
        .storage
        .signed_get("foo/bar.txt", Duration::from_secs(60))
        .await
        .unwrap()
    else {
        panic!()
    };
    let app = router(st);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/raw/{token}"))
                .header("host", UCN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let csp = r
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("sandbox"));
    assert_eq!(
        r.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(
        r.headers().get("cross-origin-resource-policy").unwrap(),
        "same-site"
    );
    let cd = r
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cd.starts_with("attachment;"));
    let body = r.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"hello two-origin");
}

#[tokio::test]
async fn sign_in_sets_session_cookie_and_returns_csrf() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"hunter2"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap();
    // Dev mode (no HTTPS in test) drops the `__Host-` prefix.
    assert!(cookie.starts_with("cd_sid="), "got cookie {cookie}");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
    assert!(!cookie.contains("Secure"));
    let body = r.into_body().collect().await.unwrap().to_bytes();
    let body_s = std::str::from_utf8(&body).unwrap();
    assert!(body_s.contains("csrf_token"));
}

#[tokio::test]
async fn sign_in_with_wrong_password_returns_401() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"WRONG"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_with_unknown_username_returns_401() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"nobody","password":"hunter2"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn app_origin_carries_strict_csp() {
    let app = router(fixture().await);
    // Hit /healthz (unauthenticated) to verify the CSP middleware applies
    // to every response, not just authed ones. /api/me requires AuthSession
    // and would return 401 here.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let csp = r
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("frame-ancestors 'none'"));
    assert!(csp.contains("default-src 'self'"));
}
