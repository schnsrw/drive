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

---

## 1 — Identity / sign-in

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 1.1 | Sign-in card (username + password) | ✅ done | P0 | v2 palette + Fraunces title + shake on error |
| 1.2 | Anti-enumeration 401 | ✅ done | P0 | constant-time hash compare |
| 1.3 | Session cookie (`__Host-cd_sid` Secure HttpOnly SameSite=Lax) | ✅ done | P0 | server-side store in sqlite |
| 1.4 | Argon2id passwords (OWASP min) | ✅ done | P0 | drive-auth |
| 1.5 | Sign-out (cookie clear + 401 on session expiry → re-auth flow) | ✅ done | P0 | wired via AuthContext |
| 1.6 | Caps-lock detection on password | ⬜ todo | P2 | small, easy |
| 1.7 | "Sign in with [SSO]" stub | ⏸ v0.2+ | — | OIDC is Phase 3 |
| 1.8 | First-run admin-setup wizard (no env-only) | ⬜ todo | P1 | if no admin exists in DB, show setup flow instead of sign-in |

## 2 — Shell chrome (sidebar, top bar, layout)

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 2.1 | Sidebar brand row (Logo + Fraunces wordmark) | ✅ done | P0 | |
| 2.2 | "New" filled dropdown (folder / upload) | 🟡 wip | P0 | button exists, dropdown menu wiring pending |
| 2.3 | Library nav (My Drive, Recent, Starred, Shared) | 🟦 stub | P1 | My Drive real; rest "Coming soon" pages |
| 2.4 | Workspaces section | 🟦 stub | P1 | placeholder; v0.2 multi-tenant |
| 2.5 | System nav (Trash, Settings, Admin) | 🟦 stub | P0 | Trash real (Phase 2); Settings + Admin shells |
| 2.6 | Storage card pinned bottom | ✅ done | P1 | shows used; quota optional |
| 2.7 | Avatar pinned at sidebar bottom | ✅ done | P0 | monogram + username + role |
| 2.8 | Top bar search (cmdk command palette) | 🟡 wip | P1 | input present, palette not wired |
| 2.9 | Top bar notifications bell | ⬜ todo | P2 | badge + dropdown stub |
| 2.10 | Help (keyboard-shortcuts modal `?`) | ⬜ todo | P2 | Phase 2 cheat-sheet |
| 2.11 | Theme toggle | ⬜ todo | P2 | dark mode tokens TBD |

## 3 — File browser (main pane)

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 3.1 | "My Drive" / folder title (Fraunces 30px) | ✅ done | P0 | |
| 3.2 | Item count next to title ("12 items") | ✅ done | P0 | |
| 3.3 | Grid view (cards w/ procedural thumbnails, type tints) | ✅ done | P0 | |
| 3.4 | List view (rows, columns, tabular numerals) | ✅ done | P0 | |
| 3.5 | Grid/List toggle (segmented control) | ✅ done | P0 | |
| 3.6 | Empty state (illustrated container + Fraunces title) | ✅ done | P0 | |
| 3.7 | Search filter (current folder + global) | 🟡 wip | P0 | input wired; global recursive search v0.2 |
| 3.8 | Folder navigation (click folder → enter, breadcrumbs, back button, Backspace) | ✅ done | P0 | shipped this pass |
| 3.9 | Sort dropdown (Name / Modified / Size, folders first) | ⬜ todo | P1 | mockup-v2 has it |
| 3.10 | Stage swap animation on folder change | ✅ done | P1 | 420ms `cd-stage` keyframes |
| 3.11 | Drag-drop upload + ghost rows | ✅ done | P0 | |
| 3.12 | Quick access strip (4 recently-opened) | 🟦 stub | P1 | section reserved, hidden until recents wired |
| 3.13 | Multi-select + selection bar | ⬜ todo | P1 | per surface §8 |
| 3.14 | Right-click context menu (Share / Rename / Move / Trash / Download) | ⬜ todo | P1 | |
| 3.15 | Skeleton-on-load shimmer | ✅ done | P1 | basic shimmer |

