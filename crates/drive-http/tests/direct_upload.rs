//! Integration tests for direct-to-storage upload — pipeline §13.6.
//! Spec: docs/research/10-direct-upload.md.
//!
//! The fixture's storage adapter is `Memory`, which can't natively presign
//! a PUT (the Memory backend issues HMAC tokens via `SignedUrl::Token`).
//! That means the `presign` route returns 409 (`AdapterCannotPresign`),
//! which is exactly the contract the SPA falls back on. The complete +
//! abort paths get exercised by inserting a placeholder row directly
//! through the repo and driving the handlers against it.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{
    Db, FileRepo, FileStatus, NewFile, NewUser, UserRepo, WorkspaceKind, WorkspaceRepo,
};
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use serde_json::{json, Value};
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
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "outsider".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: false,
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
        signed_url_ttl_secs: 300,
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

async fn sign_in(app: &axum::Router, who: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"username":"{who}","password":"hunter2hunter2"}}"#
                )))
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

async fn personal_ws(state: &HttpState, user: &str) -> String {
    let u = UserRepo::new(&state.db)
        .find_by_username(user)
        .await
        .unwrap();
    WorkspaceRepo::new(&state.db)
        .list_for_user(&u.id)
        .await
        .unwrap()
        .into_iter()
        .find(|w| matches!(w.kind, WorkspaceKind::Personal))
        .unwrap()
        .id
}

async fn json_send(
    app: &axum::Router,
    cookie: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("host", APP)
                .header("cookie", cookie)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    let v: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, v)
}

#[tokio::test]
async fn presign_on_memory_backend_is_409() {
    // Memory adapter can't natively presign — the route returns 409 so
    // the SPA falls back to the proxy multipart path.
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "admin").await;

    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        "/api/files/upload-url",
        json!({"name": "big.mp4", "size": 100_000_000, "content_type": "video/mp4"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn presign_validation_400() {
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "admin").await;

    // Empty name.
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        "/api/files/upload-url",
        json!({"name": "", "size": 1024}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // size = 0.
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        "/api/files/upload-url",
        json!({"name": "ok.mp4", "size": 0}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn presign_blocked_extension_400() {
    let state = fixture().await;
    let app = router(state);
    let cookie = sign_in(&app, "admin").await;

    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        "/api/files/upload-url",
        json!({"name": "evil.exe", "size": 1024, "content_type": "application/octet-stream"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn complete_flips_uploading_to_ready() {
    // Insert a placeholder row in `uploading` (skipping presign because
    // memory backend can't issue a native URL), put bytes through the
    // shared storage handle, then call the complete handler.
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "big.mp4".into(),
            size: 0,
            content_type: Some("video/mp4".into()),
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Uploading,
            expected_size: Some(1024),
        })
        .await
        .unwrap();

    // Stuff bytes directly into storage via the shared registry handle.
    let storage = state.registry.default_storage();
    storage
        .put(
            &format!("files/{id}"),
            bytes::Bytes::from_static(b"x".repeat(1024).leak()),
            Some("video/mp4"),
        )
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "admin").await;

    let (st, body) = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/files/{id}/complete"),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["status"], "ready");
    assert_eq!(body["size"], 1024);
    assert_eq!(body["thumbs_state"], "pending");
}

#[tokio::test]
async fn complete_on_non_uploading_row_is_409() {
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "already.txt".into(),
            size: 10,
            content_type: Some("text/plain".into()),
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Ready,
            expected_size: None,
        })
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "admin").await;
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/files/{id}/complete"),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn outsider_cannot_complete_someone_elses_upload() {
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "private.mp4".into(),
            size: 0,
            content_type: Some("video/mp4".into()),
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Uploading,
            expected_size: Some(1024),
        })
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "outsider").await;
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/files/{id}/complete"),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn abort_deletes_uploading_row() {
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "cancel.mp4".into(),
            size: 0,
            content_type: Some("video/mp4".into()),
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Uploading,
            expected_size: Some(1024),
        })
        .await
        .unwrap();

    let app = router(state.clone());
    let cookie = sign_in(&app, "admin").await;
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/files/{id}/abort"),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    assert!(FileRepo::new(&state.db).find_by_id(&id).await.is_err());
}

#[tokio::test]
async fn abort_refuses_to_nuke_ready_rows() {
    // Abort must only act on `uploading` rows — otherwise it'd become a
    // backdoor for permanent deletion bypassing trash + audit.
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();

    let id = ulid::Ulid::new().to_string();
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: id.clone(),
            parent_id: None,
            name: "real-file.txt".into(),
            size: 100,
            content_type: Some("text/plain".into()),
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Ready,
            expected_size: None,
        })
        .await
        .unwrap();

    let app = router(state.clone());
    let cookie = sign_in(&app, "admin").await;
    let (st, _) = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/files/{id}/abort"),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Row still there.
    assert!(FileRepo::new(&state.db).find_by_id(&id).await.is_ok());
}

#[tokio::test]
async fn quota_counts_uploading_rows() {
    // Set a tight quota, drop an `uploading` row that fills most of it,
    // then verify a new presign request that overflows is refused with 413.
    let state = fixture().await;
    let ws = personal_ws(&state, "admin").await;
    let users = UserRepo::new(&state.db);
    let user = users.find_by_username("admin").await.unwrap();
    users.set_quota(&user.id, Some(10_000)).await.unwrap();

    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: ulid::Ulid::new().to_string(),
            parent_id: None,
            name: "huge.bin".into(),
            size: 0,
            content_type: None,
            etag: None,
            owner_id: user.id.clone(),
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
            status: FileStatus::Uploading,
            expected_size: Some(9_000),
        })
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "admin").await;
    let (st, body) = json_send(
        &app,
        &cookie,
        "POST",
        "/api/files/upload-url",
        json!({"name": "second.mp4", "size": 2_000, "content_type": "video/mp4"}),
    )
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["used_bytes"], 9_000);
    assert_eq!(body["quota_bytes"], 10_000);
}
