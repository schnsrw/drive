<div align="center">

<img src="./logo.svg" alt="Casual Drive" width="420" />

**Open-source self-hosted file manager that opens `.xlsx` and `.docx` in real-time, in-browser editors — an alternative to Google Drive and Microsoft OneDrive you run on your own server.**

[![CI](https://img.shields.io/github/actions/workflow/status/schnsrw/drive/ci.yml?branch=main&label=CI)](./.github/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](./LICENSE)
[![Status](https://img.shields.io/badge/status-Phase%201%20walking%20skeleton-yellow)](./PLAN.md)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange?logo=rust)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-60%20passing-brightgreen)](#)
[![Polish bar](https://img.shields.io/badge/polish-Things%203%20%E2%80%A2%20Linear%20%E2%80%A2%20Raycast-blueviolet)](./docs/research/04-polish-principles.md)

[Architecture](./docs/ARCHITECTURE.md) &nbsp;·&nbsp; [Plan](./PLAN.md) &nbsp;·&nbsp; [UX Flows](./docs/ux/01-flows.md) &nbsp;·&nbsp; [Surface Spec](./docs/ux/02-surface.md) &nbsp;·&nbsp; [Research](./docs/research/00-synthesis.md)

</div>

---

Casual Drive is the file-centric front door for the [Casual Office](https://schnsrw.live) suite. A single Rust binary, a single Docker container, four pluggable storage backends, a polished web UI. Files live in *your* storage (filesystem / S3 / MinIO); editing happens in our [Casual Sheets](https://github.com/schnsrw/sheets) and [Casual Editor](https://github.com/schnsrw/docx) — wired together via the [WOPI](https://learn.microsoft.com/en-us/microsoft-365/cloud-storage-partner-program/online/) protocol.

> **Status — Phase 1 walking skeleton.** Research, planning, UX/surface specs, and four Phase-0 spikes are complete. The production crates are stubbed and runnable: Drive boots, serves both origins, signs URLs, validates WOPI tokens, and rejects cross-origin requests with 421. SPA wiring, SQLite schema, admin auth, and the full file API are the next layers — see [`PLAN.md`](./PLAN.md).

---

## What's inside

### File manager (planned for v0)

- **Macos-app-grade polished UI** — restraint over decoration; sub-100 ms feedback on every direct manipulation; full keyboard navigation; optimistic UI; light + dark themes day one. The polish bar is Things 3 / Linear / Raycast tier — see [`docs/research/04-polish-principles.md`](./docs/research/04-polish-principles.md).
- **Upload** (button, drag-drop, folder upload — multi-MB streaming with real progress).
- **Browse** root → nested folders with breadcrumbs, sort, list + grid views.
- **Open `.xlsx` / `.docx` in the editor** via WOPI handoff (new tab, same tab, or read-only).
- **Rename, create folder, move (drag + picker), trash + restore.**
- **Search** via Cmd-K command palette.
- **Share-links** (Phase 2) with optional password, expiry, view-only / edit perms.
- **Download** single file or selection-as-zip.

### Infrastructure (the unglamorous-but-important parts)

- **Single static Rust binary**, ~20–40 MB Debian-slim image, runs on a $5 VPS.
- **Pluggable storage** behind one `Storage` facade — filesystem, in-memory (tests), AWS S3, MinIO. Add Azure / GCS / B2 later by changing one line.
- **Two-origin security model** — app and user-uploaded content served from different registrable origins so a malicious upload can never XSS the app. Boot refuses to start if origins match in production.
- **WOPI 1.0 host** — the seven required endpoints, 30-min lock semantics, 10-min HMAC access tokens, per-call file-id scoping.
- **Single-tenant admin auth** for v0 — Argon2id passwords, server-side sessions, `__Host-` cookie, rate-limited login. Multi-user OIDC slot reserved for Phase 3.
- **OWASP-grounded security baseline** — magic-byte content sniffing, opaque storage IDs, strict CSP per origin, `nosniff`, `Content-Disposition: attachment`, signed URLs with constant-time verify, `cargo audit` + `cargo deny` in CI.

## What's planned out of scope

- MS365 / Office Online federation (proof-key RSA hook reserved).
- Multi-user accounts beyond a single admin (Phase 3).
- Casual Slides (`.pptx`) — MIME slot reserved; wiring lands when the Slides editor does.
- Macro-enabled Office files (accepted as opaque blobs; never auto-opened).
- Sync clients / desktop apps (browser-only).

## Quick links

| Topic | Doc |
|---|---|
| Phased delivery plan | [`PLAN.md`](./PLAN.md) |
| How Claude Code works in this repo | [`CLAUDE.md`](./CLAUDE.md) |
| Architecture | [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) |
| Research briefs (WOPI, auth, storage, polish, Rust stack, security) | [`docs/research/`](./docs/research/) |
| UX flows | [`docs/ux/01-flows.md`](./docs/ux/01-flows.md) |
| UI surface spec | [`docs/ux/02-surface.md`](./docs/ux/02-surface.md) |
| Phase 0 spike write-ups | [`docs/spikes/`](./docs/spikes/) |
| Changelog | [`CHANGELOG.md`](./CHANGELOG.md) |

## Build and run (today)

The Phase 0 spikes are runnable now; the production binary is a stub until Phase 1 fills in the crates.

```bash
# Workspace compiles + clippy clean + (stub) binary runs
cargo build --workspace
cargo run -p drive

# Spike conformance
cargo test --manifest-path spikes/01-storage/Cargo.toml
cargo test --manifest-path spikes/02-wopi-host/Cargo.toml
cargo test --manifest-path spikes/04-two-origin/Cargo.toml
```

Once Phase 1 lands, the production flow is:

```bash
cp .env.example .env
# Fill in DRIVE_ADMIN_PASSWORD_HASH and three 32-byte secrets

docker compose -f docker-compose.dev.yml up --build
# Drive on http://127.0.0.1:8080, MinIO console on http://127.0.0.1:9001
```

## Project layout

```
.
├── crates/                # Production workspace
│   ├── drive-core/        # Domain types, IDs, errors
│   ├── drive-storage/     # Storage facade over OpenDAL
│   ├── drive-wopi/        # WOPI host
│   ├── drive-auth/        # Sessions + share-links
│   ├── drive-http/        # Router + two-origin middleware + SPA mount
│   └── drive-bin/         # Binary entry point
├── spikes/                # Phase 0 proof-of-concept code (outside workspace)
│   ├── 01-storage/
│   ├── 02-wopi-host/
│   └── 04-two-origin/
├── web/                   # SPA (added in Phase 1)
├── docs/
│   ├── ARCHITECTURE.md
│   ├── research/          # 6 grounded research briefs + 00 synthesis
│   ├── ux/                # Flows + surface spec
│   └── spikes/            # Spike write-ups
├── .github/workflows/     # CI: fmt, clippy, test, spikes, audit, deny, docker
├── Cargo.toml             # Workspace
├── Dockerfile             # Multi-stage cargo-chef
├── docker-compose.dev.yml # Drive + MinIO
├── deny.toml              # cargo-deny policy
├── rustfmt.toml           # Style
├── .env.example           # Configuration contract
├── CHANGELOG.md
├── CLAUDE.md              # Claude Code working rules
├── PLAN.md                # Phased delivery
├── LICENSE                # Apache-2.0
└── NOTICE
```

## Contributing

The repo is in Phase 0 (planning + spikes). Phase 1 work begins after the planning package is reviewed. If you want to contribute:

1. Read [`CLAUDE.md`](./CLAUDE.md) and [`PLAN.md`](./PLAN.md) first.
2. Open an issue describing what you'd like to take on before sending a PR.
3. PRs must pass `cargo fmt --check`, `cargo clippy -- -Dwarnings`, `cargo test --workspace`, `cargo audit`, `cargo deny check`.
4. UI work must honour the **10 commandments** at the bottom of [`docs/research/04-polish-principles.md`](./docs/research/04-polish-principles.md) — every commandment-break in a PR has to be explicitly called out and justified.

## License

Apache-2.0 — see [`LICENSE`](./LICENSE) and [`NOTICE`](./NOTICE).

## Sister projects

- [Casual Sheets](https://github.com/schnsrw/sheets) — self-hosted `.xlsx` editor.
- [Casual Editor](https://github.com/schnsrw/docx) — self-hosted `.docx` editor.
- [Casual Office](https://schnsrw.live) — umbrella site.

Drive is the file-centric front door that wraps them into a coherent suite.
