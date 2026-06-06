# syntax=docker/dockerfile:1.7
# Multi-stage build:
#   1. web-build  → produces web/dist/ (rust-embed needs this)
#   2. planner    → cargo-chef recipe for cached dep builds
#   3. builder    → cooks deps, copies SPA artifacts, compiles drive
#   4. runtime    → small Debian image with just the binary
# See: https://github.com/LukeMathWalker/cargo-chef

# ─── Web: build the SPA bundle (needed by drive-http rust-embed) ─────────
FROM node:22-bookworm-slim AS web-build
WORKDIR /web
RUN corepack enable && corepack prepare pnpm@9.15.0 --activate
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ─── Plan: extract a dependency-only recipe so deps cache independently ──
FROM rust:1.90-slim AS chef
WORKDIR /app
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/* \
 && cargo install cargo-chef --locked --version 0.1.71

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ─── Build: cook deps from cache, then compile our code ───────────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
COPY --from=web-build /web/dist ./web/dist
RUN cargo build --release --bin drive

# ─── Runtime: small image, no toolchain ───────────────────────────────────
FROM debian:trixie-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --no-create-home --shell /usr/sbin/nologin drive

COPY --from=builder /app/target/release/drive /usr/local/bin/drive

USER drive
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/drive"]
