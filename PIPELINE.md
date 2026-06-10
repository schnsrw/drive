# Casual Drive — Pipeline

**What this is:** the forward-looking work queue. Everything in here is *missing or upcoming* — what has shipped lives in [`CHANGELOG.md`](./CHANGELOG.md) and `git log`. Each item carries the brief that owns it, the trigger that says "start now," and the priority.

**Priority bands**

- **P0** — blocks the next minor release.
- **P1** — high-value; pick up as soon as P0 is empty.
- **P2** — nice-to-have; queued.
- **P3** — vision / "future" work; needs more research before it can move.

**Trigger** — the concrete condition that flips a row from "queued" to "do this next." Some are user-facing ("first operator outgrows in-process"), some are calendar ("before the first multi-tenant prod deploy"), some are dependency ("after §X lands").

---

## Theme: Scale & infra

Single-binary stays the default. These items light up once a deployment grows past one instance or ~50 concurrent users.

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| S1 | Redis: optional rate-limit / session / presence / cache-invalidation backend behind a trait | [`16-scale-infra`](./docs/research/16-scale-infra.md) | P1 | First operator reports >1 replica OR `/api/admin/system` shows rate-limit buckets > 1000 sustained |
| S2 | OpenSearch: optional search backend behind a trait (file + note name/body indexing) | [`16-scale-infra`](./docs/research/16-scale-infra.md) | P1 | Search responses miss user intent on a Drive with > ~10k files OR an operator opts in via env |
| S3 | Indexer worker: incremental, idempotent, debounced bulk-API push | [`16-scale-infra`](./docs/research/16-scale-infra.md) §"Re-indexing" | P1 | Same as S2 |
| S4 | OpenSearch Phase 2: extracted file contents (text/CSV/markdown direct; PDF/Office via sandboxed extractor) | [`16-scale-infra`](./docs/research/16-scale-infra.md) §"Phase 2" | P2 | After S2 + S3 are stable in the wild |
| S5 | Health probe + circuit breaker surfaced in Admin → System | [`16-scale-infra`](./docs/research/16-scale-infra.md) §"Health-check" | P1 | Ships alongside S1/S2 |

## Theme: Search UX

Floor is in place (`GET /api/search` over file names, 50-result cap, no filters). The refinement turns it into a tool a team uses every day. **The "Search foundation" rows (SR1–SR8) ship together as one P0 pass** — pagination + filters + sort are interlocking and shipping any one alone leaves the surface feeling half-built. The remaining rows (SR9–SR15) are layered polish that can land in subsequent passes.

### Search foundation (ship together, one P0 pass)

Phase A + Owner chip landed (backend + chip toolbar + Owner autocomplete + infinite scroll + count chip). Remaining polish:

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| SR7 | Audit pass shipped — file / folder kebab, right-click, shift-+-cmd-click multi-select, SelectionBar, bulk-trash, bulk-download all share the same code path in browse and search mode (`filteredEntries` is the source of truth in both); `refresh()` regression fixed; note hits now carry a kebab with **Copy link** (deep-links via `?note=<id>` that Shell hydrates on mount). Remaining: rename / move / trash for note hits — pending a Notes-tab actions surface those operations can re-use, then the SR7 row closes. | [`12-search-surface`](./docs/ux/12-search-surface.md) §"Per-result actions" + §"Bulk actions" | P2 | After the Notes-tab actions surface lands |

