# 12 — Global search surface

Companion to `02-surface-v2.md` §"Top bar". Closes pipeline §3.7 — turns the existing top-bar search input from a current-folder filter into a real recursive-across-the-Drive search.

## Pattern reference

**Dropbox / Google Drive / Notion** — typing in the search field switches the main pane from "this folder" to "results across everything". The results pane lives in the same grid/list as the file browser so the rest of the chrome (preview modal, context menus, sort) keeps working unchanged.

We pick the same shape because:

1. Zero extra surface — the search field is already in the top bar.
2. Results re-use the existing FileCard / ListRow components, so context-menu + multi-select + preview all work for free.
3. No nested route — the "search mode" is a state flip inside `<Files />`, not a separate page.

## Behaviour

- Search input lives where it does today.
- When the trimmed query is empty: behaviour unchanged (renders the current folder).
- When the trimmed query is ≥ 2 chars: `<Files />` calls `GET /api/search?q=…&limit=50` and renders the response instead of `listChildren`.
- Header title flips from `My Drive` (or folder name) to **`Search results`** with the count chip + the matched query echoed alongside.
- Empty result: existing EmptyState component, copy `No files match "<query>".`
- Click a folder result → navigate into it (clears search).
- Click a file result → preview modal (current behaviour).
- Folder grouping rule from sort surface (`folders first`) is preserved.
- Debounce: 200 ms after the last keystroke before firing. Cancels the in-flight request on new input via an AbortController.

## Backend contract

### `GET /api/search?q=<query>&limit=<n>` (authed)

```json
{
  "files":   [ { /* FileDto, no thumbnails for tighter wire shape */ }, ... ],
  "folders": [ { /* FolderDto */ }, ... ]
}
```

- `q` is trimmed server-side; empty queries return empty arrays without hitting the DB.
- `limit` clamped to `[1, 200]`, default 50. Files and folders each get their own slice up to `limit`.
- Owner-scoped — admins still see only their own owned files in v0; multi-user share comes with RBAC in v0.2.
- Trashed files / folders are excluded.
- Match: case-insensitive substring against the display `name`. Database does this via `LOWER(name) LIKE LOWER(?)` with `%q%` placeholders. Phase-2 swaps to OpenSearch when DRIVE_OPENSEARCH_URL is set (pipeline §11.5 / Drive optional infra memory).

## State checklist

| | Required | Notes |
|---|---|---|
| Default (query length 0–1) | yes | renders the current folder, unchanged |
| Loading | yes | existing GridSkeleton, no flicker between keystrokes (debounce + AbortController) |
| Default (results) | yes | grid/list of matched entries, "Search results · 4 items" header |
| Empty | yes | EmptyState component, query echoed |
| Error | yes | inline ErrorState ("Couldn't reach the server.") |

## Out of scope (v0 shipped)

- Full-text search (file contents) — Phase 3 alongside OpenSearch.
- Result snippets / highlight — Phase 3.
- Searchable attributes beyond `name` (tags, share-link state, owner) — Phase 3.

---

## Phase 3 refinement — filters, full-text, suggestions

The v0 shape is "type in the top bar, get a list of name-matched results across the workspace." That's the floor. The next pass turns search into a tool a real team would rely on every day — a thin filter surface above the same result grid, full-text matching once OpenSearch is configured, and a focused-empty state that's useful instead of blank.

### Vision

Search should feel as fast and as discoverable as Notion's. Filters live inline as **chips** above the result list; each chip opens a small popover with a focused control. No modal, no second page. Keyboard-first: `Cmd-K` opens the palette, `/` focuses the top bar; from either, every chip is reachable with `Tab` + arrow keys.

### Filter surface

A horizontal row of chips sits between the `Search results · 4 items` header and the result grid. Chips render only after the user has typed (or used the palette to enter "search mode"). Chips:

