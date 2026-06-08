# 15 — Sandboxed PDF/video thumbnail subprocess (Phase 3)

> **Status:** Phase A shipped (subprocess + rlimits + ffmpeg video). Phase B (pdfium PDF) and Phase C (Linux seccomp) planned.


The image-only path shipped in v0 (§5.4 + `docs/research/11-server-thumbnails.md`). PDFs and videos remained deferred because their decoders are real CVE surface — pdfium, ffmpeg, image-magick, libheif have all had remote-code-execution bugs in the last few years that Drive's in-process workers would inherit verbatim. The security brief calls out the rule: untrusted bytes from untrusted producers (image / PDF / video) must run in a sandboxed worker.

This brief locks the subprocess shape.

## Why now

The image worker already covers most thumbnail value (image grids feel finished). PDF + video thumbnails are the second-tier polish — they make a Drive holding `architecture.pdf` and `kickoff.mp4` look noticeably better. The cost is dragging in `pdfium-rs` / `ffmpeg-the-third` (or piping to the `ffmpeg` CLI), both of which we will absolutely not run in the Axum process.

## Locked decisions

### Process boundary, not thread

- A new binary `drive-thumb-worker` shipped alongside `drive`. Forked from Drive on demand, communicates over stdin/stdout JSON-RPC.
- The worker process runs as a different uid (the operator configures `DRIVE_WORKER_UID`) when possible (Linux), so even an RCE inside pdfium can't read Drive's secrets or the bucket creds.
- Worker exits after each job (no persistent state) so any compromise is bounded to one file's lifetime.

### Resource limits enforced via OS

Per-job, before exec'ing the decoder:

- **`RLIMIT_AS` = 512 MB** virtual memory. Stops decoder bombs (the PNG bomb's PDF cousin).
- **`RLIMIT_CPU` = 30 s**. PDFs with millions of vector ops can spin forever; cut them off.
- **`RLIMIT_NOFILE` = 32**. Decoders shouldn't open more than the input + output handles.
- **`RLIMIT_NPROC` = 0** for the worker user. No fork → no shell-out from a compromised decoder.
- **seccomp** on Linux: deny `socket`, `connect`, `bind`, `listen`, `execve` after the decoder loads. The worker doesn't need network. Drive's parent process is untouched.

### JSON-RPC over stdin/stdout, not a socket

- One job per worker invocation. Parent writes a JSON request (`{kind, bytes_path, size_px, fit_mode}`); worker writes a JSON response (`{ok, png_path}` or `{ok: false, error}`).
- Bytes are passed by **path** to a temp file the parent writes (not piped through stdin) — keeps the JSON payload small + avoids two-way streaming complexity.
- Result PNG is written to another temp file path the parent reads.
- Why not a long-lived socket: simpler lifecycle, no connection state to manage, no risk of a poisoned worker contaminating the next job.

### One worker per concurrent job — bounded pool in the parent

- Drive's parent process keeps a `Semaphore::new(4)` (configurable `DRIVE_THUMB_CONCURRENCY`). 4 simultaneous decode jobs at most.
- Each acquire spawns a fresh worker; worker exits when done; the next acquire spawns a new one.
- No worker reuse — the seccomp + rlimit setup is amortised against the decode time, and worker startup is sub-100ms on Linux.

### PDF: `pdfium-render` (the Chromium PDF engine wrapped); video: `ffmpeg` CLI

- **PDF.** `pdfium-render` over hand-rolled `lopdf` because pdfium has Google's fuzz coverage and decades of hostile-input hardening. The crate is heavy (10+ MB native lib) but only the worker links it.
- **Video.** Shell out to `ffmpeg -ss 10% -i input -frames:v 1 output.png` rather than link `ffmpeg-the-third` — the CLI is what's already been pen-tested at internet scale, and we get to ride the distro's security updates. Worker rejects jobs when `ffmpeg` isn't on `PATH`.

### Worker absence → `thumbs_state = 'unsupported'`, not failed

- If `drive-thumb-worker` isn't on `PATH` (operator didn't bundle it), Drive treats PDF + video files as `unsupported` rather than `failed` — the row stops getting retried.
- The image-only path keeps working without the worker binary; only the new kinds need it.

## Locked-out decisions