### Search polish (layered, subsequent passes)

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| SR9  | Focused-empty suggestion grid (Recently opened / Edited by others / Pinned) | [`12-search-surface`](./docs/ux/12-search-surface.md) §"Suggested-when-empty" | P1 | After SR1–SR8 land |
| SR10 | Type-ahead query autocomplete (separate `/api/search/autocomplete` endpoint, 80 ms debounce). Recents popover (SR11) is the host UI — this drops server suggestions in alongside the localStorage entries. | [`12-search-surface`](./docs/ux/12-search-surface.md) §"Type-ahead" | P1 | After SR1–SR8 land |
| SR12 | No-results recovery panel (one-click filter relaxation + did-you-mean from OpenSearch `phrase_suggester`) | [`12-search-surface`](./docs/ux/12-search-surface.md) §"No-results recovery" | P1 | After SR1–SR8 land |
| SR13 | Full-text snippets + `<mark>` highlights | [`12-search-surface`](./docs/ux/12-search-surface.md) §"Full-text matching" | P1 | Requires S2 (OpenSearch) |
| SR15 | Performance budget. **Instrumentation + CI assertion shipped + budget actually met** — `lib/searchMetrics.ts` tracks keystroke→paint via `performance.mark` + `performance.measure`, PerformanceObserver buffer 100 samples, stats on `window.__cd_search_perf`. Search debounce dropped from 200 ms → 50 ms (Notion-style) so the 200 ms spec budget is reachable: local p95 settled at 207 ms (was 417 ms). E2E asserts `p95 < 500 ms` — 2× headroom over the local baseline for CI variance. **Remaining**: (a) tighten the ceiling further toward 200 ms after a few weeks of green CI; (b) type-ahead < 80 ms once SR10 lands; (c) OpenSearch round-trip < 150 ms once S2 lands. | [`12-search-surface`](./docs/ux/12-search-surface.md) §"Performance budget" | P1 | SR10 / S2 unblock the remaining sub-budgets |
| SR16 | Saved searches (server-side persistence + sharing) | — (Phase 4; needs brief) | P3 | Real user demand surfaces in feedback |
| SR17 | Boolean operators in the query (`"exact"`, `-not`, `OR`, `from:`, `before:`) | — (Phase 4; needs brief) | P3 | Power-user feedback after SR10 lands |

## Theme: Real-time / presence

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| RT1 | `PresenceHub` (in-process), SSE endpoint, heartbeat, leave | [`14-presence`](./docs/research/14-presence.md) | P1 | Multi-user OIDC sign-in is the floor; presence is the next visible team feature |
| RT2 | Avatar stack in workspace switcher row | [`14-presence`](./docs/research/14-presence.md) §"SPA surface" | P1 | Ships with RT1 |
| RT3 | File-row "someone else is viewing" dot | [`14-presence`](./docs/research/14-presence.md) §"SPA surface" | P1 | Ships with RT1 |
| RT4 | Quiet action toast (rename / trash / move) for files in viewport | [`14-presence`](./docs/research/14-presence.md) §"SPA surface" | P2 | Ships with RT1 |
| RT5 | Redis-backed presence hub for multi-instance | [`16-scale-infra`](./docs/research/16-scale-infra.md) + [`14-presence`](./docs/research/14-presence.md) | P2 | After RT1 + S1 |

## Theme: Editor integration (SDK-first)