| Chip | Control | Notes |
|---|---|---|
| **Type** | multi-select popover: Folder / Document / Spreadsheet / PDF / Image / Video / Audio / Markdown / Archive / Other | maps to canonical `content_type` buckets |
| **Owner** | autocomplete from workspace members | "me" pre-selected when palette opened via `Cmd-K`; otherwise empty |
| **Workspace** | multi-select; defaults to current workspace | omitted from the chip row when only one workspace is visible |
| **Modified** | date-range picker with relative shortcuts (Today / Last 7 / Last 30 / Custom) | maps to `modified_at` |
| **Created** | same shape as Modified | maps to `created_at` |
| **Size** | range slider with byte-bucket labels (≤ 1 MB / 1–10 MB / 10–100 MB / ≥ 100 MB) | one selected band at a time |
| **Has share link** | toggle | true / false / either (default) |
| **In trash** | toggle | true / false / either (default false) |

Selected filters render the chip in its active state (filled, accented border) with the selected value summarised inline: `Type: PDF, Image`, `Owner: Alex`, `Modified: Last 7 days`. Clicking the chip re-opens its popover; clicking the `×` clears that filter. A `Clear all` link appears at the right end of the chip row when ≥ 1 filter is active.

### Suggested-when-empty

When the search field has focus but the query is empty, the result pane renders a curated three-column suggestion grid instead of the empty state:

1. **Recently opened** — last 8 files the user previewed or handed off to an editor (driven by audit-log lookup, capped at 30-day window).
2. **Recently edited by others** — files in the current workspace updated by other members in the last 24h. Skipped for Personal workspaces.
3. **Pinned** — Phase 3 starring feature; until then the column is hidden.

Clicking a suggestion opens the file (preview modal or editor handoff). `Cmd-Click` opens in a new tab.

### Full-text matching

Once `DRIVE_OPENSEARCH_URL` is configured (per `docs/research/16-scale-infra.md`), `GET /api/search` switches matching modes:

- File name match keeps top weight (same as today).
- Note body, markdown body, extracted file contents (Phase 2 of OpenSearch — see §16) match at lower weight.
- The response shape gains `matches: { field, snippet, offsets }[]` per result; snippets are highlighted client-side with `<mark>` around offsets so theme tokens (`--accent-muted`) handle the colour.

### Recent + saved searches

A small `<menu>` icon at the right end of the chip row opens a dropdown with:

- **Recent searches** — last 10 distinct queries the user ran, with their then-active filter set. Per-user, persisted in `localStorage` (avoids a new SQL table for now).
- **Saved searches** — Phase 4. UI shows a "Save this search" footer that's disabled with a tooltip until the backend lands.

### Keyboard model

- `Cmd-K` → palette overlay (existing); the palette gets the same filter chips below its input.
- `/` → focuses the top-bar search input (new).
- `Tab` from the search input → first chip; `Shift-Tab` → back.
- `Esc` from a chip popover → closes the popover, focus returns to the chip.
- `Esc` from a focused chip → clears that chip's filter.
- `Esc` from the search input with no query → exits search mode (top bar returns to neutral, main pane returns to folder view).

### Backend contract — Phase 3

`GET /api/search` accepts:

```
?q=<query>                                  # unchanged
&limit=<n>                                  # unchanged
&type=document,spreadsheet                  # CSV; matches canonical buckets
&owner=<user_id>                            # repeatable
&workspace=<workspace_id>                   # repeatable; defaults to caller's memberships
&modified_after=<rfc3339>
&modified_before=<rfc3339>
&created_after=<rfc3339>
&created_before=<rfc3339>
&size_min=<bytes>
&size_max=<bytes>
&has_share_link=true|false
&include_trashed=true|false                 # default false
```

Response:

```json
{
  "files":   [ { ...FileDto, "matches": [ { "field": "name", "snippet": "Q2 <mark>budget</mark>.xlsx", "offsets": [[3,9]] } ] } ],
  "folders": [ ... ],
  "notes":   [ { ...NoteDto, "matches": [ ... ] } ],
  "facets":  {                              # populated only with OpenSearch
    "type":     [{ "value": "pdf", "count": 12 }, ...],
    "owner":    [{ "value": "<user_id>", "count": 8 }, ...],
    "modified": [{ "bucket": "last_7_days", "count": 17 }, ...]
  }
}
```

