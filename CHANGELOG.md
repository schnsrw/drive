# Changelog

All notable changes to Casual Drive land here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- OIDC sign-in (Authorization Code + PKCE). `DRIVE_ALLOW_PASSWORD_AUTH=false` hides the password form.
- Sandboxed `drive-thumb-worker` subprocess for video thumbnails (ffmpeg-CLI), with per-job rlimits and optional `setuid`. PDF support deferred.

### Planned

See [`PIPELINE.md`](./PIPELINE.md) — MS365 federation, presence, pdfium PDF thumbnails, Linux seccomp filter, post-finalize magic-byte sniff.

## [0.1.0] — 2026-06-08

First feature-complete release. Single Rust binary; React 19 SPA embedded
via `rust-embed`; SQLite by default, Postgres optional; four storage
backends (fs / memory / S3 / MinIO) plus per-workspace BYO (S3 / MinIO /
R2 / B2) with AES-256-GCM secret envelopes. Marketing site + live demo
live at `drive.schnsrw.live`.

### Identity + sessions

- First-run admin-setup wizard (`/api/setup/{status,admin}`) — no env-only
  bootstrap required; first sign-in creates the admin + their Personal
  workspace.
- Argon2id passwords (OWASP min `m=19 MiB, t=2, p=1`), constant-time-ish
  sign-in (no enumeration leak), server-side sessions in `sessions` table,
  `__Host-cd_sid` cookie in prod (`Secure HttpOnly SameSite=Lax`), CSRF
  token on every non-safe method.
- Caps-lock warning under the password input on the sign-in card.
- Change-password from Settings → Account.

### Workspaces (Phase 1 + Phase 2)

- Every user gets a Personal workspace (auto-created, immutable,
  untransferable). Team workspaces are explicit.
- Owner + Member roles. Owner-only rename / delete / transfer; transfer
  is an atomic SQL transaction.
- Real `WorkspaceSwitcher` in the sidebar; selection persists in
  `localStorage` and re-scopes Files / Notes / search / upload in lockstep
  via the SPA's `WorkspaceContext`.