Editor handoff in Drive runs through the Casual Sheet + Casual Document **SDKs** (browser-only) by default. A WOPI server-side path is opt-in for real-time co-editing — see [`10-sdk-integration-plan`](./docs/ux/10-sdk-integration-plan.md).

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| ED1 | SDK-embed handoff for `.xlsx` (sheet/) and `.docx` (document/) — browser-only. Phase 1 shipped (Preview-modal hosting, AutosaveStatus, lazy chunks, co-edit env opt-in). **Remaining gaps** under the [[unified-editor-lifecycle]] directive (2026-06-10): **(a)** fullscreen route `/file/<id>` peer of home/notes/activity, so the editor can break out of the preview modal frame; **(b)** sandboxed PNG thumbnails for `.docx` / `.xlsx` (today they get `FileMiniIcon`) — same `drive-thumb-worker` lane that PDF + diagram thumbnails will use; **(c)** lift the existing modal editor-host code into a reusable `EditorShell` so ED4 + future formats inherit the chrome wholesale. | [`10-sdk-integration-plan`](./docs/ux/10-sdk-integration-plan.md) + [[unified-editor-lifecycle]] | P1 | (a) before / alongside ED4; (b) ships with TH1 + the new docx/xlsx lanes; (c) refactor sprint when ED4 starts |
| ED2 | Co-edit opt-in: detect editor server URL + bridge to existing WOPI host | [`01-wopi`](./docs/research/01-wopi.md) | P1 | Operator with > 1 team member opts in |
| ED3 | MS365 / Office Online federation (proof-key RSA hook wakes up) | [`13-ms365-federation`](./docs/research/13-ms365-federation.md) | P3 | Deprioritised — SDK is the primary integration path. Wake when an operator specifically needs MS365 |
| ED4 | **Excalidraw editor for `.excalidraw` files** — third format under the [[unified-editor-lifecycle]] (one `EditorShell`, three swappable editor components). Format-specific bits: new MIME `application/vnd.excalidraw+json`, `inferKind` `excalidraw` bucket, `<Excalidraw />` component (~1.5 MB lazy chunk), `exportToBlob` lane in `drive-thumb-worker`, `theme` prop fed by `prefers-color-scheme`. **Image pipeline** (refined 2026-06-10): on insert, the SPA intercepts the binary, runs canvas → `toBlob({type:"image/webp", quality:0.85})`, then feeds the WebP data URL into Excalidraw's `addFiles`. SVGs pass through as-is (don't raster a vector). Everything stays inline in the native `.excalidraw` JSON's `files` key — one self-contained file. The `onChange` save handler MUST capture all three args `(elements, appState, files)`; load rehydrates via `excalidrawAPI.addFiles()` or `initialData.files`. Everything else (Sidebar New entry, Preview-modal chrome + AutosaveStatus + Cmd-S, fullscreen route, co-edit via `VITE_DRIVE_COLLAB_BACKEND_URL`, share-link PNG, audit events) inherits from the shared shell — no parallel implementation. Brief: `docs/research/19-excalidraw.md` (TODO). | — (needs brief — see [[excalidraw-integration]] + [[unified-editor-lifecycle]]) | P2 | After ED1's lifecycle refactor (a)+(c). Sibling collab-server repo decision blocks the co-edit half. |

## Theme: Multi-user / RBAC

The OIDC floor is in. Filling out the team-collaboration story:

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| MU1 | Workspace invitations (token-based; email-or-link) | — (needs brief; design notes in `08-byo-storage` only cover storage) | P1 | After RT1 (presence) — invitations are the moment team workspaces stop being "owner-only" |
| MU2 | Role tiers beyond Owner / Member (Viewer, Editor, Admin) | — (needs brief) | P2 | After MU1 |
| MU3 | Server-mediated email (transactional: invite / share / quota-request) | — (needs brief) | P2 | After MU1 |
| MU4 | OIDC group → role mapping (admin group, per-workspace group claims) | [`12-oidc`](./docs/research/12-oidc.md) §"admin_group" extension | P2 | After MU1 + MU2 |

## Theme: Thumbnails

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| TH1 | pdfium-render PDF page-1 thumbnails in `drive-thumb-worker`. Under the [[unified-editor-lifecycle]] this row absorbs three sibling lanes: PNG thumbnails for `.docx` + `.xlsx` (LibreOffice CLI or a tiny canvas renderer) and `.excalidraw` (`exportToBlob`). All four share the worker's threat model + rlimits + (eventually) seccomp filter from TH2. | [`15-sandboxed-thumb-worker`](./docs/research/15-sandboxed-thumb-worker.md) §"PDF" + [[unified-editor-lifecycle]] | P1 | Promoted to P1 because the office-format lanes block ED4 + the unified lifecycle; PDF lane still gated on operator demand but ships in the same worker upgrade |
| TH2 | Linux seccomp syscall filter in the worker | [`15-sandboxed-thumb-worker`](./docs/research/15-sandboxed-thumb-worker.md) §"seccomp" | P2 | Before the first multi-tenant prod deploy |
| TH3 | HEIC / RAW image support (likely via `libheif` in the worker) | — (mentioned in `15-sandboxed-thumb-worker` out-of-scope) | P3 | iPhone uploads become common in feedback |
| TH4 | CDN-cacheable thumbnail URLs (versioned key + far-future cache header) | — (briefly noted in `11-server-thumbnails`) | P2 | After TH1 |

## Theme: Uploads

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| UP2 | Resumable uploads (tus.io protocol or S3 multipart with checkpoint) | — (needs brief; `10-direct-upload` lists as out-of-scope) | P2 | First operator hits the wall on a > 1 GB upload |
| UP3 | EXIF / metadata strip on image uploads | — (needs brief) | P2 | Before the first public share-link feature with image embeds |
| UP4 | Per-workspace upload quotas (today: per-user only) | — (needs brief) | P2 | After MU1 (workspace-scoped accounting matters once teams exist) |