Facets drive the chip popovers' default option order (most-used first) and the in-popover count chips.

### State checklist (Phase 3 additions)

| | Required | Notes |
|---|---|---|
| Chip row hidden until query typed | yes | no chrome flash on focus |
| Filter popovers keyboard-navigable | yes | every option reachable via Tab + arrow + Enter |
| Filter chips reflect URL state | yes | shareable result links (`/search?q=foo&type=pdf`) |
| Recent searches in localStorage | yes | per-user, capped at 10, dedup'd |
| Snippets rendered with `<mark>` | yes | only when backend returned `matches[]` |
| Facets driven by backend response | yes | popovers render in facet order when present |
| Focused-empty → suggestion grid | yes | three columns when data, EmptyState otherwise |
| OpenSearch absent → graceful | yes | filters still work (in-memory filtering of sqlite hits); no `facets`; no snippets |

### Pagination

The v0 floor returns up to 50 results in one call and stops. For a Drive with thousands of files that's a hole; results past 50 are invisible. Phase 3 ships **cursor pagination** with infinite-scroll on desktop + a "Load more" button on mobile (and as the keyboard fallback everywhere).

**Cursor, not offset.** Offset pagination drifts under concurrent writes (a new file lands mid-scroll, the page boundary shifts, the user sees duplicates). Cursors are stable: the server returns an opaque `next_cursor` token that encodes `(sort_field, last_value, last_id)`; the client sends `?after=<token>&limit=<n>` to fetch the next page. The token is HMAC-signed so it can't be tampered with to leak across workspaces.

**Page size.** Default 30 per page; clamp `[10, 100]`. The first request returns 30; subsequent infinite-scroll loads fetch another 30. Result density (compact vs comfortable) doesn't change page size — only how much vertical space each row takes.

**Infinite scroll trigger.** An `IntersectionObserver` watches a sentinel ≈2 viewports below the last result; when it enters the viewport, the next page is fetched. Concurrent fetches are coalesced via the same AbortController pattern as the initial query.

**Total count.** The header reads `Search results · 142 files · 6 folders · 3 notes` when the backend can produce exact counts cheaply (OpenSearch with `track_total_hits=true` capped at 10 000); falls back to `Search results · 50+ files` for the sqlite path, replacing the cap once OpenSearch is configured.

**End-of-results.** A subtle `—  End of results  —` divider replaces the loading skeleton when `next_cursor` is null. No "back to top" button (browser-native).

**Restoration on back-nav.** When the user clicks a result, opens preview, then dismisses → search state (query, filters, scroll position, loaded pages) is restored. State is held in the SPA's `SearchContext` (in-memory) + URL params for query/filters; scroll position is saved on result-click and restored on return.

### Sort

A small `[Sort: Relevance ▾]` button to the right of the filter chips opens a 5-option popover:

| Option | Notes |
|---|---|
| **Relevance** (default while `q` is non-empty) | OpenSearch `_score`; falls back to "Modified ↓" for the sqlite path |
| **Modified ↓ / ↑** | `modified_at` |
| **Created ↓ / ↑** | `created_at` |
| **Name A→Z / Z→A** | case-insensitive |
| **Size ↓ / ↑** | `size`; folders sort to the bottom |

Sort selection persists in `localStorage` keyed `cd-search-sort-v1` so power users get their preferred order on every search. Resets to "Relevance" when the user clears the query (so the sort doesn't quietly leak into the next session's first query).

The folder-grouping rule (folders first) is dropped in non-Name sorts — mixing types matters more than grouping when the user asked for "modified" or "size" order.

### Result density

A small `[Compact ▾]` toggle in the result header (right of the count chip) cycles between:

- **Comfortable** (default) — current cell + row dimensions
- **Compact** — 12 px tighter row padding, smaller thumbnail, single-line metadata

Persists per-user in `localStorage`. Doesn't affect page size. Mirrors what Notion / Linear / GitHub all ship.

### Per-result actions

Same affordances as the file list:

- **Hover** a result row → kebab `⋯` button appears at the right
- **Click** → Open / Preview / Share / Rename / Move / Trash / Copy link / Download
- **Right-click** → identical menu via the existing `ContextMenu` component
- Folder results: Open / Rename / Move / Trash / Share folder (Phase 3 SH1)
- Note results: Open / Rename / Move / Trash / Copy link
- `Cmd-Click` → opens the result in a new tab without leaving search mode
- `Shift-Click` → toggles into multi-select (see below)

### Bulk actions on results

Multi-select works inside search results identically to the file list. The selection bar slides in at the top of the result pane when ≥ 1 result is selected:

- **Move** (only when all selected are in one workspace + owner-allowed)
- **Trash**
- **Download as zip** (files only; folders ≥ 1000 files are skipped client-side with a one-line note)
- **Share** (Phase 3 follow-up — only enabled when exactly one item selected)
- **Tag** (Phase 4 — when tags ship)

Selection persists across pagination loads. `Esc` clears the selection. `Cmd-A` selects every loaded result (does **not** auto-fetch unloaded pages — that would be a footgun for thousand-result selections).

### Type-ahead query autocomplete

While the user is typing, a small popover under the search input suggests query completions sourced from:

1. **Recent queries** matching the prefix (from `localStorage`).
2. **Top file / folder / note names** matching the prefix (from a lightweight `/api/search/autocomplete?q=<prefix>&limit=5` endpoint — separate from the full search call, max 50ms p99, hits OpenSearch's prefix-suggester or sqlite `name LIKE 'prefix%'`).
3. **Member names** when the user has typed `from:` or `owner:` (Phase 4 — bare-word query language).

The popover is keyboard-navigable (arrow keys + Enter); clicking outside or `Esc` dismisses. Selecting a suggestion fills the input and triggers a full search.

Debounce: 80 ms (tighter than the full search's 200 ms) so suggestions feel instant. AbortController on every keystroke.

### Search history

The same recent-searches dropdown described above shows the last 10 distinct queries with their then-active filter set. Each entry is one click to re-run. A `Clear history` link sits at the bottom. Stored in `localStorage` per-user under `cd-search-history-v1`; never sent to the server in v0.3 (Phase 4 may surface team-level "popular searches" but that's its own brief).

### No-results recovery

When the result pane comes back empty, the user gets actionable choices rather than a dead end:

```
                       No matches for "kickoff"

         Try:
           ⤴ Search in trash too         [toggle In-trash filter]
           ⤴ Drop the Owner filter       [if Owner ≠ "anyone"]
           ⤴ Widen Modified to All       [if Modified ≠ "any"]
           ⤴ Search across all workspaces [if scope = current]

         Did you mean: "kickoffs", "kick off"?    [if backend returned suggestions]
```

Each line is a button; clicking it relaxes the named filter + re-runs the search in place. Suggestions come from OpenSearch's `phrase_suggester` (when configured) or a Levenshtein-1 sqlite fallback over the workspace's most-common file/note name tokens.

### Search scope: current folder vs workspace vs all-workspaces

A small scope chip on the **left** end of the chip row (always present, not hidden until typing):

- **Current folder** — only when the user is currently browsing inside a non-root folder; auto-selected when they hit `/` from inside a folder
- **Workspace** (default) — all the active workspace's files + notes
- **All my workspaces** — every workspace the caller is a member of (multi-workspace operators); omitted from the picker when the user has only one workspace

Scope selection is part of the URL state (`&scope=folder|workspace|all`). Switching scope clears pagination + re-runs.

### Performance budget

| Operation | Target | Backstop |
|---|---|---|
| Keystroke → first paint of result pane | < 200 ms p95 (after debounce) | 300 ms hard timeout → show stale results + spinner |
| Type-ahead suggestion popover | < 80 ms p95 | 150 ms hard timeout → suppress that keystroke's suggestions |
| Pagination "Load more" round-trip | < 200 ms p95 | 500 ms → keep showing skeleton row |
| Filter-chip change → re-search | < 200 ms p95 | same as above |
| AbortController cancels in-flight on every new keystroke | always | n/a — never let a stale response render |

Cumulative layout shift across a search session: 0. Loading skeletons match the post-load row geometry exactly (same height + cell layout) so the page never jumps.

### Accessibility

- The search input has `role="combobox" aria-expanded` driven by the type-ahead popover.
- The popover list has `role="listbox"`; each option has `role="option" aria-selected`.
- Result count chip is announced via `aria-live="polite"` on update ("142 files, 6 folders, 3 notes").
- New results loaded by infinite scroll are announced via the same live region ("30 more results loaded").
- Filter chips are `role="button" aria-pressed`; popovers are `role="dialog"` and trap focus while open.
- The whole search flow is operable with keyboard only — never a "click here" without a key binding.
- Every interactive element has a label; icon-only buttons get a tooltip + `aria-label`.
- Respects `prefers-reduced-motion`: no scroll-into-view animations, no slide-in transitions on the chip row.

### Empty workspace

When the caller's current workspace has zero files + zero notes, the search input is rendered disabled with placeholder "Nothing to search yet — upload a file or create a note." The chip row is hidden. Suggestions list is empty.

### Complete state checklist (Phase 3)

| State | Required | Notes |
|---|---|---|
| Query empty, focused | yes | suggestion grid (Recently opened / Edited by others / Pinned) |
| Query empty, unfocused | yes | renders current folder, behaviour unchanged |
| Query typing (1 char) | yes | type-ahead popover; result pane still shows current folder |
| Query ≥ 2 chars | yes | result pane flips to results; chip row appears; debounce 200 ms |
| Filter chip selected, query empty | yes | search executes with `q=""` if any filter is set |
| Loading first page | yes | grid/list skeletons matching geometry, count chip = "Searching…" |
| Loading subsequent page | yes | sentinel + 3 skeleton rows at the bottom |
| Results, scroll restoration on back-nav | yes | scroll + loaded pages restored |
| No results, no filters set | yes | empty state with did-you-mean suggestions when available |
| No results, filters set | yes | recovery panel with one-click filter relaxations |
| End of results | yes | `— End of results —` divider |
| Network error (initial) | yes | inline ErrorState + "Retry" |
| Network error (pagination) | yes | error row at the sentinel with "Retry" — earlier results stay |
| Filter popovers keyboard-navigable | yes | every option reachable via Tab + arrow + Enter |
| URL reflects state (query + filters + sort + scope) | yes | shareable result links |
| Recent searches in localStorage | yes | per-user, capped at 10, dedup'd |
| Snippets rendered with `<mark>` | yes | only when backend returned `matches[]` |
| Facets driven by backend response | yes | popovers render in facet order when present |
| OpenSearch absent → graceful | yes | filters still work (in-memory filter of sqlite hits); no facets, snippets, did-you-mean, autocomplete |
| Empty workspace | yes | disabled input, helper copy |

### Out of scope (Phase 3 — saved for Phase 4+)

- Cross-workspace search by default (currently opt-in via scope chip; default-on needs RBAC + audit on every cross-workspace read — separate brief).
- Boolean operators in the query (`"exact"`, `-not`, `OR`, `from:`, `before:`) — power-user surface; UI cost > value at this stage. The autocomplete groundwork is laid; the parsing isn't.
- Saved search server-side persistence + sharing.
- Search-as-API for integrations.
- ML-based relevance tuning. The OpenSearch defaults are good enough for v0.4 / v0.5.
- Team-level "popular searches" — needs an opt-in privacy policy first.
- Search-result thumbnails larger than the file-list default — would require a layout shift on filter, not worth it.

### AI integration seam

A natural-language query rewriter would sit ahead of the OpenSearch call: "PDFs Alex shared last week" → `{ q: "", type: ["pdf"], owner: ["<alex_id>"], has_share_link: true, modified_after: "<7d_ago>" }`. **Path-only — not work** until the user prioritises it; see [`../../PIPELINE.md#path-only-ai-integration-seams`](../../PIPELINE.md#path-only-ai-integration-seams).

