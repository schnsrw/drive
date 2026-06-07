//! Integration tests for the bring-your-own storage endpoints.
//! Pipeline §8.9. Spec: docs/research/08-byo-storage.md.
//!
//! We don't test against a real S3 — we test the surrounding contract:
//! permission gates (Member 403, Personal 409), the key-missing 503, the
//! SSRF guard, and the GET shape after a (fake) configure path.
//!
//! The actual round-trip put/stat/delete is covered in
//! drive-storage::byo::test_connection unit tests.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{
    Db, NewUser, NewWorkspaceStorage, UserRepo, WorkspaceKind, WorkspaceRepo,
    WorkspaceStorageProvider, WorkspaceStorageRepo,
};
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use url::Url;

const APP: &str = "drive.test";
const UCN: &str = "usercontent-drive.test";

async fn fixture(with_master_key: bool) -> HttpState {
    let storage = Storage::memory([1u8; 32]).unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "owner".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: true,
        })
        .await
        .unwrap();
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "member".into(),
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
        session_secret: vec![0u8; 32],
        wopi_hmac_secret: [2u8; 32],
        signed_url_hmac_secret: [1u8; 32],
        admin_user: "owner".into(),
        admin_password_hash: "$argon2id$test".into(),
        recipient_footer: true,
        is_prod: false,
        sheet_origin: None,
        document_origin: None,
    };
    let auth = AuthState::new(db.clone(), false, time::Duration::hours(1));
    let registry = HttpState::default_registry(storage.clone(), [0u8; 32]);
    let storage_secret_key = if with_master_key {
        Some(Arc::new([9u8; 32]))
    } else {
        None
    };
    HttpState {
        storage,
        wopi: WopiState::new(),
        db,
        auth,
        jwt_secret: Arc::new([2u8; 32]),
        config: Arc::new(cfg),
        upload_limiter: HttpState::default_upload_limiter(),
        registry,
        storage_secret_key,
    }
}

async fn sign_in(app: &axum::Router, username: &str) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"username":"{username}","password":"hunter2hunter2"}}"#
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

async fn team_workspace_owned_by(state: &HttpState, username: &str) -> String {
    let user = UserRepo::new(&state.db)
        .find_by_username(username)
        .await
        .unwrap();
    WorkspaceRepo::new(&state.db)
        .insert("Engineering", WorkspaceKind::Team, &user.id)
        .await
        .unwrap()
        .id
}

async fn personal_workspace_of(state: &HttpState, username: &str) -> String {
    let user = UserRepo::new(&state.db)
        .find_by_username(username)
        .await
        .unwrap();
    WorkspaceRepo::new(&state.db)
        .list_for_user(&user.id)
        .await
        .unwrap()
        .into_iter()
        .find(|w| matches!(w.kind, WorkspaceKind::Personal))
        .unwrap()
        .id
}

async fn json_get(app: &axum::Router, cookie: &str, uri: &str) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
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

async fn json_send(
    app: &axum::Router,
    cookie: &str,
    method: &str,
    uri: &str,
    body: &str,
) -> StatusCode {
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
    r.status()
}

