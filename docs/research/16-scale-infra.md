# 16 — Scale infra (Redis + OpenSearch) (Phase 3)

Drive ships single-binary by design. Until ~50 concurrent users, the in-process defaults (sqlite/postgres for metadata, in-process rate limit, in-process presence hub, `LIKE` search) carry the load and operators don't need to run Redis or OpenSearch.

Past that threshold — or the moment Drive is run as more than one replica behind a load balancer — those defaults start to bite. This brief locks the shape of the opt-in path so operators can flip env vars and scale without rewrites.

## Why now

Two triggers force the conversation:

1. **>50 concurrent users on one instance.** The in-process rate-limit map grows unbounded, search starts doing `LIKE '%q%'` table scans, the SSE presence hub allocates `O(users × subscribers)` on every fan-out. None of it falls over yet — but the operator's CPU graph stops being flat.
2. **More than one Drive replica.** The moment a deployment runs `N > 1` instances behind a load balancer, the in-process state (rate limit, sessions if `MemoryStore`, presence hub, BYO adapter cache) becomes per-instance — users get a different bucket on every request and presence stops working entirely.

Both are deferred from v0 on purpose: a self-hoster on a $5 VPS shouldn't need to spin up Redis to read their own files. The opt-in lets them stay on the simple path until they've outgrown it.

## Locked decisions

### **Two optional services, never more**

- **Redis** — the default escape hatch for ephemeral shared state: rate limit, session store, presence hub, BYO adapter cache invalidation. Same Redis instance for all four (different key prefixes).
- **OpenSearch** — the default escape hatch for search: full-text indexing of file names, note bodies, share-link descriptions; filter aggregations (by type / owner / workspace / date / size). Optional secondary cluster for audit log analytics, but that's Phase 4.

No MeiliSearch, no Elasticsearch (license), no SOLR. OpenSearch wins on Apache-2.0 license + the AWS-managed flavour being a one-click for the most common deployment shape.

### **Both services are strictly opt-in via env**

```
DRIVE_REDIS_URL=redis://redis.internal:6379/0
DRIVE_OPENSEARCH_URL=https://opensearch.internal:9200
DRIVE_OPENSEARCH_INDEX_PREFIX=drive_  # default
```

If unset, Drive runs in-process for everything. Operators read the same docs as the rest of the env-var matrix.

### **Trait at the boundary, two impls per service**

Every surface that needs the escape hatch sits behind a trait that ships with two implementations:

| Surface | Trait | In-process impl | Redis impl |
|---|---|---|---|
| Rate limit | `RateLimiterBackend` | `InMemoryLimiter` (today) | `RedisLimiter` (Lua script for atomic refill) |
| Session store | `SessionStore` (tower-sessions) | `SqliteStore` (today) | `RedisStore` |
| Presence hub | `PresenceHubBackend` (§14) | `InProcessHub` | `RedisPubSubHub` |
| BYO cache invalidation | `StorageCacheBus` | `NoopBus` (today: single-instance assumption) | `RedisPubSubBus` |

| Surface | Trait | In-process impl | OpenSearch impl |
|---|---|---|---|
| File / note search | `SearchBackend` | `SqlLikeSearch` (today) | `OpenSearchBackend` |
| Search filters | (same) | reads sqlite cols + filters in memory | OpenSearch aggregations |

`Config::from_env` reads `DRIVE_REDIS_URL` / `DRIVE_OPENSEARCH_URL` and the binary picks the right impl at boot. Handlers see only the trait.

### **OpenSearch indexes file metadata, not file contents (v0.3)**

Phase 1 of OpenSearch integration covers:

- File `name`, `path`, `tags` (when tags land), `owner_username`, `workspace_id`, `created_at`, `size`, `content_type`.
- Note `title`, `body` (sanitised markdown source), `wiki_links`, `workspace_id`.
- Share-link `description` (Phase 2 sharing surface).

Phase 2 (v0.4): file *contents* — text/CSV/markdown extracted at upload time, PDF + Office formats via a separate `drive-text-extractor` worker (same sandbox shape as `drive-thumb-worker`). Out of scope here.

### **Search backend wire contract**

Both impls (`SqlLikeSearch` and `OpenSearchBackend`) speak the same `SearchBackend` trait and produce the same `GET /api/search` response shape. The differences are in capability, not contract — what the in-process path can't compute (snippets, facets, did-you-mean) is simply omitted from the response, and the SPA renders the graceful-degraded UI.

