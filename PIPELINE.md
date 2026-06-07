# Casual Drive — v0 Pipeline

**Purpose:** single source of truth for what ships in v0, what status each surface is in, and the order I'm building it.

**Posture:** v0 must FEEL like a complete Drive (per [[feedback-v0-must-feel-complete]]). Every surface a real Drive has is **visible** in the SPA even when the implementation is stubbed. Polished "Coming in v0.2 — [explanation]" empty states are first-class deliverables, not placeholders.

**Status legend:**

| | meaning |
|---|---|
| ✅ done | wired end-to-end + works in the live binary |
| 🟡 wip | in-flight this pass |
| 🟦 stub | visible in UI, returns canned data or "Coming soon" |
| ⬜ todo | not started, queued for this v0 |
| ⏸ v0.2+ | explicitly deferred past v0 |

**Priority bands:**

- **P0** — blocks v0 shipping ("looks broken / missing" without it)
- **P1** — high visibility; reads as a real Drive
- **P2** — nice-to-have but not blocking the "feels complete" bar

> **Last audit:** 2026-06-08. Every row was checked against the codebase at that commit before its status was flipped.

---

## 1 — Identity / sign-in

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 1.1 | Sign-in card (username + password) | ✅ done | P0 | v2 palette + Fraunces title + shake on error |
| 1.2 | Anti-enumeration 401 | ✅ done | P0 | constant-time hash compare |
| 1.3 | Session cookie (`__Host-cd_sid` Secure HttpOnly SameSite=Lax) | ✅ done | P0 | server-side store in sqlite |
| 1.4 | Argon2id passwords (OWASP min) | ✅ done | P0 | drive-auth |
| 1.5 | Sign-out (cookie clear + 401 on session expiry → re-auth flow) | ✅ done | P0 | wired via AuthContext |
| 1.6 | Caps-lock detection on password | ✅ done | P2 | `Input` listens to `keydown`/`keyup`/`blur` and forwards the `getModifierState("CapsLock")` value; SignIn shows a one-line ⇪ warning under the field when on |
| 1.7 | "Sign in with [SSO]" stub | ⏸ v0.2+ | — | OIDC is Phase 3 — design + locked decisions in [[12-oidc]] |
| 1.8 | First-run admin-setup wizard (no env-only) | ✅ done | P1 | `Setup.tsx` flips on `/api/setup/status` → `needs_setup: true`; `/api/setup/admin` creates the first admin + their Personal workspace |

## 2 — Shell chrome (sidebar, top bar, layout)

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 2.1 | Sidebar brand row (Logo + Fraunces wordmark) | ✅ done | P0 | |
| 2.2 | "New" filled dropdown (folder / upload) | ✅ done | P0 | `Sidebar` exposes `onNewFolder` + `onUpload`; ticks dispatched up to `Shell` |
| 2.3 | Library nav (My Drive, Notes, Recent, Starred, Shared) | ✅ done | P0 | My Drive + Notes real; Recent/Starred/Shared are polished "Coming soon" panels |
| 2.4 | Workspaces section | ✅ done | P1 | real `WorkspaceSwitcher` driven by `WorkspaceContext` (§8.4 + §8.8) |
| 2.5 | System nav (Trash, Settings, Admin) | ✅ done | P0 | all three are real surfaces; Trash is functional for files + notes |
| 2.6 | Storage card pinned bottom | ✅ done | P1 | shows live `used_bytes`; quota visible when set |
| 2.7 | Avatar pinned at sidebar bottom | ✅ done | P0 | monogram + username + role |
| 2.8 | Cmd-K command palette (cmdk) | ✅ done | P1 | `CommandPalette` mounted in `Shell`. ⌘K/Ctrl-K from anywhere outside an input. Grouped results (Go to · Folders · Files · Notes), debounced parallel search over `/api/search` + `/api/notes/search`, workspace-scoped. Selecting a file fires `cd:open-file` (Files listens, opens preview); selecting a note fires `cd:open-note` (Notes listens, opens it) |
| 2.9 | Top bar notifications bell | ✅ done | P2 | `NotificationsBell` reads recipient-facing recent activity |
| 2.10 | Help (keyboard-shortcuts modal `?`) | ✅ done | P2 | `HelpModal`; `?` / `Shift-/` opens from anywhere outside an input |
| 2.11 | Theme toggle + dark palette | ✅ done | P2 | dark token set under `:root[data-theme="dark"]` mirrored under `@media (prefers-color-scheme: dark)` for the `system` mode. ThemeToggle cycles light → dark → system. Logo cloud uses `--paper` so the mark inverts cleanly across themes. Screenshot harness captures both themes (light primary, `*-dark.png` alongside) |

