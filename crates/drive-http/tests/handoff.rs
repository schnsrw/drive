//! Integration tests for the editor handoff endpoint
//! `GET /api/files/{id}/open`. Spec: docs/ux/08-editor-handoff.md.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{Db, FileRepo, NewFile, NewUser, UserRepo, WorkspaceKind, WorkspaceRepo};
use drive_http::{router, HttpState};
use drive_storage::Storage;
use drive_wopi::WopiState;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use url::Url;

const APP: &str = "drive.test";
const UCN: &str = "usercontent-drive.test";

async fn fixture(with_editors: bool) -> HttpState {
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
        sheet_origin: with_editors.then(|| Url::parse("http://sheet.test").unwrap()),
        document_origin: with_editors.then(|| Url::parse("http://document.test").unwrap()),
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

async fn owner_id(state: &HttpState) -> String {
    UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap()
        .id
}

async fn seed_file(state: &HttpState, name: &str, content_type: &str) -> String {
    let owner = owner_id(state).await;
    let ws = WorkspaceRepo::new(&state.db)
        .list_for_user(&owner)
        .await
        .unwrap()
        .into_iter()
        .find(|w| matches!(w.kind, WorkspaceKind::Personal))
        .expect("seeded user must have a Personal workspace")
        .id;
    let id = ulid::Ulid::new().to_string();
    let f = FileRepo::new(&state.db)
        .insert(&NewFile {
            id,
            parent_id: None,
            name: name.into(),
            size: 1024,
            content_type: Some(content_type.into()),
            etag: None,
            owner_id: owner,
            workspace_id: ws,
            storage_id: None,
            thumbnail: None,
        })
        .await
        .unwrap();
    f.id
}

async fn open(app: &axum::Router, cookie: &str, file_id: &str) -> axum::http::Response<Body> {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{file_id}/open"))
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn open_xlsx_mints_token_and_assembles_entry_url() {
    let state = fixture(true).await;
    let fid = seed_file(
        &state,
        "Q2 planning.xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    )
    .await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = open(&app, &cookie, &fid).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();

    assert_eq!(body["editor_app"], "sheet");
    let token = body["access_token"].as_str().unwrap();
    assert!(token.len() > 20, "JWT should be substantial");
    assert_eq!(body["access_token_ttl"], 600_000);

    let entry = body["entry_url"].as_str().unwrap();
    let wopi_src = body["wopi_src"].as_str().unwrap();
    assert!(entry.starts_with("http://sheet.test/wopi/editor?"));
    assert!(entry.contains("access_token="));
    // The WOPISrc query param is URL-encoded inside entry_url. Pick a
    // marker that survives encoding (the file_id is alphanumeric) instead
    // of pulling a urlencoding crate in just for this assertion.
    assert!(entry.contains("WOPISrc="));
    assert!(entry.contains(&fid));
    assert!(wopi_src.contains(&format!("/wopi/files/{fid}")));
}

#[tokio::test]
async fn open_docx_routes_to_document_origin() {
    let state = fixture(true).await;
    let fid = seed_file(
        &state,
        "Product brief.docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    )
    .await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = open(&app, &cookie, &fid).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["editor_app"], "document");
    assert!(body["entry_url"]
        .as_str()
        .unwrap()
        .starts_with("http://document.test/wopi/editor?"));
}

#[tokio::test]
async fn open_unsupported_type_is_415() {
    let state = fixture(true).await;
    let fid = seed_file(&state, "Architecture.pdf", "application/pdf").await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = open(&app, &cookie, &fid).await;
    assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn open_returns_503_when_editor_unconfigured() {
    let state = fixture(false).await; // no sheet_origin / document_origin
    let fid = seed_file(
        &state,
        "Q2 planning.xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    )
    .await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = open(&app, &cookie, &fid).await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["error"], "editor not configured");
    assert_eq!(body["extension"], "sheet");
}

#[tokio::test]
async fn open_requires_auth() {
    let state = fixture(true).await;
    let fid = seed_file(
        &state,
        "Q2 planning.xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    )
    .await;
    let app = router(state);

    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{fid}/open"))
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn open_token_passes_wopi_verify() {
    // The token Drive mints must validate against the same secret + the
    // same file_id when the editor sends it back via /wopi/files/{id}.
    let state = fixture(true).await;
    let fid = seed_file(
        &state,
        "Q2 planning.xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    )
    .await;
    let jwt_secret = state.jwt_secret.clone();
    let app = router(state);
    let cookie = sign_in(&app).await;

    let r = open(&app, &cookie, &fid).await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let token = body["access_token"].as_str().unwrap();

    use std::str::FromStr;
    let parsed_id = drive_core::FileId::from_str(&fid).unwrap();
    let claims = drive_wopi::verify_token(&jwt_secret, token, parsed_id).unwrap();
    assert_eq!(claims.file_id, parsed_id);
    assert_eq!(claims.perms, drive_wopi::WopiPerms::Write);
}