- **In-process via `ffmpeg-the-third` / `pdfium-render`.** That's the whole reason this brief exists. No.
- **Docker as the sandbox.** Adds a Docker dependency to operators who picked Drive partly *because* it doesn't need Docker. The subprocess + seccomp combo is lighter.
- **WASM-based decoders.** PDFium-WASM exists (Mozilla's PDF.js path) but compressed thumbnails out of WASM JS-side is awkward + slow. Worth revisiting in v0.5 if it matures.
- **AV / virus scanning.** Different feature; ClamAV hook lives elsewhere (§6.8).
- **OCR / text extraction during thumbnail.** Tempting (alt text for accessibility) but blows scope — OCR is its own brief.

## Threat model

| Risk | Mitigation |
|---|---|
| **RCE inside pdfium / ffmpeg** | Different uid, no network, no fork, file-descriptor cap, CPU + memory limits, seccomp denying execve after decoder load. A compromise can't read Drive secrets, can't make outbound connections, and dies in ≤ 30s anyway. |
| **Decoder hangs** | RLIMIT_CPU + a wall-clock kill after 60s in the parent. |
| **OOM from a 10000-page PDF** | RLIMIT_AS at 512 MB. Page count cap (250) before handing to pdfium — we only need page 1. |
| **Symlink escape from temp dir** | Temp files created with `O_NOFOLLOW`; output path checked against a known prefix. |
| **Worker spawns shells** | seccomp blocks `execve` post-decoder load; RLIMIT_NPROC zero prevents fork. |
| **Cache poisoning via crafted PDF** | The output goes to the bucket under `thumbs/{id}/{size}.png`. If the PNG is somehow malicious, it's still just an image rendered into a sandboxed `/raw/{token}` page on the user-content origin — same threat model as a user-uploaded image. |

## Config

```
DRIVE_THUMB_WORKER_PATH=/usr/local/bin/drive-thumb-worker  # default: alongside the drive binary
DRIVE_THUMB_WORKER_UID=2000                                # default: same as drive process (no privilege drop)
DRIVE_THUMB_CONCURRENCY=4
DRIVE_THUMB_JOB_TIMEOUT_SECS=60
```

The `_UID` field gates privilege-drop behaviour. When unset, Drive runs the worker as the same user (operator already containerized + uid-isolated the whole thing). When set, Drive performs the setuid(2) inside the spawned worker after fork.

## Implementation surface

Three pieces:

1. **`crates/drive-thumb-worker/`** (new) — a small Rust binary. `main()` reads a JSON request from stdin, dispatches to `pdf::render` or `video::render`, writes JSON response to stdout. Links pdfium-render; shells out to ffmpeg for video. ~200 LOC + integration tests against real fixture PDFs/MP4s.
2. **`crates/drive-storage/src/thumbnails.rs`** — `SubprocessWorker` impl alongside the existing `ImageOnlyWorker`. Dispatches `ThumbnailKind::Pdf` / `Video` to the subprocess; `Image` still goes in-process (no need to pay the spawn cost for the safe path).
3. **`crates/drive-http/src/thumbs.rs`** — switch from `ImageOnlyWorker` to a `MultiKindWorker` that wraps both. No handler changes.

## Test plan

- Fixture PDF (single page, vector text) → 256-px PNG matches a known SHA when rendered repeatedly.
- Fixture MP4 (5-second clip) → 256-px PNG of the frame at ~10% mark.
- Decoder bomb PDF (recursive XObjects) → killed by RLIMIT_CPU, worker exits non-zero, parent records `thumbs_state = 'failed'`.
- Missing `ffmpeg` binary on PATH → video job returns `unsupported`, not `failed`.
- Missing worker binary on PATH → PDF/video files transition to `unsupported` without spawning.
- Network attempt from inside the worker → blocked by seccomp; verified with a fixture that tries to `connect()`.
- Privilege drop verified on Linux: spawned worker's `/proc/self/status` shows the configured UID.

## Distribution

- The `drive` Docker image gains a second binary (`drive-thumb-worker`) in the same layer. Operators running `docker run ghcr.io/schnsrw/casual-drive:latest` get both.
- Source builds: `cargo build --workspace` builds both binaries; install instructions get a one-line addition.
- Bare-metal installs without `ffmpeg` on PATH gracefully degrade to "PDF thumbs only" (worker still spawns for PDFs).

## Out of scope for v0.5

- **Smart cropping.** Face detection / salient region for thumbnails. Different worker, different brief.
- **HEIC / RAW image support.** Would require swapping the `image` crate for `libheif`-via-subprocess. Worth doing once iPhone uploads become common in the feedback channel.
- **OCR.** The accessibility win is real, but OCR engines (tesseract, paddlepaddle) are big. Own brief.
- **First-frame heuristics for "good" video posters.** v0.4 just takes the 10% frame; smart-frame-selection is a follow-up.
- **PDF page picker** (user picks which page to thumbnail). Reasonable, but a CLI-only operator concern for now.
