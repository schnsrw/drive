# 15 — Bring-your-own storage surface

Companion to `docs/research/08-byo-storage.md`. Settings → Storage → Workspace bucket card. Owner-only on Team workspaces.

> **Shipped:** schema + storage registry + 5 owner-only endpoints + `WorkspaceStorageCard` under Settings → Storage. Per-workspace S3/MinIO/R2/B2 with AES-256-GCM secret envelope (`DRIVE_STORAGE_SECRET_KEY`, 64 hex chars) and SSRF guard. New uploads route via the workspace's BYO adapter when configured; existing files keep their `storage_id` pointer.

## Flow

1. Owner of a Team workspace opens `Settings → Workspace → Storage`.
2. Card shows current storage: either "Server default" or the BYO summary (provider · bucket · region · last-tested badge).
3. Owner picks "Configure custom storage". Inline form opens.
4. Form fields: Provider (S3 / MinIO / R2 / Backblaze B2) · Bucket · Region · Endpoint (only when provider ≠ S3) · Access key ID · Secret access key.
5. Owner clicks "Test connection". Server runs put/stat/delete on a temp key. Result inline: ✓ "Connected in 142 ms" or ✗ "AccessDenied: …".
6. Owner clicks "Save". Server tests once more, encrypts the secret, writes the row, emits audit. Card flips to "BYO active" state.
7. **From this point** new uploads in this workspace land on the BYO bucket. Existing files keep their original storage pointer — the card states this explicitly.
8. Owner can: "Replace credentials" (re-enter secret only, bumps `key_version`), "Remove custom storage" (returns to server default for new files), or "Re-test" (idempotent).

## Surface — Server-default state (no BYO)

```
┌─ Storage ───────────────────────────────────────────────────────────┐
│  Backend in use                                                     │
│  Server default · Filesystem at /data                               │
│                                                                     │
│  ──────────────────────────────────────────────────────────────     │
│                                                                     │
│  Bring your own bucket                                              │
│  Point this workspace at an S3, MinIO, R2, or Backblaze B2 bucket   │
│  you control. New uploads will land there. Existing files stay      │
│  where they are.                                                    │
│                                                                     │
│  [ Configure custom storage ]                                       │
└─────────────────────────────────────────────────────────────────────┘
```

## Surface — BYO active state

```
┌─ Storage ───────────────────────────────────────────────────────────┐
│  Backend in use                                                     │
│  Custom · S3 · my-team-bucket · us-east-1                           │
│  ✓ Connected · tested 12 min ago                                    │
│                                                                     │
│  Endpoint   https://s3.amazonaws.com (default)                      │
│  Access ID  AKIA…7K3Q                                               │
│  Secret     ●●●●●●●●●● (encrypted at rest)                          │
│                                                                     │
│  [ Re-test connection ]  [ Replace credentials ]  [ Remove ]        │
└─────────────────────────────────────────────────────────────────────┘
```

## Surface — Configure form (open inline)

```
┌─ Configure custom storage ──────────────────────────────────────────┐
│  Provider     ( ● S3   ○ MinIO   ○ Cloudflare R2   ○ Backblaze B2 ) │
│                                                                     │
│  Bucket       [ my-team-bucket                                    ] │
│  Region       [ us-east-1                                         ] │
│  Endpoint     [ https://minio.internal:9000                       ] │   ← shown only when MinIO/R2/B2
│  Access ID    [ AKIA…                                             ] │
│  Secret       [ ●●●●●●●●●●●●●●●●●●●●●●●●●●●●                       ] │
│                                                                     │
│  [ Test connection ]                                                │
│                                                                     │
│  ✓ Connected in 142 ms · ready to save                              │
│                                                                     │
│                              [ Cancel ]   [ Save ]                  │
└─────────────────────────────────────────────────────────────────────┘
```

## Backend contract

| Method | Path | Body / Result |
|---|---|---|
| `GET` | `/api/workspaces/{id}/storage` | Owner-only. `{ kind: "default" \| "byo", config?: {...} }`. Never includes the secret. |
| `POST` | `/api/workspaces/{id}/storage/test` | Owner-only. Dry-run; does not persist. Body has full creds. Body NEVER logged. Returns `{ ok, latency_ms, error? }`. |
| `PUT` | `/api/workspaces/{id}/storage` | Owner-only. Body has full creds. Server tests, encrypts secret, writes row, audits. 422 if test fails. |
| `PATCH` | `/api/workspaces/{id}/storage/credentials` | Owner-only. Replaces only the secret (bumps `key_version`). |
| `DELETE` | `/api/workspaces/{id}/storage` | Owner-only. Removes BYO; new uploads go to server default. Existing files keep their pointer. |

All four POST/PUT/PATCH/DELETE paths refuse on Personal workspaces with 409.

## States per UI surface

- **Loading:** card-shaped skeleton (commandment 6).
- **Error (network):** soft amber banner, "Couldn't reach the API. Retry."
- **Test running:** "Test connection" button shows spinner + "Testing…". Form is disabled. The test endpoint has a hard 12s timeout server-side; SPA mirrors it.
- **Test failed:** red inline result with the exact provider error string (sanitised — no creds in the message).
- **Save failed (post-test):** same banner.
- **Replace credentials open:** only the Secret + Test/Save buttons; other fields are read-only.
- **Remove confirm:** modal — "Files already uploaded will continue to live on this bucket. New uploads will go to the server default. Continue?"

## Permissions matrix

| Role | View | Configure | Replace creds | Remove |
|---|---|---|---|---|
| Owner | Yes | Yes | Yes | Yes |
| Member | No (403 — card hidden from UI too) | — | — | — |
| Personal workspace | Always = server default; whole card hidden | — | — | — |

## Audit log entries

Surface them in `Settings → Audit log` and `Admin → Activity`:

- `Owner configured S3 storage for "Engineering" — bucket my-team-bucket, us-east-1`
- `Owner tested S3 connection for "Engineering" — ✓ 142 ms`
- `Owner replaced credentials for "Engineering" — key_version 2`
- `Owner removed custom storage for "Engineering" — returned to server default`

## Out of scope (v0.2+)

- Per-file storage migration UI ("move N files to the new bucket").
- Multi-target storage (one workspace, several buckets, routed by file type).
- IAM-role-based S3 auth (instance metadata). v0 is access-key only.
- KMS-backed master key rotation.
- A "Test from your browser" path that uses presigned URLs and skips the server hop.
