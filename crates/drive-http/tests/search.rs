//! Integration tests for the global recursive search endpoint
//! `GET /api/search?q=`. Spec: docs/ux/12-search-surface.md.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use drive_auth::{hash_password, AuthState};
use drive_core::{Backend, Config};
use drive_db::{
    Db, FileRepo, FolderRepo, NewFile, NewFolder, NewUser, UserRepo, WorkspaceKind, WorkspaceRepo,
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

async fn owner_id(state: &HttpState) -> String {
    UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap()
        .id
}

async fn personal_ws(state: &HttpState, user_id: &str) -> String {
    WorkspaceRepo::new(&state.db)
        .list_for_user(user_id)
        .await
        .unwrap()
        .into_iter()
        .find(|w| matches!(w.kind, WorkspaceKind::Personal))
        .expect("seeded user must have a Personal workspace")
        .id
}

async fn seed(state: &HttpState) {
    let owner = owner_id(state).await;
    let ws = personal_ws(state, &owner).await;
    FolderRepo::new(&state.db)
        .insert(&NewFolder {
            parent_id: None,
            name: "Projects".into(),
            owner_id: owner.clone(),
            workspace_id: ws.clone(),
        })
        .await
        .unwrap();
    for name in [
        "Q2 planning.xlsx",
        "Q3 planning.xlsx",
        "Product brief.docx",
        "Logo.svg",
    ] {
        FileRepo::new(&state.db)
            .insert(&NewFile {
                id: ulid::Ulid::new().to_string(),
                parent_id: None,
                name: name.into(),
                size: 100,
                content_type: None,
                etag: None,
                owner_id: owner.clone(),
                workspace_id: ws.clone(),
                storage_id: None,
                thumbnail: None,
            })
            .await
            .unwrap();
    }
}

async fn search(app: &axum::Router, cookie: &str, q: &str) -> Value {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/search?q={q}"))
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn search_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/search?q=anything")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn empty_query_returns_empty_arrays() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search(&app, &cookie, "").await;
    assert_eq!(body["files"].as_array().unwrap().len(), 0);
    assert_eq!(body["folders"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn substring_match_is_case_insensitive() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    // "Q2" matches "Q2 planning.xlsx"
    let body = search(&app, &cookie, "Q2").await;
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Q2 planning.xlsx"]);

    // Lowercase still matches.
    let body = search(&app, &cookie, "planning").await;
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Q2 planning.xlsx"));
    assert!(names.contains(&"Q3 planning.xlsx"));
}

#[tokio::test]
async fn search_returns_matching_folders_too() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search(&app, &cookie, "project").await;
    let folder_names: Vec<&str> = body["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(folder_names, vec!["Projects"]);
}

#[tokio::test]
async fn search_excludes_other_users_files() {
    let state = fixture().await;
    seed(&state).await;
    // Insert a file owned by a different user — same name pattern.
    let other = UserRepo::new(&state.db)
        .insert(&NewUser {
            username: "other".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: false,
        })
        .await
        .unwrap();
    let other_ws = personal_ws(&state, &other.id).await;
    FileRepo::new(&state.db)
        .insert(&NewFile {
            id: ulid::Ulid::new().to_string(),
            parent_id: None,
            name: "Q2 secrets.xlsx".into(),
            size: 100,
            content_type: None,
            etag: None,
            owner_id: other.id,
            workspace_id: other_ws,
            storage_id: None,
            thumbnail: None,
        })
        .await
        .unwrap();
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search(&app, &cookie, "Q2").await;
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    // Admin only sees their own.
    assert_eq!(names, vec!["Q2 planning.xlsx"]);
}

#[tokio::test]
async fn search_excludes_trashed_files() {
    let state = fixture().await;
    seed(&state).await;
    // Trash one of the matching files.
    let oid = owner_id(&state).await;
    let ws = personal_ws(&state, &oid).await;
    let f = FileRepo::new(&state.db)
        .search(&ws, "Q3", 10)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    FileRepo::new(&state.db).trash(&f.id).await.unwrap();
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search(&app, &cookie, "Q3").await;
    assert!(body["files"].as_array().unwrap().is_empty());
}
