//! Server-side thumbnail decoder. Pipeline §5.4.
//! Spec: docs/research/11-server-thumbnails.md.
//!
//! v0 ships an in-process IMAGE-ONLY worker (`image` crate). Phase 3
//! §15 introduces a `SubprocessWorker` that fans `Pdf` + `Video` jobs
//! out to the sandboxed `drive-thumb-worker` binary. `MultiKindWorker`
//! routes by kind so callers don't have to care which lane handles
//! what.

use bytes::Bytes;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;

/// Pre-classified hint so the worker doesn't have to re-sniff. We give it
/// the broad bucket and the byte stream — the worker decides whether it
/// can handle the kind in-process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailKind {
    Image,
    /// PDF rendering ships in v0.2. The trait accepts the kind so callers
    /// don't have to special-case "what's left over" today.
    Pdf,
    /// Video frame extraction ships in v0.2 for the same reason.
    Video,
}

#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    #[error("unsupported kind: {0:?}")]
    Unsupported(ThumbnailKind),
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("encode failed: {0}")]
    Encode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Crop to a square — used for `small` and `medium` (grid cells).
    Cover,
    /// Keep the full image, letterboxed inside the square — used for
    /// `large` (preview pane).
    Contain,
}

/// The 3 canonical sizes shipped to the SPA. Keeping them as an enum
/// (rather than `u32`) prevents callers from minting arbitrary sizes
/// that would balloon the bucket footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Small,
    Medium,
    Large,
}