## 4 — Preview / open

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 4.1 | Preview modal (Radix Dialog, detail sidebar) | ✅ done | P0 | placeholder stage; details pane real |
| 4.2 | Type-aware primary action | 🟡 wip | P0 | labels right; "Open in Sheets/Editor" → currently still downloads |
| 4.3 | WOPI handoff for .xlsx → Casual Sheets | ⬜ todo | P0 | `/api/files/{id}/open` exists server-side; SPA wiring + popup blocker fallback |
| 4.4 | WOPI handoff for .docx → Casual Editor | ⬜ todo | P0 | same shape |
| 4.5 | Inline preview for **images** | ⬜ todo | P1 | render the actual bytes (signed URL) on user-content origin |
| 4.6 | Inline preview for **PDFs** | ⬜ todo | P1 | PDF.js sandboxed in iframe on user-content origin |
| 4.7 | Inline preview for **video** | ⬜ todo | P1 | `<video>` tag, signed URL |
| 4.8 | Inline preview for **audio** | ⬜ todo | P2 | `<audio>` tag |
| 4.9 | Inline preview for **text / markdown / source** | ⬜ todo | P2 | monospaced text viewer |
| 4.10 | Quick Look (spacebar) on focused row | ⏸ v0.2+ | — | needs keyboard focus model |

## 5 — Thumbnails

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 5.1 | Procedural thumbnails (sheet grid / doc page / PDF redbar / image gradient / video play / folder glyph) | ✅ done | P0 | client-rendered, type-tinted |
| 5.2 | Client-side image-thumbnail creation on upload (canvas → 200×200 base64) | ⬜ todo | P1 | stored as file metadata, served back in list |
| 5.3 | Client-side video-poster thumbnail on upload | ⬜ todo | P2 | first-frame capture |
| 5.4 | Server-side thumbnail worker (sandboxed; images / PDFs / videos) | ⏸ v0.2+ | — | needs sandboxed image worker per security brief |
| 5.5 | Thumbnail cache (CDN-cacheable URLs) | ⏸ v0.2+ | — | depends on 5.4 |

## 6 — Upload

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 6.1 | Multipart streaming upload | ✅ done | P0 | drive-http::files POST /api/files |
| 6.2 | Magic-byte content-type sniffing (server) | ⬜ todo | P0 | `infer` crate, per security brief |
| 6.3 | Per-request body limit (env) | ✅ done | P0 | `DRIVE_BODY_LIMIT_MB` |
| 6.4 | Per-user storage quota | ⬜ todo | P1 | needs `quota_bytes` on users |
| 6.5 | Rate limit on upload endpoint | ⬜ todo | P1 | `tower-governor` |
| 6.6 | Concurrent upload cap (client) | ⬜ todo | P2 | 4-at-a-time queue |
| 6.7 | Resumable upload (tus.io) | ⏸ v0.2+ | — | not Phase 1 |
| 6.8 | Virus-scan hook (ClamAV) | 🟦 stub | P2 | adapter trait + no-op default at minimum |

## 7 — Sharing / collaboration

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 7.1 | Share-link table (`share_links`) | ✅ done | P0 | migration shipped, no API yet |
| 7.2 | `POST /api/files/{id}/share` create share-link | ⬜ todo | P0 | 128-bit token, optional password, expiry, perms |
| 7.3 | `GET /s/{token}` recipient page | ⬜ todo | P0 | stripped chrome per surface spec |
| 7.4 | Share modal in SPA (perms / password / expiry / copy) | ⬜ todo | P0 | per mockup-v2 |
| 7.5 | Recipient password-gated flow | ⬜ todo | P1 | argon2id-hashed link password |
| 7.6 | Revoke link from "shared by me" list | ⬜ todo | P1 | settings → Sharing section |

