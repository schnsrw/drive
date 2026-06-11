# Changelog

All notable changes to Casual Drive land here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Notes editor — clickable wiki-links (NT1 Phase 2).** The `+` picker now inserts a real Tiptap Link mark with `href="cd-note://<id>"` instead of plain `[[Title]]` text. MarkdownEditor's `handleClick` editor-prop catches clicks on `cd-note:` anchors, prevents the browser from following the bogus URL, and dispatches `cd:open-note` so the SPA routes to the right note in-app. The `cd-note` scheme is registered with Link's `protocols` whitelist so it parses as a link instead of escaping back to text. Markdown round-trips as `[Title](cd-note://id)`; older notes that stored `[[Title]]` as plain markdown text still render as text and upgrade lazily on edit.
- **Notes editor — drag-to-reorder blocks (NT5 Phase 2).** Grab a block by its hover handle, drag it anywhere in the document, drop. ProseMirror's built-in Dropcursor (already wired via StarterKit) draws the insertion indicator; the slice is handed off with `move:true` so the source is removed atomically on drop. Falls back to the click menu's Move up / Move down for keyboard-only users (drag stays mouse-only by design — Phase 2 of the spec covers a screen-reader path).
- **Notes editor — block "Turn into" menu (NT5 closure).** The block-handle menu now exposes eight conversion targets: Paragraph / Heading 1-3 / Bullet list / Numbered list / Quote / Code block. Each conversion chains `setParagraph()` first to clear the source node type, then the target's set/toggle — so cross-family conversions (heading → list, list → quote) all work without the user manually flattening the block first. NT5 row closed.
- **Notes editor — link dialog (NT2 Phase 2).** ⌘K / Ctrl-K opens a Radix dialog with one URL field; auto-prepends `https://` for bare hosts, rejects `javascript:` / `data:` / `vbscript:` / `file:` case-insensitively, mirrors selection-wrapping behaviour from Notion / Obsidian. Bubble toolbar gets a Link button; mobile sticky toolbar gains one too.
- **Result density toggle (SR4).** Comfortable / Compact selector in the top toolbar, persisted per-user via `localStorage`. Driven by CSS custom properties scoped on `[data-density="compact"]` so card thumb height, card meta padding, list-row padding and the list-row thumbnail all shrink together. Doesn't change page size.
- **Search URL state (SR6).** Query + filters + sort + scope serialize into URL search params (`?q=…&t=pdf,image&sort=name&dir=asc`). Bookmarkable, reload-safe, back / forward replays the search. Writes via `history.replaceState` so the back stack isn't polluted per keystroke; default state writes nothing so a clean Drive view stays on a clean URL. Files owns the write; Shell hydrates `q` on mount and listens for a `cd:search-query` event the popstate handler emits.
- **Recent-searches dropdown (SR11).** Per-user `localStorage` under `cd-search-history-v1`, capped at 10 distinct (query + filter-fingerprint) pairs, newest-first. Pops under the search input on focus; click a row to re-apply the query AND the filter set the user had active when they ran it. Keyboard nav (↑/↓/Enter/Esc). Records on Enter or blur with a non-empty query. Clear-history button at the bottom. When SR10 ships, server suggestions slot into the same popover.

### Fixed

- **Note search-hit deep-links (SR7 remnant).** Note results in search now carry a kebab → "Copy link" that writes `${origin}/?note=<id>` to the clipboard. Shell hydrates that param on mount: routes to the Notes tab and dispatches `cd:open-note` once the lazy Notes chunk has had time to attach its listener. Same dropdown surface the file kebab uses, so rename / trash slot in cleanly when the Notes-tab actions surface lands.

- **Refresh stayed in search mode (SR7).** A rename / trash / share / bulk-trash inside an active search used to swap the result pane back to the current folder listing while the query was still in the input — looked like the search had silently bailed. `refresh()` now bumps a tick the search effect listens for when called in search mode, re-running the search instead of clobbering it.