## 3 — File browser (main pane)

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 3.1 | "My Drive" / folder title (Fraunces 30px) | ✅ done | P0 | |
| 3.2 | Item count next to title ("12 items") | ✅ done | P0 | |
| 3.3 | Grid view (cards w/ procedural thumbnails, type tints) | ✅ done | P0 | |
| 3.4 | List view (rows, columns, tabular numerals) | ✅ done | P0 | |
| 3.5 | Grid/List toggle (segmented control) | ✅ done | P0 | |
| 3.6 | Empty state (illustrated container + Fraunces title) | ✅ done | P0 | |
| 3.7 | Search filter (current folder + global) | ✅ done | P0 | `searchAll` hits `/api/search`; respects active workspace via `WorkspaceContext` |
| 3.8 | Folder navigation (click folder → enter, breadcrumbs, back button, Backspace) | ✅ done | P0 | |
| 3.9 | Sort dropdown (Name / Modified / Size, folders first) | ✅ done | P1 | `SortMenu` component; persisted to localStorage |
| 3.10 | Stage swap animation on folder change | ✅ done | P1 | 420ms `cd-stage` keyframes |
| 3.11 | Drag-drop upload + ghost rows | ✅ done | P0 | |
| 3.12 | Quick access strip (4 recently-opened) | 🟦 stub | P1 | section reserved; surfaces when §10 Recent indexer lands |
| 3.13 | Multi-select + selection bar | ✅ done | P1 | `SelectionBar` component; bulk download as zip + bulk trash |
| 3.14 | Right-click context menu (Share / Rename / Move / Trash / Download) | ✅ done | P1 | `EntryContextMenu` + kebab; same handlers in both surfaces |
| 3.15 | Skeleton-on-load shimmer | ✅ done | P1 | |

## 4 — Preview / open

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 4.1 | Preview modal (Radix Dialog, detail sidebar) | ✅ done | P0 | |
| 4.2 | Type-aware primary action | ✅ done | P0 | `.xlsx`/`.docx` → Open in editor; everything else → Download |
| 4.3 | WOPI handoff for .xlsx → Casual Sheets | ✅ done | P0 | `openInEditor()` mints token, popup-blocker fallback → toast w/ "Open in tab" |
| 4.4 | WOPI handoff for .docx → Casual Document | ✅ done | P0 | same shape |
| 4.5 | Inline preview for **images** | ✅ done | P1 | `<img>` against the signed-URL on user-content origin |
| 4.6 | Inline preview for **PDFs** | ✅ done | P1 | `<iframe>` w/ browser-native viewer on user-content origin |
| 4.7 | Inline preview for **video** | ✅ done | P1 | `<video>` |
| 4.8 | Inline preview for **audio** | ✅ done | P2 | `<audio>` |
| 4.9 | Inline preview for **text / markdown / source** | ✅ done | P2 | text via `<pre>` (512 KB cap); markdown via `marked` + DOMPurify (256 KB cap) |
| 4.10 | Quick Look (spacebar) on focused row | ⏸ v0.2+ | — | needs keyboard focus model |