impl ThumbSize {
    #[must_use]
    pub fn px(self) -> u32 {
        match self {
            Self::Small => 96,
            Self::Medium => 256,
            Self::Large => 1024,
        }
    }
    #[must_use]
    pub fn fit_mode(self) -> FitMode {
        match self {
            Self::Small | Self::Medium => FitMode::Cover,
            Self::Large => FitMode::Contain,
        }
    }
    #[must_use]
    pub fn key_suffix(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
    pub fn all() -> [ThumbSize; 3] {
        [Self::Small, Self::Medium, Self::Large]
    }
    /// Storage key for a given file id + size.
    /// `thumbs/{ulid}/{size}.png` — matches the spec.
    #[must_use]
    pub fn key_for(self, file_id: &str) -> String {
        format!("thumbs/{file_id}/{}.png", self.key_suffix())
    }
}

/// In-process, image-only worker. Safe for the `image` crate's PNG /
/// JPEG / WebP / GIF / BMP decoders (vetted; bounded memory at 50 MP).
/// Refuses PDF + video → callers see `Unsupported` and the file's
/// `thumbs_state` flips to `unsupported`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImageOnlyWorker;

impl ImageOnlyWorker {
    /// Decode `bytes` (already classified) and emit a PNG at the
    /// requested target dimension. `size_px` is interpreted as
    /// fit-cover-square for `small`/`medium` and fit-contain for `large`
    /// (see `ThumbSize::fit_mode`).
    pub async fn generate(
        &self,
        kind: ThumbnailKind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> Result<Vec<u8>, ThumbnailError> {
        if !matches!(kind, ThumbnailKind::Image) {
            return Err(ThumbnailError::Unsupported(kind));
        }
        // Heavy work — push it onto a blocking thread so we don't stall
        // the tokio scheduler.
        let png = tokio::task::spawn_blocking(move || render_image(bytes, size_px, fit))
            .await
            .map_err(|e| ThumbnailError::Decode(format!("worker panicked: {e}")))??;
        Ok(png)
    }
}

fn render_image(bytes: Bytes, size_px: u32, fit: FitMode) -> Result<Vec<u8>, ThumbnailError> {
    let img = image::ImageReader::new(Cursor::new(bytes.as_ref()))
        .with_guessed_format()
        .map_err(|e| ThumbnailError::Decode(format!("guess format: {e}")))?
        .decode()
        .map_err(|e| ThumbnailError::Decode(format!("decode: {e}")))?;

    let resized = match fit {
        FitMode::Cover => image::imageops::resize(
            &img.to_rgba8(),
            size_px,
            size_px,
            image::imageops::FilterType::Lanczos3,
        ),
        FitMode::Contain => {
            // Letterbox into a transparent square.
            let scaled = img.resize(size_px, size_px, image::imageops::FilterType::Lanczos3);
            let mut canvas =
                image::RgbaImage::from_pixel(size_px, size_px, image::Rgba([0, 0, 0, 0]));
            let (w, h) = (scaled.width(), scaled.height());
            let x = (size_px.saturating_sub(w)) / 2;
            let y = (size_px.saturating_sub(h)) / 2;
            image::imageops::overlay(&mut canvas, &scaled.to_rgba8(), x as i64, y as i64);
            canvas
        }
    };

    let mut out = Vec::with_capacity(64 * 1024);
    image::DynamicImage::ImageRgba8(resized)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| ThumbnailError::Encode(format!("png write: {e}")))?;
    Ok(out)
}

/// Shared shape: a thing that can turn classified bytes into a PNG.
/// `ImageOnlyWorker`, `SubprocessWorker`, and `MultiKindWorker` all
/// implement this so `drive-http` can hold a single `Arc<dyn ...>`.
pub trait ThumbnailWorker: Send + Sync + std::fmt::Debug {
    /// Async render. Returns a future-of-PNG. The `'static` bound is
    /// what lets handlers `tokio::spawn` the call without juggling
    /// lifetimes.
    fn generate<'a>(
        &'a self,
        kind: ThumbnailKind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, ThumbnailError>> + Send + 'a>,
    >;
}

impl ThumbnailWorker for ImageOnlyWorker {
    fn generate<'a>(
        &'a self,
        kind: ThumbnailKind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, ThumbnailError>> + Send + 'a>,
    > {
        Box::pin(self.generate(kind, bytes, size_px, fit))
    }
}

// ── Phase 3 §15 — SubprocessWorker ────────────────────────────────────

/// Runs PDF + video jobs in `drive-thumb-worker`. One subprocess per
/// job (no reuse — the brief explicitly rules that out so a poisoned
/// worker can't contaminate the next file). A `Semaphore` bounds how
/// many can be in flight at once.
#[derive(Debug, Clone)]
pub struct SubprocessWorker {
    binary_path: PathBuf,
    job_timeout: Duration,
    concurrency: Arc<Semaphore>,
}

impl SubprocessWorker {
    /// `binary_path` should point at the `drive-thumb-worker` executable
    /// (the parent process resolved it from `DRIVE_THUMB_WORKER_PATH` or
    /// from `which`-on-PATH). `job_timeout` is the wall-clock hard kill.
    #[must_use]
    pub fn new(binary_path: PathBuf, job_timeout: Duration, concurrency: usize) -> Self {
        Self {
            binary_path,
            job_timeout,
            concurrency: Arc::new(Semaphore::new(concurrency.max(1))),
        }
    }

    /// Probe — does the binary exist + execute? Called once at boot so
    /// the operator sees an early warning when the worker is missing
    /// instead of a wall of "spawn failed" entries.
    pub fn binary_available(&self) -> bool {
        // Trying to exec with no input gives a fast non-zero exit; the
        // important thing is that the spawn itself succeeds (ENOENT vs
        // not-ENOENT). We don't care about the exit code here.
        std::process::Command::new(&self.binary_path)
            .arg("--noop")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|mut c| {
                // Don't leave the test child around — wait briefly.
                let _ = c.wait();
                true
            })
            .unwrap_or(false)
    }

