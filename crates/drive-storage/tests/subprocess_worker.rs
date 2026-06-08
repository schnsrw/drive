//! Phase 3 §15 — SubprocessWorker integration tests.
//!
//! We don't depend on the real `drive-thumb-worker` binary being built
//! here — `cargo test -p drive-storage` shouldn't transitively build a
//! sibling crate just to assert wire-protocol behaviour. Instead the
//! tests write a tiny shell script that emits a pre-formatted JSON
//! response, and use that as the "worker." This keeps the tests fast
//! and decoupled from the worker's PDF/video implementation.

use std::time::Duration;

use bytes::Bytes;
use drive_storage::{
    FitMode, MultiKindWorker, SubprocessWorker, ThumbnailError, ThumbnailKind, ThumbnailWorker,
};

/// Make a `path` executable on Unix. No-op on Windows (we don't ship there).
fn chmod_exec(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

/// Write a shell stub that:
///   1. Reads the JSON request on stdin (consumes it, ignored).
///   2. Writes the bytes `body` to the request's `output_path`.
///   3. Emits a JSON Ok response on stdout pointing at that path.
fn write_ok_stub(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("worker.sh");
    let body = r#"#!/usr/bin/env bash
set -euo pipefail
REQ=$(cat)
OUT_PATH=$(echo "$REQ" | python3 -c 'import sys,json;print(json.loads(sys.stdin.read())["output_path"])')
printf '\x89PNG\r\n\x1a\nfake' > "$OUT_PATH"
printf '{"ok":true,"output_path":"%s"}\n' "$OUT_PATH"
"#;
    std::fs::write(&script, body).unwrap();
    chmod_exec(&script);
    script
}

/// Stub that emits an Unsupported error response.
fn write_unsupported_stub(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("worker.sh");
    let body = r#"#!/usr/bin/env bash
cat > /dev/null
printf '{"ok":false,"error":"no ffmpeg here","kind":"unsupported"}\n'
exit 1
"#;
    std::fs::write(&script, body).unwrap();
    chmod_exec(&script);
    script
}

/// Stub that hangs forever — used to exercise the wall-clock timeout.
fn write_hang_stub(dir: &std::path::Path) -> std::path::PathBuf {
    let script = dir.join("worker.sh");
    let body = r"#!/usr/bin/env bash
cat > /dev/null
sleep 30
";
    std::fs::write(&script, body).unwrap();
    chmod_exec(&script);
    script
}

#[tokio::test]
async fn subprocess_worker_returns_png_on_ok_response() {
    let dir = tempfile::tempdir().unwrap();
    let stub = write_ok_stub(dir.path());
    let w = SubprocessWorker::new(stub, Duration::from_secs(5), 2);

    let png = w
        .generate(
            ThumbnailKind::Pdf,
            Bytes::from_static(b"fake pdf bytes"),
            256,
            FitMode::Cover,
        )
        .await
        .expect("stub worker should return PNG bytes");
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G']),
        "stub wrote PNG magic — got {:x?}",
        &png[..png.len().min(8)]
    );
}

#[tokio::test]
async fn subprocess_worker_propagates_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let stub = write_unsupported_stub(dir.path());
    let w = SubprocessWorker::new(stub, Duration::from_secs(5), 2);

    let err = w
        .generate(
            ThumbnailKind::Video,
            Bytes::from_static(b"x"),
            256,
            FitMode::Cover,
        )
        .await
        .unwrap_err();
    match err {
        ThumbnailError::Unsupported(ThumbnailKind::Video) => {}
        other => panic!("expected Unsupported(Video), got {other:?}"),
    }
}

#[tokio::test]
async fn subprocess_worker_honours_wall_clock_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let stub = write_hang_stub(dir.path());
    // 250ms timeout — comfortably below the stub's 30s sleep.
    let w = SubprocessWorker::new(stub, Duration::from_millis(250), 2);

    let start = std::time::Instant::now();
    let err = w
        .generate(
            ThumbnailKind::Pdf,
            Bytes::from_static(b"x"),
            256,
            FitMode::Cover,
        )
        .await
        .unwrap_err();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "timeout should fire well before stub finishes (took {elapsed:?})"
    );
    match err {
        ThumbnailError::Decode(m) => assert!(m.contains("timed out"), "got {m}"),
        other => panic!("expected Decode(timed out), got {other:?}"),
    }
}

#[tokio::test]
async fn subprocess_worker_rejects_image_kind() {
    // Images go in-process via ImageOnlyWorker; if a caller mistakenly
    // routes Image to SubprocessWorker, surface it loudly rather than
    // paying the spawn cost only to fail mid-way.
    let dir = tempfile::tempdir().unwrap();
    let stub = write_ok_stub(dir.path());
    let w = SubprocessWorker::new(stub, Duration::from_secs(5), 2);

    let err = w
        .generate(
            ThumbnailKind::Image,
            Bytes::from_static(b"x"),
            256,
            FitMode::Cover,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ThumbnailError::Unsupported(ThumbnailKind::Image)
    ));
}

#[tokio::test]
async fn multi_kind_image_only_renders_image_and_refuses_pdf() {
    // No subprocess configured → image kind still works; PDF + Video
    // surface Unsupported so the calling row flips cleanly.
    let png = {
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        Bytes::from(buf)
    };
    let mk = MultiKindWorker::image_only();
    let out = mk
        .generate(ThumbnailKind::Image, png.clone(), 96, FitMode::Cover)
        .await
        .unwrap();
    assert!(out.starts_with(&[0x89, b'P', b'N', b'G']));

    let err = mk
        .generate(ThumbnailKind::Pdf, png, 96, FitMode::Cover)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ThumbnailError::Unsupported(ThumbnailKind::Pdf)
    ));
}

#[tokio::test]
async fn multi_kind_routes_pdf_through_subprocess() {
    let dir = tempfile::tempdir().unwrap();
    let stub = write_ok_stub(dir.path());
    let inner = SubprocessWorker::new(stub, Duration::from_secs(5), 2);
    let mk = MultiKindWorker::new(Some(inner));

    let png = mk
        .generate(
            ThumbnailKind::Pdf,
            Bytes::from_static(b"fake"),
            256,
            FitMode::Cover,
        )
        .await
        .expect("multi-kind should route PDF to subprocess");
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
}