#[tokio::test]
async fn get_default_returns_default_status() {
    let state = fixture(true).await;
    let team = team_workspace_owned_by(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let (status, body) = json_get(&app, &cookie, &format!("/api/workspaces/{team}/storage")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "default");
}

#[tokio::test]
async fn personal_workspace_is_409() {
    let state = fixture(true).await;
    let personal = personal_workspace_of(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let status = json_send(
        &app,
        &cookie,
        "PUT",
        &format!("/api/workspaces/{personal}/storage"),
        r#"{"provider":"s3","bucket":"b","region":"us-east-1","access_key_id":"a","secret_access_key":"s"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn non_owner_member_gets_403() {
    let state = fixture(true).await;
    // Owner = "owner"; we sign in as "member" and try to operate on
    // owner's Team workspace.
    let team = team_workspace_owned_by(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "member").await;

    // Member isn't even in the workspace, but the 403 path triggers on
    // the role check regardless — `find_by_id` returns the row, then the
    // owner check fails.
    let (status, _) = json_get(&app, &cookie, &format!("/api/workspaces/{team}/storage")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn missing_master_key_returns_503_on_put() {
    let state = fixture(false).await; // no master key
    let team = team_workspace_owned_by(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let status = json_send(
        &app,
        &cookie,
        "PUT",
        &format!("/api/workspaces/{team}/storage"),
        // SSRF-allowable but won't connect; the key-missing check fires
        // before either is reached so we still get 503.
        r#"{"provider":"s3","bucket":"b","region":"us-east-1","access_key_id":"AKIATEST","secret_access_key":"SECRET"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn ssrf_blocks_metadata_endpoint() {
    let state = fixture(true).await;
    let team = team_workspace_owned_by(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let status = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/workspaces/{team}/storage/test"),
        r#"{"provider":"minio","bucket":"b","region":"us-east-1","endpoint":"http://169.254.169.254/","access_key_id":"a","secret_access_key":"s"}"#,
    )
    .await;
    // SSRF rejection maps to 400 (Validation), not the TestFailed 422.
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_endpoint_for_minio_is_400() {
    let state = fixture(true).await;
    let team = team_workspace_owned_by(&state, "owner").await;
    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let status = json_send(
        &app,
        &cookie,
        "POST",
        &format!("/api/workspaces/{team}/storage/test"),
        // MinIO without endpoint → validate_shape error → 400.
        r#"{"provider":"minio","bucket":"b","region":"us-east-1","access_key_id":"a","secret_access_key":"s"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_after_synthetic_byo_row_returns_masked_shape() {
    // We insert a row directly via the repo (skipping the live test-connection
    // round-trip) to verify the GET handler returns the right shape — masked
    // secret, key_version, provider echo.
    let state = fixture(true).await;
    let team = team_workspace_owned_by(&state, "owner").await;
    WorkspaceStorageRepo::new(&state.db)
        .upsert(&NewWorkspaceStorage {
            workspace_id: team.clone(),
            provider: WorkspaceStorageProvider::S3,
            bucket: "my-team-bucket".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIAEXAMPLE123456789".into(),
            secret_ct: "placeholder-ct".into(),
        })
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "owner").await;
    let (status, body) = json_get(&app, &cookie, &format!("/api/workspaces/{team}/storage")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "byo");
    assert_eq!(body["provider"], "s3");
    assert_eq!(body["bucket"], "my-team-bucket");
    assert_eq!(body["region"], "us-east-1");
    assert_eq!(body["secret_masked"], "••••••••••");
    // Access key shows first 4 + tail 4, masked middle.
    assert_eq!(body["access_key_id_masked"], "AKIA…6789");
    assert_eq!(body["key_version"], 1);
}

#[tokio::test]
async fn delete_returns_204_and_subsequent_get_is_default() {
    let state = fixture(true).await;
    let team = team_workspace_owned_by(&state, "owner").await;
    WorkspaceStorageRepo::new(&state.db)
        .upsert(&NewWorkspaceStorage {
            workspace_id: team.clone(),
            provider: WorkspaceStorageProvider::S3,
            bucket: "b".into(),
            region: "us-east-1".into(),
            endpoint: None,
            access_key_id: "AKIA".into(),
            secret_ct: "placeholder".into(),
        })
        .await
        .unwrap();

    let app = router(state);
    let cookie = sign_in(&app, "owner").await;

    let status = json_send(
        &app,
        &cookie,
        "DELETE",
        &format!("/api/workspaces/{team}/storage"),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status2, body) = json_get(&app, &cookie, &format!("/api/workspaces/{team}/storage")).await;
    assert_eq!(status2, StatusCode::OK);
    assert_eq!(body["kind"], "default");
}
