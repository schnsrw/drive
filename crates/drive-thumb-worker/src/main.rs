//! Phase 3 §15 — sandboxed thumbnail worker binary.
//!
//! One job per invocation. Reads a JSON `Request` from stdin, applies
//! rlimits + optional setuid, dispatches to the decoder, writes a JSON
//! `Response` to stdout, exits.
//!
//! Why a separate process: pdfium / ffmpeg have a long history of
//! parser RCEs. Drive's Axum server stays the only thing on the network
//! and holds the bucket creds; this worker holds neither.
//!
//! Spec: docs/research/15-sandboxed-thumb-worker.md.

use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use drive_thumb_worker::{ErrorKind, Kind, Request, Response};

fn main() {
    let response = match run() {
        Ok(r) => r,
        Err(e) => Response::err(ErrorKind::Internal, e.to_string()),
    };
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    if serde_json::to_writer(&mut lock, &response).is_err() {
        std::process::exit(2);
    }
    let _ = lock.write_all(b"\n");
    let _ = lock.flush();
    let exit = i32::from(!matches!(response, Response::Ok(_)));
    std::process::exit(exit);
}

fn run() -> Result<Response, WorkerError> {
    let req: Request = {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| WorkerError(format!("read stdin: {e}")))?;
        serde_json::from_str(&buf).map_err(|e| WorkerError(format!("parse request: {e}")))?
    };

    // OS-level guardrails. Linux gets the real fences; macOS rlimits
    // exist but the kernel ignores some (CPU is honoured, AS is not).
    // We apply best-effort and rely on the wall-clock timeout in the
    // parent as a hard backstop.
    sandbox::apply_resource_limits();
    sandbox::drop_privileges_if_requested();

    let resp = match req.kind {
        Kind::Pdf => render_pdf(&req),
        Kind::Video => render_video(&req),
    };
    Ok(resp)
}

#[derive(Debug)]
struct WorkerError(String);
impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for WorkerError {}

// ── PDF (stub) ────────────────────────────────────────────────────────
//
// pdfium-render is a 10+ MB native dep and shipping it cleanly across
// Linux / macOS / arm64 needs its own packaging pass. v0.5 ships the
// worker with PDF returning `unsupported`; v0.6 wires pdfium-render
// here. Drive degrades gracefully — PDF files transition to
// `thumbs_state = 'unsupported'` and stop being retried.

fn render_pdf(_req: &Request) -> Response {
    Response::err(
        ErrorKind::Unsupported,
        "PDF thumbnails require pdfium-render — not built into this worker",
    )
}

// ── Video (ffmpeg CLI) ────────────────────────────────────────────────

