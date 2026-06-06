//! End-to-end WOPI edit cycle against the Phase-1 crate.
//! Carries over from `spike-02-wopi-host` using FileId IDs and a real Storage.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use bytes::Bytes;
use drive_core::FileId;
use drive_storage::Storage;
use drive_wopi::{mint_token, router, WopiAppState, WopiClaims, WopiPerms, WopiState};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn secret() -> Arc<[u8; 32]> {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(13);
    }
    Arc::new(k)
}

async fn fixture() -> (WopiAppState, FileId) {
    let sk = secret();
    let wopi = WopiState::new();
    let storage = Storage::memory([1u8; 32]).unwrap();
    let id = FileId::new();
    wopi.register(id, "Budget.xlsx".into()).await;
    storage
        .put(&format!("files/{id}"), Bytes::from_static(b"v1"), None)
        .await
        .unwrap();
    (
        WopiAppState {
            storage,
            wopi,
            jwt_secret: sk,
        },
        id,
    )
}

fn token_for(state: &WopiAppState, id: FileId, perms: WopiPerms) -> String {
    let exp = (time::OffsetDateTime::now_utc() + time::Duration::minutes(10)).unix_timestamp();
    mint_token(
        &state.jwt_secret,
        &WopiClaims {
            user_id: "user-1".into(),
            file_id: id,
            perms,
            exp,
            jti: "t".into(),
        },
    )
}

#[tokio::test]
async fn happy_path_full_edit_cycle() {
    let (state, id) = fixture().await;
    let token = token_for(&state, id, WopiPerms::Write);
    let app = router(state.clone());

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/wopi/files/{id}/contents?access_token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = r.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"v1");

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .header("x-wopi-override", "LOCK")
                .header("x-wopi-lock", "L1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}/contents?access_token={token}"))
                .header("x-wopi-override", "PUT")
                .header("x-wopi-lock", "L1")
                .body(Body::from("v2!"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers().get("x-wopi-itemversion").unwrap(), "2");

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .header("x-wopi-override", "UNLOCK")
                .header("x-wopi-lock", "L1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}

#[tokio::test]
async fn put_without_lock_returns_409_with_lock_header() {
    let (state, id) = fixture().await;
    let token = token_for(&state, id, WopiPerms::Write);
    let app = router(state);

    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .header("x-wopi-override", "LOCK")
                .header("x-wopi-lock", "other")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}/contents?access_token={token}"))
                .header("x-wopi-override", "PUT")
                .header("x-wopi-lock", "mine")
                .body(Body::from("nope"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(r.headers().get("x-wopi-lock").unwrap(), "other");
}

#[tokio::test]
async fn token_for_other_file_rejected() {
    let (state, id) = fixture().await;
    let other = FileId::new();
    state.wopi.register(other, "x".into()).await;
    let bad = token_for(&state, id, WopiPerms::Write);
    let app = router(state);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/wopi/files/{other}?access_token={bad}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unlock_and_relock_atomic_swap() {
    let (state, id) = fixture().await;
    let token = token_for(&state, id, WopiPerms::Write);
    let app = router(state);
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .header("x-wopi-override", "LOCK")
                .header("x-wopi-lock", "old-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/wopi/files/{id}?access_token={token}"))
                .header("x-wopi-override", "LOCK")
                .header("x-wopi-oldlock", "old-id")
                .header("x-wopi-lock", "new-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
