//! Integration tests for the share-link endpoints — owner side
//! (POST/GET/DELETE) and recipient side (POST /api/share/{token}).

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{Db, FileRepo, NewFile, NewUser, UserRepo};
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
    };
    let auth = AuthState::new(db.clone(), false, time::Duration::hours(1));
    HttpState {
        storage,
        wopi: WopiState::new(),
        db,
        auth,
        jwt_secret: Arc::new([2u8; 32]),
        config: Arc::new(cfg),
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
    let set_cookie = r
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    set_cookie.split(';').next().unwrap().to_string()
}

async fn seed_file(state: &HttpState, owner_id: &str) -> String {
    let id = ulid::Ulid::new().to_string();
    let files = FileRepo::new(&state.db);
    let f = files
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "secret.pdf".into(),
            size: 1024,
            content_type: Some("application/pdf".into()),
            etag: Some("etag".into()),
            owner_id: owner_id.into(),
            thumbnail: None,
        })
        .await
        .unwrap();
    f.id
}

async fn owner_id(state: &HttpState) -> String {
    UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn create_share_returns_link_with_url() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/files/{fid}/share"))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"permissions":"view"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["permissions"], "view");
    assert_eq!(body["has_password"], false);
    let token = body["token"].as_str().unwrap();
    assert_eq!(token.len(), 22, "URL-safe base64 of 16 bytes is 22 chars");
    assert!(body["url"]
        .as_str()
        .unwrap()
        .contains(&format!("/s/{token}")));
}

#[tokio::test]
async fn create_share_rejects_non_view_permissions() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/files/{fid}/share"))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"permissions":"edit"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_share_requires_auth() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/files/{fid}/share"))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"permissions":"view"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn resolve_share_returns_file_metadata() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    // Mint a share.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/files/{fid}/share"))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"permissions":"view"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let mint: Value = serde_json::from_slice(&bytes).unwrap();
    let token = mint["token"].as_str().unwrap();

    // Public resolve — no auth.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/share/{token}"))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let resolved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resolved["file"]["name"], "secret.pdf");
    assert_eq!(resolved["file"]["content_type"], "application/pdf");
    assert_eq!(resolved["permissions"], "view");
    assert!(resolved["download_url"]
        .as_str()
        .unwrap()
        .starts_with("/api/share/"));
}

#[tokio::test]
async fn resolve_share_gates_on_password() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/files/{fid}/share"))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"permissions":"view","password":"open-sesame"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let mint: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(mint["has_password"], true);
    let token = mint["token"].as_str().unwrap().to_owned();

    // No password → 401 + WWW-Authenticate.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/share/{token}"))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        r.headers().get("www-authenticate").unwrap(),
        "x-share-password"
    );

    // Wrong password → 401 too.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/share/{token}"))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"nope"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // Right password → 200.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/share/{token}"))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"password":"open-sesame"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn resolve_share_returns_410_when_expired() {
    use drive_db::{NewShareLink, ShareLinkRepo};

    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    // Insert an already-expired share via the DB directly (the handler
    // normalises negative expiries to no-expiry, so we can't go through
    // the public API for this case).
    let now = time::OffsetDateTime::now_utc();
    let mint = ShareLinkRepo::new(&state.db)
        .insert(&NewShareLink {
            token: "expired-token-fixture-00".into(),
            file_id: Some(fid),
            folder_id: None,
            password_hash: None,
            permissions: "view".into(),
            expires_at: Some(now - time::Duration::hours(1)),
            created_by: oid,
        })
        .await
        .unwrap();
    let app = router(state);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/share/{}", mint.token))
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::GONE);
}

#[tokio::test]
async fn list_and_revoke_shares() {
    let state = fixture().await;
    let oid = owner_id(&state).await;
    let fid = seed_file(&state, &oid).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    // Two shares.
    for _ in 0..2 {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/files/{fid}/share"))
                    .header("host", APP)
                    .header("cookie", &cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"permissions":"view"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
    }

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{fid}/shares"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let list: Value = serde_json::from_slice(&bytes).unwrap();
    let shares = list["shares"].as_array().unwrap();
    assert_eq!(shares.len(), 2);

    let first_id = shares[0]["id"].as_str().unwrap();

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/shares/{first_id}"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{fid}/shares"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let list: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list["shares"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn resolve_share_404_for_unknown_token() {
    let state = fixture().await;
    let app = router(state);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/share/no-such-token-here-22ch")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}