- **Notes editor — "Link to note" slash item (NT3 Phase 2).** The slash menu (`/`) gets a "Link to note" entry that hands off to the existing `+` note-link picker, so users have keyboard-symmetric paths to insert a wiki-link. The picker still inserts a real Tiptap Link mark with `href="cd-note://<id>"` (NT1 Phase 2) — markdown round-trip + in-app click navigation unchanged.
- **Marketing site — mobile-sized screenshot variants (MK-PERF-95).** `optimize-screenshots.mjs` now also emits an `@800w.avif` + `@800w.webp` for every PNG. The `<picture>` in `ScreenshotShowcase.astro` carries two extra `<source>` entries gated on `media="(max-width: 768px)"` so phones serve the small variant before the desktop one's even considered. On the LCP screenshot (`files-list`) that's 38 KB AVIF → 10 KB AVIF (3.8× smaller). Desktop is unchanged.
- **Presence backend Phase 1b — SSE stream (RT1).** `GET /api/presence/{ws}` is now a `text/event-stream` that sends an initial `Present` burst (one event per currently-active user) followed by a live feed of `Present` / `Left` events as users beat, leave, or expire. `PresenceHub` refactored to carry a `tokio::sync::broadcast::Sender` per workspace alongside the entries map — `beat()` / `leave()` / `sweep_expired()` all publish to the bus. Keepalive ticks every 25 s (well under the typical 60 s reverse-proxy timeout). Membership-gated like the other two endpoints. Subscribe-before-snapshot ordering means a `beat` landing in the race window produces at-worst a duplicate `Present` (which the SPA reducer dedups on) rather than a missed announcement. 10/10 unit tests cover beat / leave / snapshot / sweep / cross-workspace isolation / subscriber receives Present / subscriber receives Left / sweep publishes Left / cross-workspace subscribers don't see each other / tint determinism. Remaining: audit-event broadcast (1c) + per-user stream cap + heartbeat rate limit (1d).
- **Presence backend Phase 1a (RT1).** Real-time presence is starting to ship. `crates/drive-http/src/presence.rs` ships the in-process `PresenceHub` (per-workspace `HashMap<user_id, PresenceEntry>` behind an `RwLock`), a 60 s TTL sweep task that ticks every 5 s, and two endpoints — `POST /api/presence/{ws}/beat` (heartbeat, optional `{viewing: file_id}` body) and `POST /api/presence/{ws}/leave` (explicit goodbye). Both are membership-gated via `WorkspaceMemberRepo::role_of`; non-members get 403. Deterministic per-user avatar tint via FNV-hash → 8-colour palette. Hub spans into `HttpState`, the sweep is `spawn_sweep()`'d at process boot. 6 unit tests cover beat / leave / snapshot / sweep / cross-workspace isolation / tint determinism. SSE stream + audit-event broadcast follow in Phases 1b / 1c.
- **Notes — fixed sticky-top formatting toolbar.** Notion-style: a persistent toolbar pinned to the top of the editor pane (sticky during scroll) hosts the same Bold / Italic / Strike / Code / Link / H1-H3 / Bullet / Numbered / Quote actions the bubble menu has carried since NT2. Bubble menu still appears on text selection — discoverability for first-time users, proximity-to-cursor for power users. The button row was extracted into a shared `ToolbarRow` component so both surfaces stay in lock-step. Desktop only (≥1024 px + hover); mobile keeps NT6's bottom toolbar.

### Fixed

- **Notes — title / body edits no longer skipped during autosave round-trip.** The autosave handler used to `setOpen(serverResponse)` after every save, which blindly clobbered local title + body with whatever the server returned — keystrokes typed during the network turn vanished as the controlled input + Tiptap editor re-synced from props. The handler now keeps `prev.title` + `prev.body` from local state and only takes the server-owned fields (timestamps, parent_id, version), so mid-flight edits survive and the next debounced save picks them up.

### Performance

- **Search debounce 200 ms → 50 ms (SR15).** The spec budgets p95 keystroke→paint at 200 ms but the existing debounce ate the entire budget before fetch + paint started. Drops to 50 ms (Notion / Linear style). Local p95 fell from 417 ms → 207 ms — essentially on the spec target. AbortController on every keystroke already coalesces in-flight requests, so a tighter debounce produces more cancels, not more wasted server work. Playwright ceiling tightened from 800 ms → 500 ms.

- **Search latency instrumentation (SR15, first pass).** New `lib/searchMetrics.ts` opens a `performance.mark` window on the first keystroke after a paint, closes it in a double-rAF after the search effect's `setState({ ready })` so the timestamp lines up with a composited frame. PerformanceObserver folds each `performance.measure` into a rolling 100-sample buffer; `window.__cd_search_perf()` returns `{ count, p50_ms, p95_ms, max_ms }` for Playwright + manual DevTools probes. CI assertion against the spec's p95 < 200 ms budget follows in a later pass once we've seen a few runs of baseline numbers.

### Changed

- **Search a11y polish (SR14).** Search input wired as `role="combobox" aria-autocomplete="list" aria-controls aria-expanded`; the recents popover gets a stable listbox id and per-row option ids, mirrored back to the input via `aria-activedescendant` so screen readers announce the highlighted entry as the user arrows. Search-region wrapper carries `role="search"`. SortMenu trigger now exposes the live sort key + direction in its `aria-label` (was just "Sort"); items switched to `DropdownMenu.RadioGroup` / `RadioItem` so each option announces as `menuitemradio` with `aria-checked`. Decorative icons across the search surface marked `aria-hidden`. Existing `aria-live="polite"` on the count chip + infinite-scroll loader and Radix Popover's built-in focus-trap on chip popovers already cover the remaining spec bullets. SR14 closed.
- OIDC sign-in (Authorization Code + PKCE). `DRIVE_ALLOW_PASSWORD_AUTH=false` hides the password form.
- Sandboxed `drive-thumb-worker` subprocess for video thumbnails (ffmpeg-CLI), with per-job rlimits and optional `setuid`. PDF support deferred.
- **SDK integration (Phase 1)** — `@schnsrw/docx-js-editor@1.0.0` + `@schnsrw/casual-sheets@0.3.0` mounted inline in the Preview modal. `.docx` opens the real editor in-Drive; `.xlsx` falls back to the existing WOPI new-tab handoff until Phase 1.5 ships an xlsx → IWorkbookData converter. See [`docs/ux/10-sdk-integration-plan.md`](./docs/ux/10-sdk-integration-plan.md).
  - New endpoints `GET / PUT /api/files/{id}/content` carry bytes through the same-origin authenticated session — no token mint, no user-content origin redirect. The WOPI handoff stays around for the third-party launch.
  - `DriveFileSource` implements the SDK's `FileSource` interface; `CasualDocEditor` wires the editor with the signed-in admin's identity. Co-edit defaults off — operators set `VITE_DRIVE_COLLAB_BACKEND_URL=wss://...` to enable.
  - `AutosaveStatus` indicator surfaces in the Preview chrome (Google-Docs-style "Saving… / Saved 2m ago").
  - Bundle splits: vendor-docx-editor (2.5 MB) lazy-loads only when a `.docx` is opened; cold load stays at ~677 KB.

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