**Request — `GET /api/search`**

```
?q=<query>                        # may be empty when at least one filter is set
&after=<cursor>                   # opaque, HMAC-signed; absent = first page
&limit=<n>                        # default 30, clamp [10, 100]
&sort=relevance|modified|created|name|size
&sort_dir=desc|asc                # default desc for modified/created/size, asc for name
&scope=folder|workspace|all
&folder_id=<id>                   # required when scope=folder
&type=<csv>                       # canonical buckets: folder,document,spreadsheet,pdf,image,video,audio,markdown,archive,other
&owner=<user_id>                  # repeatable
&workspace=<workspace_id>         # repeatable; required when scope=workspace+
&modified_after=<rfc3339>
&modified_before=<rfc3339>
&created_after=<rfc3339>
&created_before=<rfc3339>
&size_min=<bytes>
&size_max=<bytes>
&has_share_link=true|false
&include_trashed=true|false       # default false
```

**Response shape**

```json
{
  "files":   [ { /* FileDto */, "matches": [ { "field": "name", "snippet": "...", "offsets": [[3,9]] } ] } ],
  "folders": [ { /* FolderDto */ } ],
  "notes":   [ { /* NoteDto */, "matches": [ ... ] } ],
  "total":   { "files": 142, "folders": 6, "notes": 3, "exact": true },
  "next_cursor": "eyJzIjoibSIsInYiOjE3...",   // null when no more pages
  "facets":  {                                 // OpenSearch only; omitted otherwise
    "type":     [{ "value": "pdf", "count": 12 }, ...],
    "owner":    [{ "value": "<user_id>", "count": 8, "label": "Alex" }, ...],
    "modified": [{ "bucket": "last_7_days", "count": 17 }, ...]
  },
  "suggestions": ["kickoffs", "kick off"]      // OpenSearch only; omitted otherwise
}
```

**`total.exact` semantics**

- `true` when the backend produced an exact count cheaply (OpenSearch with `track_total_hits=true` up to 10 000).
- `false` when the count is the page-size cap (sqlite path) — SPA renders `50+ files` / `Many results` accordingly.

**Cursor format**

Opaque base64-url string. Internally `HMAC-SHA256(sort_field || last_value || last_id || page_size || filters_hash)`, signed with the existing `signed_url_hmac_secret`. The server verifies the HMAC + the filter hash on every paginated request — a forged or stale cursor returns 400, never silently drifts the result set. Cursors expire 1 hour after issuance (the same window as a typical user session of scrolling).

**Sort semantics**

- `relevance` requires OpenSearch (BM25 `_score`); the sqlite path silently falls back to `modified desc` and returns `sort_applied: "modified"` in the response so the SPA can grey out the Relevance option in the popover.
- All other sorts apply equally to both backends.
- Folder grouping (folders first) is applied only when `sort=name`. Other sorts mix types — the user asked for "by modified date," they don't want folders artificially floated.

**OpenSearch query shape (Phase 1)**

```json
{
  "query": {
    "bool": {
      "must":   [ { "multi_match": { "query": "<q>", "fields": ["name^4", "title^3", "body"] } } ],
      "filter": [
        { "terms": { "workspace_id": ["<ws1>", "<ws2>"] } },   // always present; caller's memberships
        { "terms": { "content_type_bucket": ["pdf", "image"] } },
        { "term":  { "trashed": false } },
        { "range": { "modified_at": { "gte": "<rfc3339>", "lte": "<rfc3339>" } } }
      ]
    }
  },
  "aggs": {
    "type":     { "terms": { "field": "content_type_bucket", "size": 10 } },
    "owner":    { "terms": { "field": "owner_id", "size": 10 } },
    "modified": { "date_range": { "field": "modified_at", "ranges": [...] } }
  },
  "highlight": { "fields": { "name": {}, "title": {}, "body": { "fragment_size": 120, "number_of_fragments": 2 } } },
  "suggest": {
    "did_you_mean": { "text": "<q>", "phrase": { "field": "name", "size": 3 } }
  },
  "size":  30,
  "search_after": [ "<sort_value>", "<id>" ],   // from cursor
  "sort":  [ { "_score": "desc" }, { "_id": "asc" } ],
  "track_total_hits": 10000
}
```

**Sqlite fallback shape**