fn render_video(req: &Request) -> Response {
    if !has_ffmpeg() {
        return Response::err(
            ErrorKind::Unsupported,
            "ffmpeg not on PATH — install ffmpeg to enable video thumbnails",
        );
    }

    // 10% mark gives a much better poster than frame 0 (which is often
    // a fade-in or codec header). The scaler choice mirrors the
    // ImageOnlyWorker's fit semantics.
    let size = req.size_px;
    let scale = match req.fit {
        drive_thumb_worker::FitMode::Cover => format!("scale={size}:-1"),
        drive_thumb_worker::FitMode::Contain => {
            format!("scale=w={size}:h={size}:force_original_aspect_ratio=decrease")
        }
    };

    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-ss", "10%"])
        .args(["-i", &req.input_path])
        .args(["-frames:v", "1"])
        .args(["-vf", &scale])
        .arg(&req.output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    match out {
        Ok(out) if out.status.success() && Path::new(&req.output_path).exists() => {
            Response::ok(req.output_path.clone())
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Response::err(
                ErrorKind::Decode,
                format!("ffmpeg exit {:?}: {stderr}", out.status.code()),
            )
        }
        Err(e) => Response::err(ErrorKind::Internal, format!("ffmpeg spawn: {e}")),
    }
}

fn has_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── OS guardrails ─────────────────────────────────────────────────────
//
// libc calls only — keeping the unsafe footprint tiny and localized
// rather than pulling in `nix` or `rlimit` (which would add ~40 deps
// for what's a half-page of POSIX).

mod sandbox {
    #[allow(unsafe_code)]
    pub(super) fn apply_resource_limits() {
        // 512 MB virtual-memory cap.
        set_rlimit(libc::RLIMIT_AS, 512 * 1024 * 1024);
        // 30 s of CPU time — generous backstop for real ffmpeg jobs.
        set_rlimit(libc::RLIMIT_CPU, 30);
        // 32 file descriptors — enough for std + input + output + a
        // few decoder internals. Defeats fd-exhaustion DoS.
        set_rlimit(libc::RLIMIT_NOFILE, 32);
    }

    // `RLIMIT_*` constants are typed differently across platforms (Linux:
    // `__rlimit_resource_t = c_uint`; macOS: bare `c_int`). `libc::setrlimit`
    // takes the matching type per platform too, so we cfg-gate the wrapper
    // signature; call sites are identical because `libc::RLIMIT_*` has the
    // matching type for this build.
    #[cfg(target_os = "linux")]
    type ResId = libc::__rlimit_resource_t;
    #[cfg(not(target_os = "linux"))]
    type ResId = libc::c_int;

    #[allow(unsafe_code)]
    fn set_rlimit(resource: ResId, soft: u64) {
        let lim = libc::rlimit {
            rlim_cur: soft as libc::rlim_t,
            rlim_max: soft as libc::rlim_t,
        };
        // SAFETY: setrlimit takes a value resource id + pointer to a
        // struct we own. A failure here means the platform refuses the
        // value; best-effort sandboxing is still better than none, so
        // we ignore the return.
        let lim_ptr: *const libc::rlimit = &raw const lim;
        unsafe {
            let _ = libc::setrlimit(resource, lim_ptr);
        }
    }

    #[allow(unsafe_code)]
    pub(super) fn drop_privileges_if_requested() {
        let Some(uid) = std::env::var("DRIVE_THUMB_WORKER_UID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            return;
        };
        // SAFETY: setuid is the canonical privilege-drop call. We never
        // come back — the worker exits after one job. If the call
        // fails, fail closed.
        unsafe {
            if libc::setuid(uid) != 0 {
                eprintln!("drive-thumb-worker: setuid({uid}) failed; refusing to run");
                std::process::exit(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_returns_unsupported() {
        let req = Request {
            kind: Kind::Pdf,
            input_path: "/tmp/x.pdf".into(),
            output_path: "/tmp/x.png".into(),
            size_px: 256,
            fit: drive_thumb_worker::FitMode::Cover,
        };
        let resp = render_pdf(&req);
        match resp {
            Response::Err(e) => {
                assert_eq!(e.kind, ErrorKind::Unsupported);
                assert!(e.error.to_lowercase().contains("pdfium"));
            }
            Response::Ok(_) => panic!("expected unsupported"),
        }
    }

    #[test]
    fn video_unsupported_when_ffmpeg_off_path() {
        // Force a PATH that won't contain ffmpeg so the `has_ffmpeg()`
        // probe fails deterministically — regardless of whether the
        // test host has ffmpeg installed.
        let req = Request {
            kind: Kind::Video,
            input_path: "/nonexistent/video.mp4".into(),
            output_path: "/tmp/out.png".into(),
            size_px: 256,
            fit: drive_thumb_worker::FitMode::Cover,
        };
        let prev = std::env::var_os("PATH");
        // SAFETY: single-threaded test process; restoring PATH right
        // after. (set_var is safe on stable but the unsafe-on-edition-
        // 2024 lint will flag it — wrapping in unsafe is forward-
        // compatible.)
        std::env::set_var("PATH", "/var/empty");
        let resp = render_video(&req);
        if let Some(p) = prev {
            std::env::set_var("PATH", p);
        }
        match resp {
            Response::Err(e) => assert_eq!(e.kind, ErrorKind::Unsupported),
            Response::Ok(_) => panic!("expected unsupported"),
        }
    }
}