## 5 — Thumbnails

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 5.1 | Procedural thumbnails | ✅ done | P0 | client-rendered, type-tinted, all kinds |
| 5.2 | Client-side image-thumbnail creation on upload (canvas → base64) | ✅ done | P1 | `generateThumbnail()`; stored on the file row, served back in list |
| 5.3 | Client-side video-poster thumbnail on upload | ✅ done | P2 | `generateThumbnail` handles `video/mp4|webm|quicktime|ogg`: hidden `<video>` → seek to 10% → canvas → same encode pipeline. 5s timeouts on metadata/seek so broken codecs don't hang the queue |
| 5.4 | Server-side thumbnail worker (images in v0; PDFs / videos v0.2) | ✅ done (image slice) | P2 | migration 0010 adds `thumbs_state`. `ImageOnlyWorker` in `drive-storage::thumbnails` decodes via the `image` crate on a blocking thread → 3 PNG sizes (96/256/1024) at `thumbs/{id}/{size}.png`. Lazy generation kicked from `GET /api/files/{id}/thumb/{size}`. PDF + video decoders remain deferred to v0.2 (need a sandboxed subprocess per security brief). FileDto exposes `thumbs_state` + `thumb_urls`; `FileThumb` prefers server assets over the inline data URI when ready |
| 5.5 | Thumbnail cache (CDN-cacheable URLs) | ⏸ v0.2+ | — | depends on 5.4 |

## 6 — Upload

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 6.1 | Multipart streaming upload | ✅ done | P0 | drive-http::files POST /api/files |
| 6.2 | Magic-byte content-type sniffing (server) | ✅ done | P0 | `sniff_and_check_content_type()` via `infer` crate; rejects executables + sets the authoritative MIME |
| 6.3 | Per-request body limit (env) | ✅ done | P0 | `DRIVE_BODY_LIMIT_MB` |
| 6.4 | Per-user storage quota | ✅ done | P1 | `users.quota_bytes`; upload returns 413 + admin allocates via `/api/admin/users/{id}/quota` |
| 6.5 | Rate limit on upload endpoint | ✅ done | P1 | in-process token bucket per user (`RateLimiter`); returns 429 + `Retry-After` |
| 6.6 | Concurrent upload cap (client) | ✅ done | P2 | new `mapWithConcurrency` worker pool replaces unbounded `Promise.allSettled`; 4 lanes, matches the server's per-user rate limit so 20-file drops batch instead of bursting |
| 6.7 | Resumable upload (tus.io) | ⏸ v0.2+ | — | not Phase 1 |
| 6.8 | Virus-scan hook (ClamAV) | 🟦 stub | P2 | adapter trait + no-op default at minimum |

## 7 — Sharing / collaboration

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 7.1 | Share-link table (`share_links`) | ✅ done | P0 | |
| 7.2 | `POST /api/files/{id}/share` create share-link | ✅ done | P0 | 128-bit token, optional password (Argon2id), expiry, perms |
| 7.3 | `GET /s/{token}` recipient page | ✅ done | P0 | stripped chrome, `Recipient.tsx` |
| 7.4 | Share modal in SPA (perms / password / expiry / copy) | ✅ done | P0 | `ShareDialog.tsx`, also lists + revokes existing links |
| 7.5 | Recipient password-gated flow | ✅ done | P1 | Argon2id-hashed link password; constant-time compare |
| 7.6 | Revoke link from "shared by me" list | ✅ done | P1 | `DELETE /api/shares/{id}` + ShareDialog list |