- File + folder rows are workspace-scoped (`workspace_id` column,
  migration 0006 backfills from the owner's Personal workspace).
- Cross-workspace operations 403; cross-workspace `parent_id` on note
  moves 422.

### Files

- Grid + list views, sort dropdown (persisted), folder navigation with
  breadcrumbs + Backspace, drag-drop upload with ghost rows, multi-select
  + selection bar (bulk download as zip + bulk trash), right-click +
  kebab context menu (Share / Rename / Move / Trash / Download).
- Trash + restore (per-file; per-folder ships in v0.2 with the recursive
  trash worker).
- Search across the active workspace via `GET /api/search?q=`.
- Inline previews for **images** (signed-URL `<img>`), **PDFs**
  (`<iframe>` browser-native viewer), **video** (`<video>`), **audio**
  (`<audio>`), **text** (512 KB cap, `<pre>`), **markdown** (256 KB cap,
  `marked` + DOMPurify).
- Client-side thumbnails on upload: 192-px square for images
  (`createImageBitmap` → canvas → WebP / JPEG / PNG fallback) and for
  videos (offscreen `<video>` → seek 10% → canvas).
- Concurrent upload cap: 4 lanes via `mapWithConcurrency`.

### Notes / Wiki

- Workspace-scoped pages (migration 0008: `notes` + `note_links` tables).
  Personal workspaces = personal notes; team workspaces = team notes.
- Markdown source + live preview side-by-side on desktop; tabbed on
  mobile. `marked` + DOMPurify (no new deps); no Lexical.
- `[[Wiki link]]` syntax. Server re-indexes outgoing links on every body
  save; dangling links resolve when the target page is created later.
  Backlinks rendered under the editor.
- Drag-to-reorder tree (lexicographic `order_key` for O(1) inserts);
  trash + restore; full-text search across title + body.
- `Cmd-N` new page, `Cmd-S` flush save, `Tab`-to-indent in source, 600ms
  debounced auto-save with a "Saved N s ago" microstate.

### Editor handoff (WOPI)

- 7-endpoint WOPI host (CheckFileInfo, GetFile, PutFile, Lock, Unlock,
  RefreshLock, UnlockAndRelock) with the asymmetric 409 + `X-WOPI-Lock`
  contract. File-id-scoped JWT access tokens (HMAC-SHA256), 10-minute TTL.
- Click `.xlsx` → opens in Casual Sheet; click `.docx` → opens in Casual
  Document. Popup-blocker fallback toasts a manual-open link.
- Proof-key RSA hook is wired but stubbed for v0 (no MS365 federation
  yet — design parked in `docs/research/13-ms365-federation.md`).

### Sharing

- `POST /api/files/{id}/share` mints a 128-bit token with optional
  Argon2id-hashed password + expiry + view-only permissions.
- `GET /s/{token}` recipient page with stripped chrome; password gate
  + constant-time compare.
- `DELETE /api/shares/{id}` revokes; existing share-links visible from
  the ShareDialog.

### Cmd-K command palette

- `cmdk`-backed palette mounted at the Shell level. `⌘K` / `Ctrl-K`
  toggles from anywhere outside an editable element.
- Three result groups, populated in parallel: **Go to** (every left-rail
  destination + keyboard-shortcuts modal), **Folders / Files** (against
  `/api/search`, workspace-scoped), **Notes** (against
  `/api/notes/search`). Selection routes via `cd:open-file` /
  `cd:open-note` CustomEvents so the palette stays decoupled.

### Bring-your-own storage per workspace

- Migration 0007: `workspace_storage` table. AES-256-GCM secret envelope
  (`drive-storage::secret_box`) — base64 of `nonce || ciphertext || tag`,
  AAD binds each ciphertext to `<row.id>:<key_version>` so rotations
  invalidate stale cache entries automatically.
- SSRF guard refuses metadata IPs, RFC1918 / link-local / loopback
  unless `DRIVE_ALLOW_INSECURE_BYO=true`, non-http(s) schemes, unknown
  hostnames.
- 5 owner-only endpoints: GET / PUT / DELETE / POST `/test` / PATCH
  `/credentials`. Personal workspaces 409; non-Owner Members 403;
  missing master key 503.
- `StorageRegistry` caches per-workspace adapters keyed by
  `(storage_id, key_version)`. Upload routing picks the workspace's BYO
  adapter when present and pins `files.storage_id` so existing files
  stay on their original bucket when the workspace later flips storage.
- SPA `WorkspaceStorageCard` under Settings → Storage: provider picker
  (S3 / MinIO / R2 / B2), test-then-save flow, replace credentials,
  remove with confirm. Owner-only on Team workspaces.

### Direct-to-storage upload

- Migration 0009: `files.status` enum (uploading / ready / failed) +
  `expected_size`. Quota math counts uploading rows so parallel presigns
  can't both squeeze under the cap.
- 3 endpoints: `POST /api/files/upload-url` (presign + create
  `uploading` row), `POST /api/files/{id}/complete` (stat + flip to
  `ready`), `POST /api/files/{id}/abort` (drop row + best-effort delete).
- 15-minute PUT TTL. Adapters that can't presign (fs / memory) return
  409 — the SPA falls back to the proxy multipart path transparently.
- SPA activates the direct path at files ≥ 8 MiB when
  `VITE_DIRECT_UPLOAD=1`.

### Server-side thumbnails (image slice)

- Migration 0010: `files.thumbs_state` enum (pending / ready /
  unsupported / failed) + `thumbs_generated_at`.
- `ImageOnlyWorker` in `drive-storage::thumbnails` decodes via the
  `image` crate on a blocking thread. Three sizes (96 / 256 / 1024 px),
  WebP / JPEG / PNG cascade, stored under `thumbs/{id}/{size}.png` in
  the same bucket as the original.
- Lazy generation kicked from `GET /api/files/{id}/thumb/{size}` — the
  worker only runs when the SPA actually asks. `POST
  /api/files/{id}/thumb/regenerate` (owner-only) forces a re-run.
- `FileDto` exposes `thumbs_state` + `thumb_urls`; `FileThumb` prefers
  server assets over the inline data URI when ready.
- PDF + video decoders explicitly NOT in-process — design parked in
  `docs/research/15-sandboxed-thumb-worker.md` for v0.2 subprocess.

### Quotas + admin

- Per-user `quota_bytes` (NULL = unlimited). Upload returns 413 on
  exceed; quota math sums against `used_bytes` per workspace including
  `uploading` rows.
- `POST /api/me/quota/request` emits an audit event the admin sees on
  Activity + Admin → Users.
- Admin → Users surface: inline quota editing, add-user dialog (username
  + password + admin toggle + initial quota), quota upgrade-request
  panel with one-click approve.

### Activity + audit

- `audit_events` table populated on every state transition (auth,
  upload, download, trash, share, workspace.*, workspace_storage.*,
  notes.*, files.upload_*, etc.). 20+ action types.
- `/api/activity` paginated feed; SPA renders a grouped-by-day timeline
  with type-tagged badges.
- Audit-emit is fire-and-forget (`tokio::spawn`) so handler latency
  isn't gated on the insert.

### Admin

- `/admin` route shell, admin-only. System health card (version,
  git_sha, built_at, license, storage backend, db backend, uptime,
  active sessions, recent sign-ins). Storage adapter card. Users card.
- Quota upgrade requests surface in the Activity feed AND on the Users
  card with one-click approve.

### Settings

- Real surfaces: Account (change password), Storage (backend readout +
  used / quota + workspace BYO card + signed-URL TTL row), About
  (version / license / build / repo). Polished "Coming in v0.2 —" stubs
  for Workspace / Members / Roles / Sharing / Notifications / API
  tokens — every surface a real Drive has is visible.

### Rate limit + safety

- Per-user upload token bucket (30 uploads/min, 0.5/sec refill). Returns
  429 + `Retry-After`.
- Magic-byte content-type sniffing on upload via `infer` crate;
  executables rejected; sniffed MIME overrides client-asserted.
- `Content-Disposition: attachment` forced on non-previewable types
  served from the user-content origin. Strict `nosniff`.

### Two-origin model

- App origin (`drive.<host>`): SPA + JSON API + WOPI. Cookies live here.
  Strict CSP.
- User-content origin (`usercontent-drive.<host>`): `/raw/{token}` only.
  `CSP: sandbox; default-src 'none'`. No cookies.
- Boot refuses to start in production when the two origins match.

### Signed URLs

- `/api/files/{id}/download` 302s to a signed URL. TTL is configurable
  via `DRIVE_SIGNED_URL_TTL_SECS` (default 300s, floor 30s). Surfaced in
  Settings → Storage. Share-link signed URLs use a tighter 120s on
  purpose (third-party link → minimise exposure).
- S3 / MinIO get native presign via OpenDAL; fs / memory get HMAC-
  signed tokens validated by the user-content `/raw/{token}` handler.

### Marketing site + GH Pages

- New `marketing/` Astro 5 project at `https://drive.schnsrw.live`.
  Multi-page docs site (landing + 4 docs MDX routes + screenshots + the
  embedded demo). Static HTML by default → indexable on first request.
- Per-page SEO: unique title / description, canonical, OG + Twitter,
  JSON-LD `SoftwareApplication` on landing. Dynamic `robots.txt` +
  `sitemap.xml` keyed to `ASTRO_SITE`.
- Self-hosted Inter (fetched by CI), AVIF / WebP / PNG via Astro
  `<Image>`. Pre-built OG card at `/og/default.png` (1200×630).
- Lighthouse CI in the deploy workflow: Performance / Accessibility /
  SEO ≥ 0.95 mobile profile (4× CPU slowdown) hard-fail.
- 16 real screenshots in `marketing/public/screenshots/` (8 surfaces ×
  2 themes, mobile light-only) captured by the Playwright harness at
  `web/tests/e2e/marketing-screenshots.mjs`.
- Embedded demo at `/demo` — the SPA built with `VITE_DEMO_MODE=1` +
  `VITE_BASE=/demo-app/`, served behind a slim iframe-host page.

### CI + deploy

- Backend gates per PR: `cargo fmt --check`, `cargo clippy -- -Dwarnings`,
  `cargo test --workspace`, `cargo audit --deny warnings`, `cargo deny check`.
- Marketing deploy: GitHub Actions builds the SPA in demo mode, copies
  into `marketing/public/demo-app/`, runs `astro build`, executes
  Lighthouse CI, uploads to Pages.
- Multi-stage `cargo-chef` Docker image on `debian:trixie-slim`.

### Specs / docs

- 12 research briefs (`docs/research/00–11`): synthesis, WOPI, auth,
  storage, polish principles, Rust stack, security, marketing,
  BYO storage, notes/wiki, direct upload, server thumbnails.
- 17 surface specs (`docs/ux/01–17`): flows, every UI surface.
- Architecture, contributing, install, configuration documented on the
  marketing site.
- README rewritten to reflect v0 reality.

### Phase 3 design (not implemented; specs only)

- `docs/research/12-oidc.md` — multi-tenant SSO via OIDC.
- `docs/research/13-ms365-federation.md` — Office Online WOPI client
  federation via proof-key RSA validation.
- `docs/research/14-presence.md` — Drive-shell ambient presence (SSE,
  one channel per workspace).
- `docs/research/15-sandboxed-thumb-worker.md` — sandboxed subprocess
  for PDF + video thumbnails (seccomp + rlimits + privilege drop).

### Numbers

- 29 backend test suites passing across `drive-{core,db,storage,wopi,auth,http,bin}`.
- 34 storage unit tests (seal/open, SSRF block list, thumbnail decoder,
  registry cache invariants).
- 0 clippy warnings across `cargo clippy --workspace --tests`.
- 16 research briefs + 17 surface specs.
- 1 outstanding v0 item — the DNS flip for the marketing site (decision,
  not code).

---

## Pre-0.1.0 history

The Phase 0 spike work (storage facade conformance, WOPI host, two-origin
binary, SPA shell) and the early Phase 1 walking-skeleton crates landed
before this changelog format was adopted. See `git log` for that history
and `docs/spikes/` for the spike write-ups.
