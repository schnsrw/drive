//! Integration tests for MU1 — workspace invitations.
//!
//! Covers the happy paths + the security gates each endpoint owes:
//! membership-required create / list / revoke, anonymous-safe peek,
//! signed-in accept, single-use exhaustion, expiry, revocation.

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
    // Pre-seed every test identity so individual tests don't need a
    // mid-flight UserRepo::insert (the Db handle is moved into the
    // router by `router(state)`, so we can't easily reach it later).
    for u in ["alice", "bob", "carol", "dan", "eve"] {
        UserRepo::new(&db)
            .insert(&NewUser {
                username: u.into(),
                password_hash: hash_password("hunter2hunter2").unwrap(),
                is_admin: u == "alice",
            })
            .await
            .unwrap();
    }
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
        signed_url_ttl_secs: 300,
        oidc: None,
        allow_password_auth: true,
        thumb_worker: drive_core::ThumbWorkerConfig::default(),
        session_secret: vec![0u8; 32],
        wopi_hmac_secret: [2u8; 32],
        signed_url_hmac_secret: [1u8; 32],
        admin_user: "alice".into(),
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
        thumb_worker: std::sync::Arc::new(drive_storage::MultiKindWorker::image_only()),
        presence: drive_http::presence::PresenceHub::new(),
    }
}

async fn sign_in(app: &axum::Router, username: &str) -> String {
    let body = format!(r#"{{"username":"{username}","password":"hunter2hunter2"}}"#);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(body))
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

async fn create_team_workspace(app: &axum::Router, cookie: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/workspaces")
                .header("host", APP)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"Engineering"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    body["id"].as_str().unwrap().to_string()
}

async fn create_invite(app: &axum::Router, cookie: &str, ws: &str, body: &str) -> Value {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/workspaces/{ws}/invitations"))
                .header("host", APP)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED, "create invite failed");
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn create_lists_then_revoke_round_trip() {
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &cookie).await;

    let created = create_invite(&app, &cookie, &ws, r#"{"role":"member"}"#).await;
    let inv_id = created["id"].as_str().unwrap().to_string();
    assert!(!created["token"].as_str().unwrap().is_empty());
    assert_eq!(created["role"], "member");
    assert_eq!(created["max_uses"], 1);

    // List should return exactly one row.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/workspaces/{ws}/invitations"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let list: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["revoked"], false);

    // Revoke.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/workspaces/{ws}/invitations/{inv_id}"))
                .header("host", APP)
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // Subsequent peek 404s because the row's now revoked.
    let token = created["token"].as_str().unwrap();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/invitations/{token}"))
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn peek_is_anonymous_safe_and_returns_workspace_info() {
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &cookie).await;

    let created = create_invite(&app, &cookie, &ws, r#"{"role":"member"}"#).await;
    let token = created["token"].as_str().unwrap();

    // No cookie — anonymous request must succeed.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/invitations/{token}"))
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["workspace_name"], "Engineering");
    assert_eq!(body["inviter_username"], "alice");
    assert_eq!(body["role"], "member");
    assert_eq!(body["remaining_uses"], 1);
    // Token MUST NOT appear in the peek payload.
    assert!(body.get("token").is_none());
}

#[tokio::test]
async fn non_member_cannot_create_invitation() {
    let state = fixture().await;
    let app = router(state);
    // Alice owns the workspace.
    let alice_cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &alice_cookie).await;
    // Bob signs in but is NOT a member of `ws`.
    let bob_cookie = sign_in(&app, "bob").await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/workspaces/{ws}/invitations"))
                .header("host", APP)
                .header("cookie", &bob_cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"role":"member"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn signed_in_accept_adds_member() {
    let state = fixture().await;
    let app = router(state);
    let alice_cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &alice_cookie).await;
    let invite = create_invite(&app, &alice_cookie, &ws, r#"{"role":"member"}"#).await;
    let token = invite["token"].as_str().unwrap();

    let bob_cookie = sign_in(&app, "bob").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .header("cookie", &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["workspace_id"], ws);
    assert_eq!(body["already_member"], false);

    // Bob now sees the workspace in his list.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/workspaces")
                .header("host", APP)
                .header("cookie", &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let ws_list = body["workspaces"].as_array().unwrap();
    assert!(
        ws_list.iter().any(|w| w["id"] == ws.as_str()),
        "Bob should now belong to the team workspace"
    );
}

#[tokio::test]
async fn single_use_token_exhausts_after_one_accept() {
    let state = fixture().await;
    let app = router(state);
    let alice_cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &alice_cookie).await;
    let invite = create_invite(&app, &alice_cookie, &ws, r#"{"role":"member"}"#).await;
    let token = invite["token"].as_str().unwrap();

    let bob_cookie = sign_in(&app, "bob").await;
    // First accept OK.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .header("cookie", &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // Same user accepting again → 200 with already_member=true
    // (idempotent — saves a round trip when the link is reopened).
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .header("cookie", &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["already_member"], true);

    // A different user trying the now-exhausted token → 409.
    let carol_cookie = sign_in(&app, "carol").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .header("cookie", &carol_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn multi_use_invite_admits_multiple_acceptors() {
    let state = fixture().await;
    let app = router(state);
    let alice_cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &alice_cookie).await;
    let invite = create_invite(
        &app,
        &alice_cookie,
        &ws,
        r#"{"role":"member","max_uses":3}"#,
    )
    .await;
    let token = invite["token"].as_str().unwrap();

    // Bob accepts. Carol accepts. Dan accepts. Then a fourth user
    // (Eve) tries and gets 409 because slots are exhausted.
    for u in ["bob", "carol", "dan"] {
        let cookie = sign_in(&app, u).await;
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/invitations/{token}/accept"))
                    .header("host", APP)
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "user {u} should have a slot");
    }

    // Fourth user — Eve — should be denied.
    let eve_cookie = sign_in(&app, "eve").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .header("cookie", &eve_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn unknown_token_peek_returns_404() {
    let state = fixture().await;
    let app = router(state);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/invitations/not-a-real-token-just-bytes-AAA")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn anonymous_accept_mints_user_session_and_membership() {
    let state = fixture().await;
    let app = router(state);
    let alice_cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &alice_cookie).await;
    let invite = create_invite(&app, &alice_cookie, &ws, r#"{"role":"member"}"#).await;
    let token = invite["token"].as_str().unwrap();

    // No cookie — anonymous magic-link accept.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/invitations/{token}/accept"))
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // Response carries the workspace_id + the freshly-minted user.
    let set_cookie = r
        .headers()
        .get("set-cookie")
        .expect("magic-link accept should set a session cookie")
        .to_str()
        .unwrap()
        .to_string();
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["workspace_id"], ws);
    assert_eq!(body["already_member"], false);
    let created = body["created_user"]
        .as_object()
        .expect("created_user payload");
    assert!(!created["user_id"].as_str().unwrap().is_empty());
    let new_username = created["username"].as_str().unwrap();
    assert!(
        new_username.starts_with("user-"),
        "expected user-* username, got {new_username}"
    );

    // The Set-Cookie line carries a real session — using it on
    // /api/me should return the new user.
    let cookie = set_cookie.split(';').next().unwrap();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let me: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(me["admin"].as_str().unwrap(), new_username);
}

#[tokio::test]
async fn admin_role_invitations_are_rejected_until_mu2() {
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "alice").await;
    let ws = create_team_workspace(&app, &cookie).await;

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/workspaces/{ws}/invitations"))
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"role":"admin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}
