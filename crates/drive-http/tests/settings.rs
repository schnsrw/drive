//! Integration tests for the Settings-surface endpoints —
//! `POST /api/auth/change-password` and `GET /api/about`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{Db, NewUser, SessionRepo, UserRepo};
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

async fn sign_in(app: &axum::Router, password: &str) -> String {
    let body = format!(r#"{{"username":"admin","password":"{password}"}}"#);
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
    assert_eq!(r.status(), StatusCode::OK, "sign-in failed");
    let set_cookie = r
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    set_cookie.split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn change_password_happy_path_rotates_and_invalidates_other_sessions() {
    let app = router(fixture().await);

    // First sign-in — the session we'll *keep* after the password change.
    let cookie_alive = sign_in(&app, "hunter2hunter2").await;
    // Second sign-in — a separate device. Should be invalidated.
    let cookie_dead = sign_in(&app, "hunter2hunter2").await;
    assert_ne!(cookie_alive, cookie_dead);

    // Rotate the password using cookie_alive.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("host", APP)
                .header("cookie", &cookie_alive)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"old_password":"hunter2hunter2","new_password":"correct-horse-battery-staple"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // cookie_alive still works.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("host", APP)
                .header("cookie", &cookie_alive)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    // cookie_dead does not.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header("host", APP)
                .header("cookie", &cookie_dead)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // The new password lets us sign in again from scratch.
    let _ = sign_in(&app, "correct-horse-battery-staple").await;
}

#[tokio::test]
async fn change_password_rejects_wrong_old() {
    let app = router(fixture().await);
    let cookie = sign_in(&app, "hunter2hunter2").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"old_password":"WRONG","new_password":"correct-horse-battery-staple"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn change_password_rejects_short_new() {
    let app = router(fixture().await);
    let cookie = sign_in(&app, "hunter2hunter2").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"old_password":"hunter2hunter2","new_password":"short"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn change_password_rejects_same_as_old() {
    let app = router(fixture().await);
    let cookie = sign_in(&app, "hunter2hunter2").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("host", APP)
                .header("cookie", &cookie)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"old_password":"hunter2hunter2","new_password":"hunter2hunter2"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn change_password_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/change-password")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"old_password":"hunter2hunter2","new_password":"correct-horse-battery-staple"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn about_returns_build_metadata() {
    let app = router(fixture().await);
    let cookie = sign_in(&app, "hunter2hunter2").await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/about")
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
    assert_eq!(v["version"], "0.0.1");
    assert!(v["git_sha"].is_string());
    assert!(v["built_at"].is_string());
    assert_eq!(v["license"], "Apache-2.0");
    assert!(v["storage_backend"].is_string());
    assert!(v["db_backend"].is_string());
}

#[tokio::test]
async fn about_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/about")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

// The SessionRepo helper itself — exercised once independent of HTTP.
#[tokio::test]
async fn session_repo_delete_for_user_except_keeps_one() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "u".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: false,
        })
        .await
        .unwrap();
    let user = UserRepo::new(&db).find_by_username("u").await.unwrap();
    let sessions = SessionRepo::new(&db);
    let keep = sessions
        .insert(
            "keep",
            &drive_db::NewSession {
                user_id: user.id.clone(),
                csrf_token: "k".into(),
                ttl: time::Duration::hours(1),
            },
        )
        .await
        .unwrap();
    sessions
        .insert(
            "dead-a",
            &drive_db::NewSession {
                user_id: user.id.clone(),
                csrf_token: "a".into(),
                ttl: time::Duration::hours(1),
            },
        )
        .await
        .unwrap();
    sessions
        .insert(
            "dead-b",
            &drive_db::NewSession {
                user_id: user.id.clone(),
                csrf_token: "b".into(),
                ttl: time::Duration::hours(1),
            },
        )
        .await
        .unwrap();

    let killed = sessions
        .delete_for_user_except(&user.id, &keep.id)
        .await
        .unwrap();
    assert_eq!(killed, 2);
    assert!(sessions.get("keep").await.is_ok());
    assert!(sessions.get("dead-a").await.is_err());
    assert!(sessions.get("dead-b").await.is_err());
}