```sql
SELECT id, name, modified_at, ...
FROM   files
WHERE  workspace_id IN (?, ?, ...)
  AND  trashed_at IS NULL
  AND  LOWER(name) LIKE LOWER(?)             -- '%<q>%'
  AND  modified_at >= ? AND modified_at <= ?
  AND  ...
ORDER  BY modified_at DESC, id DESC
LIMIT  31;                                    -- +1 for has-more detection
```

The same query runs over `notes` (matching `title` and `body`) and `folders` (matching `name`); the handler unions + truncates client-side. `has_share_link` is computed via `EXISTS (SELECT 1 FROM share_links WHERE file_id = files.id)` — a single index lookup.

**Performance SLOs**

| Path | p50 | p95 | p99 | Backstop |
|---|---|---|---|---|
| Sqlite, ≤ 10k files in workspace | < 40 ms | < 120 ms | < 300 ms | 500 ms timeout → empty result + warning header |
| OpenSearch, ≤ 100k files indexed | < 50 ms | < 150 ms | < 400 ms | circuit-break → sqlite fallback for 60s |
| Type-ahead autocomplete (separate endpoint) | < 30 ms | < 80 ms | < 150 ms | suppress for that keystroke; never delay the main result |

**Abort semantics**

The handler reads `req.extensions::<tokio::sync::CancellationToken>()`. Axum already cancels the request future when the client drops; the OpenSearch / sqlx future inherits the cancellation. No per-handler abort code — just make sure long queries don't block on uncancellable work.

**Facet cache**

OpenSearch facets for the *unfiltered* current workspace are cached in the optional Redis cache for 30 seconds (key: `search:facets:<ws>:<scope>`). Filter-narrowed facets are computed on every request — they're cheap and the cache hit rate would be near zero.

### **Re-indexing is incremental + idempotent**

- Every write to `files` / `notes` / `share_links` enqueues an indexing job (`tokio::spawn` for v0.3; queue worker once volume justifies one).
- On boot, Drive performs a delta sync: walks rows newer than the last-indexed-at, pushes diffs. No full reindex on every restart.
- An `/api/admin/reindex` button drops the indexes + rebuilds — for after schema changes or recovering from index corruption.

### **Redis-backed sessions inherit `tower-sessions::RedisStore` directly**

No reinvention. `tower-sessions` already supports Redis; the env-var swap is one line in `drive-auth::session_store_from_env`.

### **Presence hub on Redis uses PUBSUB, not streams**

§14's brief described an in-process channel-per-workspace. The Redis impl maps each channel onto a Redis PUBSUB channel named `presence:{workspace_id}`. Subscribers are local SSE listeners; the publisher is whichever instance saw the heartbeat / audit event. No persistence — same amnesic semantics as the in-process hub.

We don't use Redis Streams because we don't need replay or consumer groups for presence.

### **Health-check + circuit-break on the optional services**

- Boot probe: connect + ping. If `DRIVE_REDIS_URL` is set but unreachable, **boot fails** — operator opted in, deserves the loud failure.
- Runtime: every call goes through a `tower` middleware that opens a circuit breaker on 5 consecutive failures over 10s, falls back to in-process for 60s, retries. Surface in `/api/admin/system` as `redis_status: healthy | circuit_open | unconfigured`.
- Same shape for OpenSearch.

## Locked-out decisions

- **Redis-as-cache-of-sqlite-rows.** Tempting (`User`, `Workspace` are read-heavy). Skipped: the moment we cache rows we have an invalidation surface, and the metadata DB is the cheapest piece of the stack anyway. Cache when measured, not before.
- **OpenSearch for audit log.** Different access pattern (time-series, write-heavy, range queries). v0.4 brief if ever needed; sqlite/postgres is fine until then.
- **OpenSearch-as-primary-storage.** No. SQL stays authoritative; OpenSearch is a derived view that can be rebuilt.
- **Embedded Tantivy/Lucene-on-disk.** Same trap as "Redis-as-cache" — adds a second source of truth, doesn't survive multi-node. If the operator's at >50 users they can run OpenSearch.
- **Multi-region Redis / OpenSearch clusters.** Operator concern, not Drive's. We document "any reachable URL works" and stop.

## Threat model

