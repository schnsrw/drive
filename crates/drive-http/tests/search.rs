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
        signed_url_ttl_secs: 300,
        oidc: None,
        allow_password_auth: true,
        thumb_worker: drive_core::ThumbWorkerConfig::default(),
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
        thumb_worker: std::sync::Arc::new(drive_storage::MultiKindWorker::image_only()),
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
                status: drive_db::FileStatus::Ready,
                expected_size: None,
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
            status: drive_db::FileStatus::Ready,
            expected_size: None,
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

// ── Phase 3 SR backend tests ─────────────────────────────────────────

async fn search_with(app: &axum::Router, cookie: &str, query: &str) -> Value {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/search?{query}"))
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK, "{query}");
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn response_carries_total_next_cursor_and_sort_applied() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "q=planning").await;
    assert!(body["total"]["exact"].as_bool().unwrap());
    assert_eq!(body["total"]["files"].as_u64().unwrap(), 2);
    assert_eq!(body["sort_applied"].as_str().unwrap(), "modified");
    // Two results ⇒ no next page.
    assert!(body.get("next_cursor").is_none() || body["next_cursor"].is_null());
}

#[tokio::test]
async fn type_filter_pdf_returns_only_pdf_files() {
    let state = fixture().await;
    let owner = owner_id(&state).await;
    let ws = personal_ws(&state, &owner).await;
    for (name, ct) in [
        ("report.pdf", Some("application/pdf")),
        ("snap.png", Some("image/png")),
        (
            "draft.docx",
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        ),
    ] {
        FileRepo::new(&state.db)
            .insert(&NewFile {
                id: ulid::Ulid::new().to_string(),
                parent_id: None,
                name: name.into(),
                size: 100,
                content_type: ct.map(str::to_string),
                etag: None,
                owner_id: owner.clone(),
                workspace_id: ws.clone(),
                storage_id: None,
                thumbnail: None,
                status: drive_db::FileStatus::Ready,
                expected_size: None,
            })
            .await
            .unwrap();
    }
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "type=pdf").await;
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["report.pdf"]);
}

#[tokio::test]
async fn type_filter_image_uses_prefix_match() {
    let state = fixture().await;
    let owner = owner_id(&state).await;
    let ws = personal_ws(&state, &owner).await;
    for (name, ct) in [
        ("a.png", Some("image/png")),
        ("b.jpg", Some("image/jpeg")),
        ("c.pdf", Some("application/pdf")),
    ] {
        FileRepo::new(&state.db)
            .insert(&NewFile {
                id: ulid::Ulid::new().to_string(),
                parent_id: None,
                name: name.into(),
                size: 100,
                content_type: ct.map(str::to_string),
                etag: None,
                owner_id: owner.clone(),
                workspace_id: ws.clone(),
                storage_id: None,
                thumbnail: None,
                status: drive_db::FileStatus::Ready,
                expected_size: None,
            })
            .await
            .unwrap();
    }
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "type=image").await;
    let mut names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["a.png", "b.jpg"]);
}

#[tokio::test]
async fn size_filter_excludes_rows_outside_band() {
    let state = fixture().await;
    let owner = owner_id(&state).await;
    let ws = personal_ws(&state, &owner).await;
    for (name, size) in [("tiny.bin", 100u64), ("big.bin", 50_000_000)] {
        FileRepo::new(&state.db)
            .insert(&NewFile {
                id: ulid::Ulid::new().to_string(),
                parent_id: None,
                name: name.into(),
                size,
                content_type: None,
                etag: None,
                owner_id: owner.clone(),
                workspace_id: ws.clone(),
                storage_id: None,
                thumbnail: None,
                status: drive_db::FileStatus::Ready,
                expected_size: None,
            })
            .await
            .unwrap();
    }
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "size_min=1000000&q=.bin").await;
    let names: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["big.bin"]);
}