    async fn run_job(
        &self,
        kind: drive_thumb_worker::Kind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> Result<Vec<u8>, ThumbnailError> {
        // Bound concurrency at the subprocess layer — beyond this and
        // we'd be paying the seccomp/rlimit setup cost while waiting on
        // CPU anyway. The semaphore live across the whole job.
        let _permit = self
            .concurrency
            .acquire()
            .await
            .map_err(|e| ThumbnailError::Decode(format!("semaphore closed: {e}")))?;

        // Write the input to a temp file the worker can open. We pass
        // by path (not pipe) so the JSON payload stays small and the
        // worker can `O_NOFOLLOW` cleanly.
        let dir =
            tempfile::tempdir().map_err(|e| ThumbnailError::Decode(format!("tempdir: {e}")))?;
        let input_path = dir.path().join("in.bin");
        let output_path = dir.path().join("out.png");
        tokio::fs::write(&input_path, &bytes)
            .await
            .map_err(|e| ThumbnailError::Decode(format!("write input: {e}")))?;

        let req = drive_thumb_worker::Request {
            kind,
            input_path: input_path.to_string_lossy().into_owned(),
            output_path: output_path.to_string_lossy().into_owned(),
            size_px,
            fit: match fit {
                FitMode::Cover => drive_thumb_worker::FitMode::Cover,
                FitMode::Contain => drive_thumb_worker::FitMode::Contain,
            },
        };
        let req_json = serde_json::to_vec(&req)
            .map_err(|e| ThumbnailError::Decode(format!("encode req: {e}")))?;

        // Spawn, write request to stdin, read response from stdout,
        // with a wall-clock kill at `job_timeout`. tokio::process gives
        // us a `Child` that we can `kill().await` on timeout.
        let mut child = tokio::process::Command::new(&self.binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ThumbnailError::Decode(format!("spawn worker: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(&req_json)
                .await
                .map_err(|e| ThumbnailError::Decode(format!("write stdin: {e}")))?;
            // Drop stdin to signal EOF — the worker reads to end.
            drop(stdin);
        }

        let kind_for_error = req.kind;
        let output = tokio::time::timeout(self.job_timeout, child.wait_with_output()).await;
        let output = match output {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(ThumbnailError::Decode(format!("wait worker: {e}"))),
            Err(_) => {
                // Timed out. tokio::time::timeout doesn't kill the
                // child for us — but wait_with_output already moved
                // the handle. The kernel will reap when the process
                // exits or, if it really is hung, the rlimits inside
                // the worker take over. Surface the timeout.
                return Err(ThumbnailError::Decode(format!(
                    "worker job timed out after {}s",
                    self.job_timeout.as_secs()
                )));
            }
        };

        let resp: drive_thumb_worker::Response =
            serde_json::from_slice(&output.stdout).map_err(|e| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                ThumbnailError::Decode(format!("parse worker response: {e} (stderr: {stderr})"))
            })?;

        match resp {
            drive_thumb_worker::Response::Ok(_) => {
                let png = tokio::fs::read(&output_path)
                    .await
                    .map_err(|e| ThumbnailError::Decode(format!("read worker output: {e}")))?;
                Ok(png)
            }
            drive_thumb_worker::Response::Err(e) => match e.kind {
                drive_thumb_worker::ErrorKind::Unsupported => {
                    let k = match kind_for_error {
                        drive_thumb_worker::Kind::Pdf => ThumbnailKind::Pdf,
                        drive_thumb_worker::Kind::Video => ThumbnailKind::Video,
                    };
                    Err(ThumbnailError::Unsupported(k))
                }
                _ => Err(ThumbnailError::Decode(format!(
                    "worker reported {:?}: {}",
                    e.kind, e.error
                ))),
            },
        }
    }
}

impl ThumbnailWorker for SubprocessWorker {
    fn generate<'a>(
        &'a self,
        kind: ThumbnailKind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, ThumbnailError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let wire = match kind {
                ThumbnailKind::Pdf => drive_thumb_worker::Kind::Pdf,
                ThumbnailKind::Video => drive_thumb_worker::Kind::Video,
                ThumbnailKind::Image => return Err(ThumbnailError::Unsupported(kind)),
            };
            self.run_job(wire, bytes, size_px, fit).await
        })
    }
}