## 8 — Multi-user / teams / RBAC

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 8.1 | `users` table (multi-row shape) | ✅ done | P0 | schema is multi-user already |
| 8.2 | Member list under Settings → Members | 🟦 stub | P1 | "Coming in v0.2 — invite teammates …" |
| 8.3 | Invitation API + email send | ⏸ v0.2+ | — | Phase 3 |
| 8.4 | Workspace table + workspace switcher | ✅ done | P1 | spec [[13-workspaces-surface]], phase 1 shipped — phase 2 below |
| 8.5 | RBAC (roles + permissions) | 🟡 wip | P1 | Owner+Member today; Admin/Editor/Viewer split + per-action checks in §8.8 |
| 8.6 | Per-workspace storage quota | ⏸ v0.2+ | — | per-user quota done; workspace-level cap rolls in with phase 2 |
| 8.7 | Admin user management UI (create / list / quota allocation) | 🟡 wip | P1 | backend done; Admin → Users table + inline quota edit UI pending |
| 8.8 | **Phase 2** — file/folder `workspace_id` column + scoped queries + switcher re-scopes UI | ✅ done | P0 | migration 0006 backfills from owner's Personal; `list_children_in_workspace` + `workspace_used_bytes`; handlers accept `?workspace=` / multipart `workspace_id` / body `workspace_id`; SPA `WorkspaceContext` re-renders Files/search/upload on switcher pick. Full RBAC role-tiers (Admin/Editor/Viewer) deferred — Owner+Member membership gate is enforced now |
| 8.9 | Bring-your-own storage per workspace (S3 / MinIO / R2 / B2 + test-connection flow) | ✅ done | P1 | migration 0007, AES-256-GCM secret envelope with per-row AAD bound to `(id, key_version)`, SSRF guard (metadata-IP block list + private-range gating), 5 owner-only endpoints, upload-routing via StorageRegistry + per-file `storage_id` pointer (existing files keep their bucket when workspace flips), SPA `WorkspaceStorageCard` with configure / test / replace / remove. 8 integration tests + 22 storage unit tests green |
| 8.10 | Quota upgrade request flow | ✅ done | P2 | `POST /api/me/quota/request` emits audit event; admin sees in Activity |
| 8.11 | Notes / Wiki (personal + workspace scope) | ✅ done | P1 | dedicated `notes` table + `note_links` backlinks index (migration 0008). Markdown source + live preview (textarea + `marked` + `dompurify` — no Lexical, no new deps). `[[Wiki link]]` autocomplete, drag-to-reorder tree, search across title+body, trash/restore. 7 unit + 8 integration tests cover link parsing, wiki-link indexing, dangling-link reresolve, cross-workspace 403, body-cap 413, cycle prevention. Sidebar `Notes` entry, demo-mode shim ships with the SPA |

## 9 — Settings

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 9.1 | `/settings` route shell with section nav | ⬜ todo | P0 | needed to anchor all stubs |
| 9.2 | Account (change password) | ⬜ todo | P1 | real for admin |
| 9.3 | Workspace | 🟦 stub | P1 | Coming soon |
| 9.4 | Members | 🟦 stub | P1 | Coming soon |
| 9.5 | Roles & permissions | 🟦 stub | P1 | Coming soon |
| 9.6 | Sharing defaults | 🟦 stub | P1 | hook into 7.* once built |
| 9.7 | Storage (backend + quota readout) | ⬜ todo | P1 | reads from `/api/me` |
| 9.8 | Notifications | 🟦 stub | P2 | Coming soon |
| 9.9 | API tokens | 🟦 stub | P2 | Coming soon |
| 9.10 | Audit log (link to /activity) | 🟦 stub | P1 | Coming soon |
| 9.11 | About (version / license / build) | ⬜ todo | P2 | reads from build env |

## 10 — Activity / audit

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 10.1 | `/activity` route shell | ⬜ todo | P1 | feed of events |
| 10.2 | `audit_log` SQL table + migration | ⬜ todo | P1 | event types, actor, target, timestamp |
| 10.3 | Server-side audit emit on auth / upload / download / trash / share | ⬜ todo | P1 | tower middleware |
| 10.4 | Activity feed UI (timeline) | ⬜ todo | P1 | grouped by day, type-tagged |
| 10.5 | Filter by event type / actor / date | ⏸ v0.2+ | — | |

## 11 — Admin / monitoring

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 11.1 | `/admin` route shell | ⬜ todo | P1 | only visible to is_admin users |
| 11.2 | System health card (storage backend, db, uptime) | ⬜ todo | P1 | |
| 11.3 | Active sessions list | ⬜ todo | P2 | tied to sessions table |
| 11.4 | Audit-log link from admin | ⬜ todo | P2 | |
| 11.5 | Cache + indexing dashboards (OpenSearch / Redis when enabled) | 🟦 stub | P2 | Coming soon |
| 11.6 | Storage adapter status | ⬜ todo | P2 | shows configured backend |

## 12 — Metadata

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 12.1 | Core metadata (name, size, type, modified, created, version) | ✅ done | P0 | files table |
| 12.2 | Sniffed content-type stored (not client-asserted) | ⬜ todo | P0 | tied to 6.2 |
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
| 13.3 | HMAC self-mint for fs / memory | ✅ done | P0 | drive-storage::Token variant |
| 13.4 | `/raw/{token}` on user-content origin | ✅ done | P0 | drive-http::raw |
| 13.5 | `GET /api/files/{id}/download` 302 → signed URL | ✅ done | P0 | drive-http::files |
| 13.6 | `POST /api/files/{id}/upload-url` (direct-to-S3 client upload) | ⬜ todo | P2 | bypasses Drive for large uploads |
| 13.7 | Settings UI showing signed-URL TTL | 🟦 stub | P2 | Coming soon |