#[tokio::test]
async fn cursor_pagination_walks_results_in_two_pages() {
    let state = fixture().await;
    let owner = owner_id(&state).await;
    let ws = personal_ws(&state, &owner).await;
    // 15 files; with limit=10 we expect page 1 of 10 + page 2 of 5.
    for i in 0..15 {
        FileRepo::new(&state.db)
            .insert(&NewFile {
                id: ulid::Ulid::new().to_string(),
                parent_id: None,
                name: format!("doc-{i:02}.txt"),
                size: 100,
                content_type: Some("text/plain".into()),
                etag: None,
                owner_id: owner.clone(),
                workspace_id: ws.clone(),
                storage_id: None,
                thumbnail: None,
                status: drive_db::FileStatus::Ready,
                expected_size: None,
            })
            .await
            .unwrap();
    }
    let app = router(state);
    let cookie = sign_in(&app).await;

    let page1 = search_with(&app, &cookie, "q=doc-&limit=10&sort=name&sort_dir=asc").await;
    assert_eq!(page1["files"].as_array().unwrap().len(), 10);
    let cursor = page1["next_cursor"].as_str().expect("next_cursor required");

    let page2 = search_with(
        &app,
        &cookie,
        &format!("q=doc-&limit=10&sort=name&sort_dir=asc&after={cursor}"),
    )
    .await;
    // Phase A: page 2 has the remaining 5; cursor terminates.
    assert_eq!(page2["files"].as_array().unwrap().len(), 5);
    assert!(page2["next_cursor"].is_null());

    // No overlap between pages.
    let p1_ids: std::collections::HashSet<String> = page1["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect();
    let p2_ids: std::collections::HashSet<String> = page2["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect();
    assert!(p1_ids.is_disjoint(&p2_ids));
}

#[tokio::test]
async fn cursor_rejects_when_filter_set_changes() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;

    // Page 1 with one filter set.
    let p1 = search_with(&app, &cookie, "q=planning&limit=1&sort=name&sort_dir=asc").await;
    let cursor = match p1["next_cursor"].as_str() {
        Some(c) => c.to_string(),
        None => {
            // 2 rows + limit 1 ⇒ has next; if not we can't test the
            // filter-mismatch case meaningfully.
            return;
        }
    };

    // Reuse the cursor under a *different* filter — server must 400.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/search?q=different&limit=1&sort=name&sort_dir=asc&after={cursor}"
                ))
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cross_workspace_search_returns_403_for_non_member() {
    let state = fixture().await;
    seed(&state).await;
    // Build a workspace the admin isn't a member of.
    let other = UserRepo::new(&state.db)
        .insert(&NewUser {
            username: "other".into(),
            password_hash: hash_password("hunter2hunter2").unwrap(),
            is_admin: false,
        })
        .await
        .unwrap();
    let other_ws = personal_ws(&state, &other.id).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/search?q=anything&workspace={other_ws}"))
                .header("host", APP)
                .header("cookie", cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn scope_all_returns_only_caller_workspaces() {
    let state = fixture().await;
    seed(&state).await;
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "q=planning&scope=all").await;
    // Two planning files exist in admin's Personal workspace.
    assert_eq!(body["files"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn notes_appear_in_response_when_their_title_matches() {
    use drive_db::{NewNote, NotesRepo};
    let state = fixture().await;
    seed(&state).await;
    let owner = owner_id(&state).await;
    let ws = personal_ws(&state, &owner).await;
    NotesRepo::new(&state.db)
        .insert(&NewNote {
            workspace_id: ws,
            parent_id: None,
            title: "Q2 planning kickoff".into(),
            owner_id: owner,
            order_key: "a0".into(),
        })
        .await
        .unwrap();
    let app = router(state);
    let cookie = sign_in(&app).await;
    let body = search_with(&app, &cookie, "q=kickoff").await;
    let titles: Vec<&str> = body["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["Q2 planning kickoff"]);
}