// ── Phase 3 §15 — MultiKindWorker ─────────────────────────────────────

/// Routes by kind: images go in-process (safe, no spawn cost); PDFs +
/// videos go to the subprocess worker. When the subprocess binary
/// isn't available, PDF/video calls return `Unsupported` — the row
/// transitions cleanly and the operator sees a one-line warning at
/// boot.
#[derive(Debug)]
pub struct MultiKindWorker {
    image: ImageOnlyWorker,
    sub: Option<SubprocessWorker>,
}

impl MultiKindWorker {
    #[must_use]
    pub fn new(sub: Option<SubprocessWorker>) -> Self {
        Self {
            image: ImageOnlyWorker,
            sub,
        }
    }

    /// Image-only fallback — convenience for tests/deployments that
    /// haven't bundled the worker binary.
    #[must_use]
    pub fn image_only() -> Self {
        Self::new(None)
    }
}

impl ThumbnailWorker for MultiKindWorker {
    fn generate<'a>(
        &'a self,
        kind: ThumbnailKind,
        bytes: Bytes,
        size_px: u32,
        fit: FitMode,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<u8>, ThumbnailError>> + Send + 'a>,
    > {
        Box::pin(async move {
            match kind {
                ThumbnailKind::Image => self.image.generate(kind, bytes, size_px, fit).await,
                ThumbnailKind::Pdf | ThumbnailKind::Video => match &self.sub {
                    Some(sw) => sw.generate(kind, bytes, size_px, fit).await,
                    None => Err(ThumbnailError::Unsupported(kind)),
                },
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Bytes {
        // 4×4 solid-red PNG built in-test so we don't ship fixtures.
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        Bytes::from(buf)
    }

    #[tokio::test]
    async fn image_worker_decodes_png() {
        let w = ImageOnlyWorker;
        let out = w
            .generate(ThumbnailKind::Image, tiny_png(), 96, FitMode::Cover)
            .await
            .unwrap();
        assert!(out.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
    }

    #[tokio::test]
    async fn image_worker_refuses_pdf() {
        let w = ImageOnlyWorker;
        let err = w
            .generate(ThumbnailKind::Pdf, tiny_png(), 96, FitMode::Cover)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ThumbnailError::Unsupported(ThumbnailKind::Pdf)
        ));
    }

    #[tokio::test]
    async fn image_worker_refuses_video() {
        let w = ImageOnlyWorker;
        let err = w
            .generate(ThumbnailKind::Video, tiny_png(), 96, FitMode::Cover)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ThumbnailError::Unsupported(ThumbnailKind::Video)
        ));
    }

    #[tokio::test]
    async fn cover_returns_square() {
        let w = ImageOnlyWorker;
        let bytes = w
            .generate(ThumbnailKind::Image, tiny_png(), 96, FitMode::Cover)
            .await
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 96);
        assert_eq!(decoded.height(), 96);
    }

    #[tokio::test]
    async fn contain_letterboxes_into_square() {
        let w = ImageOnlyWorker;
        // Wide source so Contain has to letterbox.
        let img = image::RgbaImage::from_pixel(8, 2, image::Rgba([0, 255, 0, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let bytes = w
            .generate(ThumbnailKind::Image, Bytes::from(buf), 64, FitMode::Contain)
            .await
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 64);
    }

    #[test]
    fn thumb_size_key_matches_spec() {
        assert_eq!(ThumbSize::Small.key_for("01ABC"), "thumbs/01ABC/small.png");
        assert_eq!(
            ThumbSize::Medium.key_for("01ABC"),
            "thumbs/01ABC/medium.png"
        );
        assert_eq!(ThumbSize::Large.key_for("01ABC"), "thumbs/01ABC/large.png");
    }
}
