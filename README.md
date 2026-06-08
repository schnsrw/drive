<div align="center">

<img src="./logo.svg" alt="Casual Drive" width="420" />

**Open-source, self-hosted Drive that opens `.xlsx` and `.docx` in the browser. A drop-in alternative to Google Drive or OneDrive — your storage, your editors, your server.**

[![CI](https://img.shields.io/github/actions/workflow/status/schnsrw/drive/ci.yml?branch=main&label=CI)](./.github/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-195%20green-brightgreen)](#)

[Live demo](https://drive.schnsrw.live/demo) &nbsp;·&nbsp; [Docs](https://drive.schnsrw.live/docs/install) &nbsp;·&nbsp; [Architecture](./docs/ARCHITECTURE.md) &nbsp;·&nbsp; [Pipeline](./PIPELINE.md)

</div>

---

Casual Drive is a small, sharp file manager built around two ideas:

1. **Your files belong on your server.** Filesystem, S3, MinIO, Cloudflare R2, Backblaze B2 — pick any. Per-workspace bring-your-own-bucket too.
2. **Office files belong in the browser.** Click a `.xlsx` and it opens in [Casual Sheet](https://github.com/schnsrw/sheet); click a `.docx` and it opens in [Casual Document](https://github.com/schnsrw/document) — browser-only via the editor SDKs by default, or via [WOPI](https://learn.microsoft.com/en-us/microsoft-365/cloud-storage-partner-program/online/) when an editor server is configured for real-time co-editing.

One Rust binary, one Docker container, a polished React SPA, and a marketing site that doubles as live documentation.

## Quickstart

```bash
docker run -d --name drive \
  -p 8080:8080 \
  -v $HOME/drive-data:/data \
  -e DRIVE_BIND=0.0.0.0:8080 \
  -e DRIVE_APP_ORIGIN=https://drive.your-server \
  -e DRIVE_USERCONTENT_ORIGIN=https://usercontent-drive.your-server \
  -e DRIVE_STORAGE_BACKEND=fs \
  -e DRIVE_FS_ROOT=/data \
  ghcr.io/schnsrw/casual-drive:latest
```

Visit `https://drive.your-server`, complete the one-time admin setup, upload a file, click it. That's the demo. Full env-var matrix at <https://drive.schnsrw.live/docs/configuration>.

## What it does

| Surface | Feature |
|---|---|
| **Files** | Grid + list views, search, sort, drag-to-upload, multi-select, context menus, breadcrumbs, trash + restore, inline previews for images / PDFs / video / audio / text / markdown. |
| **Notes / Wiki** | Workspace-scoped pages with markdown source + live preview, `[[wiki-link]]` backlinks, drag-to-reorder tree, search across title + body. |
| **Editor handoff** | Click `.xlsx` → opens in Casual Sheet; click `.docx` → opens in Casual Document. WOPI access tokens, 30-min locks. |
| **Sharing** | Per-file share links with optional password (Argon2id) and expiry. Stripped-chrome recipient page. |
| **Cmd-K** | One keyboard surface for files + notes + nav. ⌘K from anywhere. |
| **Workspaces** | Personal (auto-created, untransferable) + Team workspaces with Owner/Member roles and atomic ownership transfer. |
| **Per-workspace storage** | Bring-your-own S3 / MinIO / R2 / B2 bucket. AES-256-GCM secret envelope, SSRF guard, test-connection flow. |
| **Quotas + admin** | Per-user storage caps, in-app quota upgrade requests, admin user-management UI, audit feed. |
| **Direct upload** | Files ≥ 8 MiB on S3-compatible backends PUT straight to the bucket, bypassing the Drive process. |
| **Server thumbnails** | 96 / 256 / 1024 px PNG thumbnails generated lazily on first access. Images render in-process; video via the sandboxed `drive-thumb-worker` subprocess (ffmpeg-CLI). |
| **OIDC sign-in** | Authorization Code + PKCE against any compliant IdP. Optional `DRIVE_ALLOW_PASSWORD_AUTH=false` to hide the password form once SSO is wired. |
| **Settings + Activity + Admin** | Full surfaces, real data, with stubs ("Coming in v0.2 — …") only for features that haven't shipped. |

## What's locked in v0

Two-origin model, WOPI handoff, OpenDAL storage, tower-sessions, Argon2id passwords, Rust 1.85 + Axum 0.8, React 19 + Vite 7 + Tailwind v4, Astro 5 for the marketing site. Reopening any of these requires new research + a synthesis update — see [`CLAUDE.md`](./CLAUDE.md).

## What's deferred

MS365 / Office Online federation, presence (avatar stack + file-row dots), PDF thumbnails (needs pdfium-render in the worker), post-finalize magic-byte sniff for direct uploads, resumable + multipart uploads, EXIF strip, server-mediated invitations + email, Pagefind docs search, i18n. See [`PIPELINE.md`](./PIPELINE.md) for the table.

## Repo layout

```
drive/
  crates/                Production Rust workspace
    drive-core/          Domain types, Config, errors
    drive-db/            SQLx repos + migrations (SQLite + Postgres portable)
    drive-storage/       OpenDAL facade, BYO secret envelope, thumbnail worker
    drive-wopi/          WOPI host (7 endpoints, lock state)
    drive-auth/          Sessions, Argon2id, share links
    drive-http/          Axum router, two-origin middleware, every API surface
    drive-bin/           Binary entry point
  web/                   React 19 SPA, embedded into the binary via rust-embed
  marketing/             Astro 5 site (drive.schnsrw.live) + the /demo SPA
  docs/
    ARCHITECTURE.md      System-level architecture
    research/            12 grounded research briefs + synthesis
    ux/                  17 surface specs and numbered flows
  .github/workflows/     CI: fmt, clippy, audit, deny, tests; Pages deploy
  PIPELINE.md            Single source of truth for what ships + status
  CLAUDE.md              Working rules for AI assistants in this repo
```

## Build + dev loop

```bash
# Backend
cargo run -p drive               # Rust binary on :8080

# SPA dev server (HMR; proxies /api to the backend)
cd web && pnpm install && pnpm dev

# Marketing site
cd marketing && pnpm install && pnpm dev
```

Required env for `cargo run -p drive`:

```
DRIVE_BIND=127.0.0.1:8080
DRIVE_APP_ORIGIN=http://127.0.0.1:8080
DRIVE_USERCONTENT_ORIGIN=http://127.0.0.1:18090
DRIVE_DB_URL=sqlite:///tmp/drive.db
DRIVE_STORAGE_BACKEND=fs
DRIVE_FS_ROOT=/tmp/drive-files
DRIVE_SESSION_SECRET=<32+ bytes>
DRIVE_WOPI_HMAC_SECRET=<32 bytes>
DRIVE_SIGNED_URL_HMAC_SECRET=<32 bytes>
DRIVE_ADMIN_USER=admin
DRIVE_ADMIN_PASSWORD_HASH=<argon2id$...>
```

Full env-var contract in `.env.example` and on the docs site.

## CI gates

Every PR runs `cargo fmt --check`, `cargo clippy -- -Dwarnings`, `cargo test --workspace`, `cargo audit --deny warnings`, `cargo deny check`. Marketing site has its own Lighthouse CI on landing + `/docs/install` with hard-fail thresholds (Performance / Accessibility / SEO ≥ 0.95 mobile profile).

## Demo

- **Live demo** (in-memory, no backend, resets on reload): <https://drive.schnsrw.live/demo>
- **Marketing site** (install + configuration + architecture + contributing docs): <https://drive.schnsrw.live>

The same SPA bundle is served in both places — the demo just runs against a localStorage-backed shim. Drop your own files in, sign in as `demo` / `demo`, click around.

## Contributing

1. Read [`CLAUDE.md`](./CLAUDE.md) — the five inviolable rules + locked decisions.
2. Read the [Contributing docs](https://drive.schnsrw.live/docs/contributing) and the relevant `docs/research/` brief for whatever area you're touching.
3. Open an issue describing what you'd like to take on before sending a PR.
4. PRs must pass the CI gates above. UI work must honour the [10 polish commandments](./docs/research/04-polish-principles.md) — every commandment-break needs explicit justification in the PR description.

## License

Apache-2.0 — see [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE).

## Sister projects

- [Casual Sheet](https://github.com/schnsrw/sheet) — self-hosted `.xlsx` editor.
- [Casual Document](https://github.com/schnsrw/document) — self-hosted `.docx` editor.
- [Casual Office](https://schnsrw.live) — umbrella site.

Drive is the file-centric front door that wraps them into a coherent suite.