| Risk | Mitigation |
|---|---|
| **Redis password in DRIVE_REDIS_URL leaks via logs** | URL redactor middleware already redacts `Authorization`, `Cookie`, `?access_token=`. Add `redis://*` URL redaction (replace `:<pwd>@` with `:***@`). |
| **OpenSearch query injection via search box** | Use the OpenSearch client library's bound-parameter form (`Query::multi_match`), never string-concat into a query body. |
| **Index leaks data across workspaces** | Every indexed doc has `workspace_id`; every query has a `term` filter on `workspace_id ∈ caller's memberships`. No `_all` queries from user-facing endpoints. |
| **Stolen Redis credentials → presence forgery** | Presence events ride alongside the audit log; the audit log is authoritative + lives in SQL. A Redis attacker can spam events but can't fake history. |
| **Redis connection exhaustion DoS** | `bb8-redis` pool with `max_size` matching the worker count; presence subscribers reuse a single pubsub connection. |

## Config

```
DRIVE_REDIS_URL=redis://user:pass@host:6379/0          # opt-in, all four Redis-backed surfaces
DRIVE_OPENSEARCH_URL=https://user:pass@host:9200       # opt-in
DRIVE_OPENSEARCH_INDEX_PREFIX=drive_                   # default; lets a shared cluster host multiple Drives
DRIVE_OPENSEARCH_FILES_INDEX=drive_files               # explicit override (default: prefix + "files")
DRIVE_OPENSEARCH_NOTES_INDEX=drive_notes               # explicit override
DRIVE_INDEXER_BATCH_SIZE=200                           # bulk-API batch
DRIVE_INDEXER_INTERVAL_MS=2000                         # debounce
```

## Endpoints affected

| Endpoint | Today (in-process) | With Redis | With OpenSearch |
|---|---|---|---|
| `POST /api/auth/sign-in` | sqlite session | sqlite or `RedisStore` (env-picked) | unchanged |
| `POST /api/files` (any upload) | sqlite write + audit | + enqueue index job (if OS set) | indexed within `INTERVAL_MS` |
| `GET /api/search` | `LIKE '%q%'` | unchanged | OS query, filters, snippets |
| `GET /api/presence/{ws}` (§14) | in-process hub | Redis PUBSUB hub | unchanged |
| `POST /api/admin/reindex` | nothing (no index) | nothing | drop + rebuild all OS indexes |

## Implementation surface

Four modules + one boot wire-up, ~800 LOC:

- `crates/drive-cache/` (new) — thin Redis facade with the four traits. Default impls are in-process where they exist today.
- `crates/drive-search/` (new) — the `SearchBackend` trait + `SqlLikeSearch` (moved from `drive-http::search`) + `OpenSearchBackend`.
- `crates/drive-auth/src/session_store.rs` — picker between sqlite and Redis stores.
- `crates/drive-http/src/state.rs` — `HttpState` gains `Arc<dyn SearchBackend>` + `Arc<dyn PresenceHubBackend>` + the rate-limit trait swap.
- `crates/drive-bin/src/main.rs` — env-driven picker at boot, health probes, `tracing::info!` per-surface status.

The indexer worker (PoC) is a `tokio::spawn` in `drive-bin` that drains a `tokio::sync::mpsc` channel. Promotion to a dedicated job queue (sqlx-job, river, etc.) is a Phase 4 conversation.

## Test plan

- Compile-time: `drive-cache` + `drive-search` compile with both `redis` and `default-features = false`. Feature-gated tests for the Redis impls (only run in CI with a sidecar redis service).
- Integration: rate-limit + session + presence each have a Redis-backed test that uses `testcontainers-rs` to spin up a Redis. Skipped by default; CI runs them on the `infra` job.
- OpenSearch: ditto. `testcontainers-rs` spins up `opensearchproject/opensearch:2` on demand; integration tests for index, search, filters, reindex.
- Smoke: in-process impls keep their existing tests verbatim — the trait extraction must not break them.
- Two-instance: a script in `scripts/` brings up 2 Drive replicas + Redis + OpenSearch via `docker compose`, runs a Playwright test that signs in on one + sees presence on the other.

## When to ship

This brief is deferred until **the first operator outgrows in-process**. Concrete trigger: any of —

- `/api/admin/system` reports `rate_limit_buckets > 1000` for a sustained hour.
- An operator opens an issue saying they're running >1 replica.
- A real-world Drive starts dropping presence events (telemetry shows hub fan-out > 5ms p99).

Until then, the existing in-process defaults are correct.