## 15 — Marketing site + GH Pages

Spec: [[07-marketing-site]] + [[14-marketing-surface]]. Astro 5, multi-page docs site, looser marketing identity, /demo embeds the SPA bundle.

| # | Item | Status | Priority | Notes |
|---|---|---|---|---|
| 15.1 | Astro scaffold + tokens + Base layout + Nav + Footer | ✅ done | P0 | mobile-first, dark/light, JSON-LD, OG, canonical, sitemap |
| 15.2 | Landing `/` (Hero, ScreenshotShowcase, FeatureGrid, HowItWorks, Compare, FinalCta) | ✅ done | P0 | single H1, SoftwareApplication schema |
| 15.3 | `/docs/install` + `/docs/configuration` + `/docs/architecture` + `/docs/contributing` (MDX) | ✅ done | P0 | shared DocLayout w/ sidebar |
| 15.4 | `/screenshots` gallery | ✅ done | P1 | placeholder tiles until real shots drop into public/screenshots/ |
| 15.5 | `/demo` route (iframe → /demo-app/ bundle) | ✅ done | P0 | noindex; SPA built with `VITE_BASE=/demo-app/` |
| 15.6 | GitHub Actions workflow (build SPA + Astro, deploy Pages) | ✅ done | P0 | replaces prior SPA-only deploy; fetches Inter fonts in CI |
| 15.7 | Capture + commit real screenshots | ✅ done | P1 | Playwright harness `web/tests/e2e/marketing-screenshots.mjs` (also exposed as `pnpm screenshots` from web/) drives the demo SPA across Files / Notes / Sharing / Settings / Admin / Activity, light theme + mobile viewport. 7 PNGs in `marketing/public/screenshots/`; gallery + landing showcase wired to them |
| 15.8 | Pre-built OG image at `/og/default.png` (1200×630) | ⬜ todo | P1 | links currently fall back to text card |
| 15.9 | Lighthouse CI job in workflow (target P/A/B/S ≥ 95) | ⬜ todo | P2 | enforces the perf budget in CR |
| 15.10 | Pagefind-powered docs search | ⏸ v0.2+ | — | pages are few enough for Cmd-F today |
| 15.11 | `/blog` route | ⏸ v0.2+ | — | slot reserved in footer |
| 15.12 | i18n | ⏸ v0.2+ | — | English-only; structure leaves room for astro-i18n |
| 15.13 | Domain flip (drive.schnsrw.live → schnsrw.live apex / casualoffice.org) | ⬜ todo | P1 | OLD CNAME at web/public/CNAME is now orphaned (deploy artifact is marketing/dist). Add marketing/public/CNAME when DNS decided; set repo `MARKETING_SITE_URL` variable. |

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
| 14.11 | 46 backend tests passing + 32 spike conformance | ✅ done |

---

## Build order (next passes)

These are the next units of work in priority. I drive top-down without asking.

1. **Reorganize sidebar + add stub routes** — Recent, Starred, Shared, Workspace, Settings, Admin, Activity. Each routes to a "Coming in v0.2" page with planned design preview. Quick visual gain.
2. **Settings shell** — sectioned page (Account, Workspace, Members, Roles, Sharing, Storage, Notifications, API tokens, Audit log, About). Real for Storage + About; stubs for the rest.
3. **First-run admin setup wizard** — if `users` table empty, show wizard instead of sign-in. Username + password + workspace name.
4. **Share-link backend + modal** — `POST /api/files/{id}/share`, `GET /s/{token}`, modal UI per mockup-v2.
5. **Activity / audit log** — `audit_log` table + middleware emit + `/activity` feed.
6. **Real preview for images** — `<img>` tag with signed-URL.
7. **Real preview for PDFs** — PDF.js sandboxed in iframe on user-content origin.
8. **Client-side image thumbnail on upload** — canvas → base64, stored as metadata.
9. **WOPI handoff wiring** for `.xlsx` / `.docx` (open in sibling editors).
10. **Sort dropdown** + multi-select + context menus.
11. **Notifications bell + help shortcut modal**.
12. **Admin dashboard** with system-health card.

User: if anything's missing, add it. Default execution is top-down.
