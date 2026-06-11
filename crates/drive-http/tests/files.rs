//! Integration tests for the file + folder REST API. All endpoints require
//! a valid session cookie obtained via `/api/auth/sign-in`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bytes::Bytes;
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
    UserRepo::new(&db)
        .insert(&NewUser {
            username: "admin".into(),
            password_hash: hash_password("hunter2").unwrap(),
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
        presence: drive_http::presence::PresenceHub::new(),
    }
}

/// Sign in as admin, return the session cookie value (full `Cookie:` header).
async fn sign_in(app: &axum::Router) -> String {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/sign-in")
                .header("host", APP)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"admin","password":"hunter2"}"#))
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
    // Set-Cookie header looks like "cd_sid=...; Path=/; HttpOnly; ...".
    // Strip everything after the first `;` to get the cookie value.
    let pair = set_cookie.split(';').next().unwrap();
    pair.to_string()
}

fn auth_req(
    method: &str,
    path: &str,
    cookie: &str,
    content_type: Option<&str>,
    body: Body,
) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("host", APP)
        .header("cookie", cookie);
    if let Some(ct) = content_type {
        b = b.header("content-type", ct);
    }
    b.body(body).unwrap()
}

async fn json_body(r: axum::http::Response<Body>) -> Value {
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn list_root_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/folders/root/children")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_root_is_empty_for_fresh_admin() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            "/api/folders/root/children",
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body = json_body(r).await;
    assert_eq!(body["folders"].as_array().unwrap().len(), 0);
    assert_eq!(body["files"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn create_folder_then_list_root_shows_it() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/folders",
            &cookie,
            Some("application/json"),
            Body::from(r#"{"name":"Reports"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let created = json_body(r).await;
    assert_eq!(created["name"], "Reports");
    let id = created["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty());

    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            "/api/folders/root/children",
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    let listed = json_body(r).await;
    assert_eq!(listed["folders"].as_array().unwrap().len(), 1);
    assert_eq!(listed["folders"][0]["id"], id);
}

#[tokio::test]
async fn create_folder_rejects_empty_name() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/folders",
            &cookie,
            Some("application/json"),
            Body::from(r#"{"name":"   "}"#),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_file_then_list_root_shows_it() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    let boundary = "----testboundary";
    let body = build_multipart(
        boundary,
        &[
            MultipartField::Text("parent_id", ""),
            MultipartField::File("file", "hello.txt", "text/plain", b"hello world"),
        ],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let created = json_body(r).await;
    assert_eq!(created["name"], "hello.txt");
    assert_eq!(created["size"], 11);
    let id = created["id"].as_str().unwrap().to_string();

    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            "/api/folders/root/children",
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    let listed = json_body(r).await;
    assert_eq!(listed["files"].as_array().unwrap().len(), 1);
    assert_eq!(listed["files"][0]["id"], id);
    assert_eq!(listed["files"][0]["name"], "hello.txt");
}

#[tokio::test]
async fn get_file_meta_returns_the_dto_after_upload() {
    // `GET /api/files/{id}` — used by Drive's SPA when it lands on
    // `/file/<id>` cold (refresh / shared URL / bookmark) without an
    // in-memory FileDto from the file list.
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    let boundary = "----metaboundary";
    let body = build_multipart(
        boundary,
        &[
            MultipartField::Text("parent_id", ""),
            MultipartField::File("file", "meta.txt", "text/plain", b"meta-payload"),
        ],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    let created = json_body(r).await;
    let id = created["id"].as_str().unwrap().to_string();

    // Fetch the metadata by id.
    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/api/files/{id}"),
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let meta = json_body(r).await;
    assert_eq!(meta["id"], id);
    assert_eq!(meta["name"], "meta.txt");
    assert_eq!(meta["size"], 12);
    assert_eq!(meta["content_type"], "text/plain");
}

#[tokio::test]
async fn get_file_meta_404s_for_unknown_id() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            "/api/files/does-not-exist",
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_file_meta_requires_auth() {
    let app = router(fixture().await);
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/files/any-id")
                .header("host", APP)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn upload_rejects_forbidden_extension() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    for name in ["malware.exe", "install.sh", "auto.bat", "setup.tar.gz.cmd"] {
        let boundary = "----testboundary-blk";
        let body = build_multipart(
            boundary,
            &[MultipartField::File(
                "file",
                name,
                "application/octet-stream",
                b"junk",
            )],
        );
        let r = app
            .clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                &cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap();
        assert_eq!(
            r.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{name} should be 415"
        );
        let body = json_body(r).await;
        assert_eq!(body["error"], "file type not allowed");
        let ext = body["extension"].as_str().unwrap();
        assert!(["exe", "sh", "bat", "cmd"].contains(&ext));
    }
}

#[tokio::test]
async fn upload_still_accepts_macro_enabled_office_files() {
    // .docm / .xlsm / .pptm are intentionally allowed per CLAUDE.md —
    // accepted as opaque blobs, never auto-opened in the editor.
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    for name in ["doc.docm", "sheet.xlsm", "deck.pptm"] {
        let boundary = "----testboundary-macro";
        let body = build_multipart(
            boundary,
            &[MultipartField::File(
                "file",
                name,
                "application/octet-stream",
                b"blob",
            )],
        );
        let r = app
            .clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                &cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{name} should upload OK");
    }
}

#[tokio::test]
async fn upload_rejects_when_over_quota() {
    use drive_db::UserRepo;
    let state = fixture().await;
    let user = UserRepo::new(&state.db)
        .find_by_username("admin")
        .await
        .unwrap();
    UserRepo::new(&state.db)
        .set_quota(&user.id, Some(100))
        .await
        .unwrap();
    let app = router(state);
    let cookie = sign_in(&app).await;

    let boundary = "----testboundary-quota";
    let payload = vec![b'x'; 200];
    let body = build_multipart(
        boundary,
        &[MultipartField::File(
            "file",
            "big.txt",
            "text/plain",
            &payload,
        )],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = json_body(r).await;
    assert_eq!(body["error"], "quota exceeded");
    assert_eq!(body["quota"], 100);
}

#[tokio::test]
async fn upload_throttles_burst_with_429_and_retry_after() {
    use drive_http::{RateLimitConfig, RateLimiter};
    use std::sync::Arc;
    let mut state = fixture().await;
    state.upload_limiter = Arc::new(RateLimiter::new(RateLimitConfig {
        capacity: 2.0,
        refill_per_sec: 0.01,
    }));
    let app = router(state);
    let cookie = sign_in(&app).await;

    async fn upload(app: &axum::Router, cookie: &str, idx: usize) -> axum::http::Response<Body> {
        let boundary = format!("----rate{idx}");
        let body = build_multipart(
            &boundary,
            &[MultipartField::File("file", "a.txt", "text/plain", b"hi")],
        );
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap()
    }

    assert_eq!(upload(&app, &cookie, 0).await.status(), StatusCode::OK);
    assert_eq!(upload(&app, &cookie, 1).await.status(), StatusCode::OK);
    let r = upload(&app, &cookie, 2).await;
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(r.headers().get("retry-after").is_some());
    let body = json_body(r).await;
    assert_eq!(body["error"], "rate limited");
    assert!(body["retry_after_seconds"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn upload_rejects_executable_disguised_as_text() {
    // First 4 bytes of a Windows PE file: "MZ\x90\x00". The .txt
    // extension would otherwise pass the extension blocklist; magic-byte
    // sniffing catches the lie.
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let mut payload = Vec::with_capacity(128);
    payload.extend_from_slice(b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00");
    payload.extend_from_slice(&[0u8; 100]);

    let boundary = "----testboundary-pe";
    let body = build_multipart(
        boundary,
        &[MultipartField::File(
            "file",
            "notes.txt",
            "text/plain",
            &payload,
        )],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let body = json_body(r).await;
    assert_eq!(body["error"], "file type not allowed");
    // Sniffer reports the real extension regardless of the filename.
    assert_eq!(body["extension"], "exe");
}

#[tokio::test]
async fn upload_sniffs_real_content_type_and_stores_it() {
    // 8-byte PNG magic header + minimal IHDR-ish bytes so infer picks
    // it up. The client lies and says "application/octet-stream";
    // server should override with the sniffed "image/png".
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let mut payload = Vec::with_capacity(64);
    payload.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    payload.extend_from_slice(&[0u8; 56]);

    let boundary = "----testboundary-png";
    let body = build_multipart(
        boundary,
        &[MultipartField::File(
            "file",
            "photo.bin",
            "application/octet-stream",
            &payload,
        )],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let dto = json_body(r).await;
    assert_eq!(dto["content_type"], "image/png");
}

#[tokio::test]
async fn upload_with_thumbnail_stores_and_returns_it() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    // Minimal 1x1 transparent PNG, base64-encoded. Real clients send a
    // 200×200 canvas thumbnail; the shape is identical.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkAAIAAAoAAv/lxKUAAAAASUVORK5CYII=";

    let boundary = "----testboundary-thumb";
    let body = build_multipart(
        boundary,
        &[
            MultipartField::File("file", "photo.png", "image/png", b"PNGDATA"),
            MultipartField::Text("thumbnail", png),
        ],
    );
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            "/api/files",
            &cookie,
            Some(&format!("multipart/form-data; boundary={boundary}")),
            Body::from(body),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let dto = json_body(r).await;
    assert_eq!(dto["thumbnail"].as_str().unwrap(), png);

    // Listing surfaces it too.
    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            "/api/folders/root/children",
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    let listed = json_body(r).await;
    assert_eq!(listed["files"][0]["thumbnail"].as_str().unwrap(), png);
}

#[tokio::test]
async fn upload_with_oversize_or_garbage_thumbnail_drops_it_silently() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    // Over the 64 KB cap.
    let huge = format!("data:image/png;base64,{}", "A".repeat(70_000));
    // Not a data URI at all.
    let garbage = "<script>alert(1)</script>";

    for thumb in [huge.as_str(), garbage] {
        let boundary = "----testboundary-thumb-bad";
        let body = build_multipart(
            boundary,
            &[
                MultipartField::File("file", "p.png", "image/png", b"data"),
                MultipartField::Text("thumbnail", thumb),
            ],
        );
        let r = app
            .clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                &cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "upload should still succeed");
        let dto = json_body(r).await;
        assert!(
            dto.get("thumbnail").is_none() || dto["thumbnail"].is_null(),
            "thumbnail should be absent; got {dto}"
        );
    }
}

#[tokio::test]
async fn rename_then_move_then_trash_then_restore() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;

    // Create a folder and upload a file into root.
    let folder = json_body(
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/folders",
                &cookie,
                Some("application/json"),
                Body::from(r#"{"name":"Reports"}"#),
            ))
            .await
            .unwrap(),
    )
    .await;
    let folder_id = folder["id"].as_str().unwrap().to_string();

    let boundary = "----b";
    let body = build_multipart(
        boundary,
        &[MultipartField::File("file", "a.txt", "text/plain", b"hi")],
    );
    let file = json_body(
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                &cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap(),
    )
    .await;
    let file_id = file["id"].as_str().unwrap().to_string();

    // Rename the file.
    let r = app
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/api/files/{file_id}"),
            &cookie,
            Some("application/json"),
            Body::from(r#"{"name":"a-renamed.txt"}"#),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json_body(r).await["name"], "a-renamed.txt");

    // Move into the folder.
    let r = app
        .clone()
        .oneshot(auth_req(
            "PATCH",
            &format!("/api/files/{file_id}"),
            &cookie,
            Some("application/json"),
            Body::from(format!(r#"{{"parent_id":"{folder_id}"}}"#)),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(json_body(r).await["parent_id"], folder_id);

    // Root no longer lists it.
    let listed = json_body(
        app.clone()
            .oneshot(auth_req(
                "GET",
                "/api/folders/root/children",
                &cookie,
                None,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert!(listed["files"].as_array().unwrap().is_empty());

    // Trash it.
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/api/files/{file_id}/trash"),
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);

    // Restore puts it back under the folder.
    let r = app
        .clone()
        .oneshot(auth_req(
            "POST",
            &format!("/api/files/{file_id}/restore"),
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    let inside = json_body(
        app.clone()
            .oneshot(auth_req(
                "GET",
                &format!("/api/folders/{folder_id}"),
                &cookie,
                None,
                Body::empty(),
            ))
            .await
            .unwrap(),
    )
    .await;
    let kids = inside["children"]["files"].as_array().unwrap();
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0]["id"], file_id);
}

#[tokio::test]
async fn download_redirects_to_signed_user_content_url() {
    let app = router(fixture().await);
    let cookie = sign_in(&app).await;
    let boundary = "----b";
    let body = build_multipart(
        boundary,
        &[MultipartField::File("file", "x.txt", "text/plain", b"abc")],
    );
    let file = json_body(
        app.clone()
            .oneshot(auth_req(
                "POST",
                "/api/files",
                &cookie,
                Some(&format!("multipart/form-data; boundary={boundary}")),
                Body::from(body),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = file["id"].as_str().unwrap();
    let r = app
        .clone()
        .oneshot(auth_req(
            "GET",
            &format!("/api/files/{id}/download"),
            &cookie,
            None,
            Body::empty(),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FOUND);
    let loc = r.headers().get("location").unwrap().to_str().unwrap();
    assert!(
        loc.starts_with(&format!("http://{UCN}/raw/")),
        "got location {loc}"
    );
}

// ─── Multipart helper ────────────────────────────────────────────────────

enum MultipartField<'a> {
    Text(&'a str, &'a str),
    File(&'a str, &'a str, &'a str, &'a [u8]),
}

fn build_multipart(boundary: &str, fields: &[MultipartField<'_>]) -> Bytes {
    let mut out: Vec<u8> = Vec::new();
    for f in fields {
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\n");
        match f {
            MultipartField::Text(name, value) => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                        .as_bytes(),
                );
            }
            MultipartField::File(name, filename, content_type, bytes) => {
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                         Content-Type: {content_type}\r\n\r\n"
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(bytes);
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");
    Bytes::from(out)
}