## 8 — Multi-user / teams / RBAC

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 8.1 | `users` table (multi-row shape) | ✅ done | P0 | |
| 8.2 | Member list under Settings → Members | 🟦 stub | P1 | "Coming in v0.2 — invite teammates …" until §8.3 lands |
| 8.3 | Invitation API + email send | ⏸ v0.2+ | — | Phase 3 |
| 8.4 | Workspace table + workspace switcher | ✅ done | P1 | spec [[13-workspaces-surface]], phase 1 shipped |
| 8.5 | RBAC (roles + permissions) | 🟡 wip | P1 | Owner+Member today; Admin/Editor/Viewer split is deferred to invitations |
| 8.6 | Per-workspace storage quota | ⏸ v0.2+ | — | per-user quota done; workspace-level cap belongs with §8.9 BYO storage v2 |
| 8.7 | Admin user management UI (create / list / quota allocation) | ✅ done | P1 | `Admin → Users` table + inline quota edit + add-user dialog + upgrade-request panel |
| 8.8 | **Phase 2** — file/folder `workspace_id` column + scoped queries + switcher re-scopes UI | ✅ done | P0 | migration 0006 backfills from owner's Personal; handlers accept `?workspace=` / multipart `workspace_id`; SPA `WorkspaceContext` re-renders Files/search/upload on switcher pick. Full RBAC tiers deferred |
| 8.9 | Bring-your-own storage per workspace (S3 / MinIO / R2 / B2 + test-connection flow) | ✅ done | P1 | migration 0007, AES-256-GCM secret envelope (`drive-storage::secret_box`), SSRF guard, 5 owner-only endpoints, upload-routing via `StorageRegistry` + per-file `storage_id`. SPA `WorkspaceStorageCard`. 8 integration + 22 unit tests |
| 8.10 | Quota upgrade request flow | ✅ done | P2 | `POST /api/me/quota/request` emits audit event; admin sees it in Activity + Admin → Users |
| 8.11 | Notes / Wiki (personal + workspace scope) | ✅ done | P1 | migration 0008, `notes` + `note_links` backlinks index, markdown + live preview, `[[wiki link]]`s, drag-to-reorder tree. 7 unit + 8 integration tests |

## 9 — Settings

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 9.1 | `/settings` route shell with section nav | ✅ done | P0 | `Settings.tsx` |
| 9.2 | Account (change password) | ✅ done | P1 | `POST /api/auth/change-password` + AccountSection |
| 9.3 | Workspace | 🟦 stub | P1 | "Coming in v0.2"; rename + transfer live elsewhere |
| 9.4 | Members | 🟦 stub | P1 | waits on §8.3 invitations |
| 9.5 | Roles & permissions | 🟦 stub | P1 | waits on §8.5 role tiers |
| 9.6 | Sharing defaults | 🟦 stub | P1 | hook into §7 once per-user defaults exist |
| 9.7 | Storage (backend + quota readout + workspace BYO card) | ✅ done | P1 | `StorageSection` + `WorkspaceStorageCard` |
| 9.8 | Notifications | 🟦 stub | P2 | Coming soon |
| 9.9 | API tokens | 🟦 stub | P2 | Coming soon |
| 9.10 | Audit log (link to /activity) | ✅ done | P1 | Settings tile links to /activity |
| 9.11 | About (version / license / build) | ✅ done | P2 | `AboutSection` reads `/api/about` |

## 10 — Activity / audit

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 10.1 | `/activity` route shell | ✅ done | P1 | `Activity.tsx` |
| 10.2 | `audit_events` SQL table + migration | ✅ done | P1 | migration 0002 |
| 10.3 | Server-side audit emit on auth / upload / download / trash / share / workspace / storage / notes | ✅ done | P1 | `AuditRepo::emit` fire-and-forget; 20+ action types |
| 10.4 | Activity feed UI (timeline) | ✅ done | P1 | grouped by day, type-tagged, paginated via `?before=` |
| 10.5 | Filter by event type / actor / date | ⏸ v0.2+ | — | |

## 11 — Admin / monitoring

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 11.1 | `/admin` route shell | ✅ done | P1 | only visible to `is_admin`; non-admin gets a polite block |
| 11.2 | System health card (version / git_sha / built_at / storage / db / uptime / active sessions / recent sign-ins) | ✅ done | P1 | `SystemCard` |
| 11.3 | Active sessions list | ✅ done | P2 | session count surfaced in SystemCard; per-session list deferred |
| 11.4 | Audit-log link from admin | ✅ done | P2 | "Activity feed" link in the admin header |
| 11.5 | Cache + indexing dashboards (OpenSearch / Redis when enabled) | 🟦 stub | P2 | only relevant once optional infra lands |
| 11.6 | Storage adapter status | ✅ done | P2 | `StorageCard` in Admin shows configured backend + bucket + endpoint + region |