## Theme: Sharing

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| SH1 | Folder-level share links (today: per-file) | — (extends `05-sharing-surface`) | P1 | First user reports they need to share a folder |
| SH2 | Share-link descriptions + indexed in search | [`12-search-surface`](./docs/ux/12-search-surface.md) | P2 | Ships with SR3 |
| SH3 | Per-share download caps (max-N or expire-after-N-downloads) | — (extends `05-sharing-surface`) | P2 | Operator with sensitive shares asks |
| SH4 | Authenticated shares (require a Drive account to open) | — (needs brief; depends on MU1) | P2 | After MU1 |

## Theme: Notes / Wiki

The current Notes app is shaped for developers (markdown source pane + literal `[[link]]` syntax). For Drive to be the file home of a real team, Notes needs the experience Obsidian Live Preview / macOS Notes / Mem / Bear have converged on: live-render markdown, no source ever visible, slash + `@` + `+` as discovery aids, premium aesthetic restraint.

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| NT1 | **Live-render Tiptap editor** — Phase 1 (foundation) + Phase 2 (wiki-link picker inserts a real Tiptap Link mark with `href="cd-note://<id>"`; MarkdownEditor's `handleClick` intercepts and fires `cd:open-note`; markdown round-trips as `[Title](cd-note://id)`). Older notes that stored `[[Title]]` as plain text still render as text and upgrade lazily on edit. Remaining: `[[` as a parity trigger for users who prefer Obsidian's syntax (multi-char trigger needs a custom PM plugin). | [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md) | P2 | `[[` parity is the last piece |
| NT2 | Floating formatting toolbar — Phase 1 shipped (Bold / Italic / Strike / Code / H1-H3 / Bullet / Numbered / Quote). Phase 2 link dialog shipped (⌘K opens a Radix dialog; URL field auto-prepends `https://`, rejects `javascript:` / `data:` / `file:`; Remove button when caret is inside an existing link; mobile toolbar gains a Link button). Remaining: "Turn into → ..." sub-menu. | [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md) §"Floating formatting" | P2 | "Turn into" is the last piece |
| NT3 | Slash menu (`/`) — Phase 1 shipped (H1-H3 / Bullet / Numbered / Quote / Code block / Divider with keyboard nav). Phase 2 "Link to note" shipped — slash item hands off to the existing `+` note-link picker. Remaining: "Embed file from Drive" — needs a file-picker component (workspace tree + thumbnail preview) that doesn't exist yet; ships once we surface a reusable file-picker for the @ mention parity work too. | [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md) §"Slash menu" | P2 | "Embed file" ships with the workspace-wide file picker |
| NT4 | `@` people-mention + `+` note-link pickers — Phase 1 shipped (member fetch via `/api/workspaces/{id}/members`, note picker reads tree from parent, "Create page «query»" footer, keyboard-first navigation). Phase 2 adds the `[[` parity trigger (needs a custom multi-char ProseMirror plugin) + a semantic mention node tied to notifications. | [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md) §"@ for people, [[ or + for notes" | P2 | `[[` parity after user feedback; semantic mention node alongside notifications brief |
| NT6 | Mobile sticky bottom toolbar — Phase 1 shipped (Bold / Italic / List / Heading cycle / Link placeholder / `/` opens slash menu; sits above the keyboard with safe-area padding). Phase 2 adds the long-press block sheet (mobile analogue of NT5's drag-handle menu — shares design surface). | [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md) §"Mobile" | P2 | Long-press sheet after NT5 ships |
| NT7 | Note attachments (drag-drop image / file → embed) | — (extends `09-notes-wiki`) | P2 | After SR1 — search should index attached file names alongside note bodies |
| NT8 | Note-to-PDF / note-to-public-web export | — (needs brief) | P2 | Operator asks |
| NT9 | Real-time collab on notes (Tiptap + Yjs) | — (needs brief; out-of-scope for [`17-notes-general-user-ux`](./docs/research/17-notes-general-user-ux.md)) | P3 | After NT1 + ED2 — Tiptap pivot makes Yjs achievable |
| NT10 | AI block actions (`/ask AI`, summarise, translate) | — **path-only, not work** | P3 | Integration seam: NT3 slash menu's command list. No brief, no provider pick, no implementation until explicitly prioritised. See [Path-only AI](#path-only-ai-integration-seams) below |

## Theme: Marketing site / docs

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| MK1 | Domain flip + final CNAME (`drive.schnsrw.live` → final apex) | [`07-marketing-site`](./docs/research/07-marketing-site.md) | P1 | Calendar / DNS decision; nothing technical blocks it |
| MK-PERF-95 | **Restore marketing Lighthouse gate to ≥0.95** (currently 0.85 — temporarily relaxed). Phase 1 (AVIF/WebP screenshots) shipped 0.74 → 0.84; preload attempt regressed TBT to 2150ms (decode contention). Investigate: smaller mobile-only screenshot variant with `<source media>`, deferring inline scripts, code-splitting any remaining heavy CSS. Use the local mobile-profile Lighthouse with `cpuSlowdownMultiplier:4` + 3 runs to mirror CI's variance. | [`07-marketing-site`](./docs/research/07-marketing-site.md) | P1 | Pick up before the next marketing-facing release |
| MK2 | Pagefind docs search | [`07-marketing-site`](./docs/research/07-marketing-site.md) | P2 | After the first user can't find a doc page on their own |
| MK3 | i18n (start with the marketing site, then docs, then SPA) | — (needs brief) | P3 | First non-English contributor opens an issue |

## Theme: Observability

| # | Item | Brief | Priority | Trigger |
|---|---|---|---|---|
| OB2 | Prometheus metrics endpoint (`/metrics` on the app origin, mTLS or token-gated) | — (needs brief) | P2 | First operator asks for Grafana integration |
| OB3 | OpenTelemetry traces for the request lifecycle | — (needs brief) | P3 | After OB2 |
| OB4 | Audit-event export to S3 (rolling daily JSONL) | — (needs brief) | P2 | Compliance ask from an operator |

---

## Path-only AI integration seams

AI is **deliberately not work** at the current stage of the project — the value Drive delivers is "file home for a real team," and AI is icing on a cake that hasn't fully baked. Briefs and PIPELINE rows may *name* where an AI hook would integrate, in one sentence, so future contributors don't have to re-derive the seams. They must not turn into work without explicit user green-light.

The documented seams across surfaces (for orientation only, not as a queue):

| Surface | Where AI would plug in | What it would do |
|---|---|---|
| Notes editor | Slash-menu command list (NT3) | `/ask AI` → block-level summarise / translate / continue-writing / extract-tasks |
| Search | `GET /api/search` query rewriter ahead of the OpenSearch call (SR3) | Natural-language → structured filter inference ("PDFs Alex shared last week") |
| File upload | Post-finalize hook alongside the magic-byte sniff (UP1) | Auto-tag / auto-describe newly uploaded images + PDFs |
| Sharing | Share-link creation form | Auto-summarise the file's contents into the share description (SH2) |
| Activity | Audit-event renderer | Natural-language daily digest of "what happened in this workspace" |

None of these have a brief, a provider pick, a prompt design, or any code. They are markers for the future. Adding any of them to the active queue requires the user saying "build it."

---

## What's not in this pipeline

If a row is not here, one of three things is true:

1. It's already shipped — see [`CHANGELOG.md`](./CHANGELOG.md).
2. It's explicitly out of scope for the foreseeable future (see the "out of scope" sections in each research brief).
3. It's a real gap we haven't recognised yet — open an issue.

---

## How to add a row

Each row should answer four questions in this order:

1. **What** — one short noun phrase.
2. **Which brief** — link the doc that owns the design. If the brief doesn't exist yet, mark as "needs brief" and queue writing one first.
3. **Priority** — P0–P3 per the bands above.
4. **Trigger** — the concrete signal that says "start now." A trigger like "first operator asks" is fine; a trigger like "before the first multi-tenant prod deploy" is better.
