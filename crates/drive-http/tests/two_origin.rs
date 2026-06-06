//! End-to-end: two-origin assembly + the /raw/{token} handler + the merged
//! /wopi router. Carries over the spike-04 surface with real `HttpState`.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bytes::Bytes;
use drive_core::{Backend, Config};
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
    };
    HttpState {
        storage,
        wopi: WopiState::new(),
        jwt_secret: Arc::new([2u8; 32]),
        config: Arc::new(cfg),
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
async fn app_origin_carries_strict_csp() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
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