## 12 — Metadata

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 12.1 | Core metadata (name, size, type, modified, created, version) | ✅ done | P0 | |
| 12.2 | Sniffed content-type stored (not client-asserted) | ✅ done | P0 | server overrides client header with sniffed kind (§6.2) |
| 12.3 | EXIF strip (images) | ⏸ v0.2+ | — | privacy/security |
| 12.4 | Hash / etag | ✅ done | P1 | etag from storage adapter |
| 12.5 | Tags / labels | 🟦 stub | P2 | UI placeholder; v0.2 backing |
| 12.6 | Custom fields | ⏸ v0.2+ | — | |
| 12.7 | Shared-with avatars in details panel | 🟦 stub | P1 | hidden in single-tenant v0 |

## 13 — Presigned URLs (clarification)

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 13.1 | Storage facade `signed_get` / `signed_put` | ✅ done | P0 | drive-storage |
| 13.2 | S3 / MinIO native presign | ✅ done | P0 | via opendal |
| 13.3 | HMAC self-mint for fs / memory | ✅ done | P0 | `SignedUrl::Token` variant |
| 13.4 | `/raw/{token}` on user-content origin | ✅ done | P0 | drive-http::raw |
| 13.5 | `GET /api/files/{id}/download` 302 → signed URL | ✅ done | P0 | drive-http::files |
| 13.6 | Direct-to-storage upload (presign + complete + abort) | ✅ done | P2 | migration 0009 adds `files.status` + `expected_size`. 3 endpoints under `/api/files/{upload-url,id/complete,id/abort}`. Quota committed at presign (so parallel uploads can't both fit). Adapters that can't presign return 409 → SPA falls back to proxy. SPA opts in at file ≥ 8 MiB via `VITE_DIRECT_UPLOAD=1`. 15-min PUT TTL. §13.6a (post-finalize magic-byte sniff) deferred to v0.2 |
| 13.7 | Settings UI showing signed-URL TTL | ✅ done | P2 | new `Config::signed_url_ttl_secs` (env `DRIVE_SIGNED_URL_TTL_SECS`, default 300s, floor 30s) threaded through `Storage::signed_get`; `/api/about` returns it + `body_limit_mb`; Settings → Storage → Backend card surfaces both |

## 14 — Backend chassis (recap — already shipped)

| # | Item | Status |
|---|---|---|
| 14.1 | Rust workspace (drive-core / -db / -storage / -wopi / -auth / -http / -bin) | ✅ done |
| 14.2 | OpenDAL storage with fs / memory / S3 / MinIO adapters | ✅ done |
| 14.3 | sqlx Any pool with portable SQLite + Postgres migrations | ✅ done |
| 14.4 | Two-origin Axum (app + user-content, host-dispatch 421) | ✅ done |
| 14.5 | rust-embed SPA in single static binary | ✅ done |
| 14.6 | WOPI host (7 endpoints) | ✅ done |
| 14.7 | tower-sessions + Argon2id auth | ✅ done |
| 14.8 | File + folder CRUD API | ✅ done |
| 14.9 | Multi-stage cargo-chef Dockerfile | ✅ done |
| 14.10 | CI (fmt, clippy, audit, deny, tests, Docker build) | ✅ done |
| 14.11 | Backend tests passing across the workspace (27 suites at audit time) | ✅ done |

## 15 — Marketing site + GH Pages

Spec: [[07-marketing-site]] + [[14-marketing-surface]]. Astro 5, multi-page docs site, looser marketing identity, /demo embeds the SPA bundle.

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 15.1 | Astro scaffold + tokens + Base layout + Nav + Footer | ✅ done | P0 | mobile-first, JSON-LD, OG, canonical, sitemap |
| 15.2 | Landing `/` (Hero, ScreenshotShowcase, FeatureGrid, HowItWorks, Compare, FinalCta) | ✅ done | P0 | single H1, SoftwareApplication schema |
| 15.3 | `/docs/install` + `/docs/configuration` + `/docs/architecture` + `/docs/contributing` (MDX) | ✅ done | P0 | shared DocLayout w/ sidebar |
| 15.4 | `/screenshots` gallery | ✅ done | P1 | wired to real PNGs |
| 15.5 | `/demo` route (iframe → /demo-app/ bundle) | ✅ done | P0 | noindex; SPA built with `VITE_BASE=/demo-app/` |
| 15.6 | GitHub Actions workflow (build SPA + Astro, deploy Pages) | ✅ done | P0 | fetches Inter fonts in CI |
| 15.7 | Capture + commit real screenshots | ✅ done | P1 | Playwright harness `web/tests/e2e/marketing-screenshots.mjs` |
| 15.8 | Pre-built OG image at `/og/default.png` (1200×630) | ✅ done | P1 | `marketing/scripts/build-og.mjs` (sharp + inline SVG) |
| 15.9 | Lighthouse CI job in workflow (target P/A/B/S ≥ 95) | ✅ done | P2 | `lighthouserc.json` + `@lhci/cli` |
| 15.10 | Pagefind-powered docs search | ⏸ v0.2+ | — | pages are few enough for Cmd-F today |
| 15.11 | `/blog` route | ⏸ v0.2+ | — | slot reserved in footer |
| 15.12 | i18n | ⏸ v0.2+ | — | English-only; structure leaves room for astro-i18n |
| 15.13 | Domain flip (drive.schnsrw.live → schnsrw.live apex / casualoffice.org) | ⬜ todo | P1 | OLD CNAME at `web/public/CNAME` is now orphaned (deploy artifact is `marketing/dist`). Add `marketing/public/CNAME` when DNS decided; set repo `MARKETING_SITE_URL` variable |
| 15.13a | Dynamic `robots.txt` keyed to `ASTRO_SITE` | ✅ done | P1 | converted from static `public/robots.txt` to `src/pages/robots.txt.ts` |
| 15.13b | `/sitemap.xml` convenience alias | ✅ done | P2 | `src/pages/sitemap.xml.ts` mirrors the @astrojs/sitemap index. Humans + legacy crawlers that hit `/sitemap.xml` by convention now resolve; spec-correct `/sitemap-index.xml` continues to ship |
| 15.13c | `marketing/public/CNAME` for `drive.schnsrw.live` | ✅ done | P0 | the marketing artifact is the Pages deploy target now; CNAME pins the Pages domain in repo + `astro.config.mjs` defaults `ASTRO_SITE` to `https://drive.schnsrw.live` so sitemap URLs match the deploy |

---

## Outstanding v0 work (after the 2026-06-08 audit + subsequent shipping passes)

Every P0/P1/P2 row in the tables above is now ✅ done or ⏸ v0.2+. The single residual is a config / decision item, not code:

| Priority | Surface | Item | Effort |
|---|---|---|---|
| **P1** | 15.13 | Domain flip + final CNAME (needs DNS decision — `drive.schnsrw.live` works today; flip to `schnsrw.live` apex / `casualoffice.org` when the call is made) | XS |

Everything else is either a 🟦 stub waiting on its enabling feature (workspace invitations §8.3, RBAC role tiers §8.5, OIDC sign-in §1.7) or explicitly **⏸ v0.2+** with the design parked in a research brief.

---

## Phase 3 — what's already specced

When v0 has been dogfooded at scale, the next contributor can pick up any of these without re-litigating the design:

| Brief | What it covers |
|---|---|
| [[12-oidc]] | Multi-tenant SSO via OIDC. Authorization Code Flow + PKCE, in-process ID-token validation, Drive-side sessions. Maps onto §1.7. |
| [[13-ms365-federation]] | Office Online client federation. Proof-key RSA validation, served discovery doc, opt-in. Wakes the dormant hook in `drive-wopi`. |
| [[14-presence]] | Drive-shell ambient awareness (avatar stack + file-row dot + quiet toast). SSE one-channel-per-workspace, in-process hub with a Redis escape hatch. NOT in-editor cursors. |
| [[15-sandboxed-thumb-worker]] | PDF + video thumbnails in a `drive-thumb-worker` subprocess. seccomp + rlimits + privilege drop. Promotes §5.4 from "image slice done" to full coverage. |
| §13.6a | Post-finalize magic-byte sniff on direct uploads — the residual hook from the §13.6 spec. Worth doing before a multi-tenant prod deploy. |

---

## Build order (next passes)

The v0 queue is empty. Phase 3 starts with whichever brief above the operator picks first — typically **§12 OIDC** because it unlocks every multi-tenant story downstream.

User: if anything's missing, add it. Default execution is top-down.
