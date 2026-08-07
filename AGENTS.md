# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Commands

### Rust Backend
```bash
cargo run                    # Dev mode
cargo build --release        # Release build
cargo test                   # All tests
cargo test --lib <test_name> # Single test
```

### Frontend
```bash
cd frontend && npm install && npm run dev    # Dev server
cd frontend && npm run build                 # Builds to frontend/dist/
```

### Docker
```bash
# Build ARM64 image (requires pre-built binary)
cargo build --release --target aarch64-unknown-linux-musl
podman build --platform linux/arm64 -t docker.io/givenge/reader-rust:${TAG}-aarch64 -f Dockerfile .

# Build x86_64 image (requires pre-built binary)
cargo build --release --target x86_64-unknown-linux-musl
podman build --platform linux/amd64 -t docker.io/givenge/reader-rust:${TAG}-x86_64 -f Dockerfile.x86 .
```

Dockerfiles do NOT compile Rust in-container. Build the binary on the host first, then copy it.

### Docker Release (Podman)
- Default release repository: `docker.io/givenge/reader-rust`
- Default rolling tags:
- `latest` -> x86_64
- `latest-aarch64` -> arm64
- Build commands must explicitly set platform:
- x86_64: `podman build --platform linux/amd64 ... -f Dockerfile.x86 .`
- arm64: `podman build --platform linux/arm64 ... -f Dockerfile .`
- For any “发布版本 / 发布docker镜像 / release版本” request, run `./scripts/release.sh` by default.
- If user does not specify version, auto-bump patch from latest tag (`vX.Y.Z -> vX.Y.(Z+1)`).
- Full end-to-end workflow is in `/RELEASE_WORKFLOW.md`.
- Docker-specific details remain in `/DOCKER_RELEASE.md`.

## Configuration

Loaded from `.env` file (via `dotenvy`) or environment variables. Separator is `__` for nested keys. See `.env.example` for all options.

Key settings:
- `SERVER_HOST` / `SERVER_PORT` — default `0.0.0.0:8080`
- `DATABASE_URL` — SQLite path, default `sqlite:storage/reader.db?mode=rwc`
- `WEB_ROOT` — static files path, default `frontend/dist`
- `SECURE` / `SECURE_KEY` — security mode toggle
- `INVITE_CODE` — registration gate
- `USER_LIMIT` / `USER_BOOK_LIMIT` — default 50 / 2000
- `LOG_LEVEL` — default `info`
- `REQUEST_TIMEOUT_SECS` — default 15

## Architecture

Rust implementation of "阅读3.0" — a book source reading API server.

### Module Structure
- `src/api/` — HTTP handlers & routing (axum), routes under `/reader3/*`
- `src/service/` — Business logic (book search, sources, users)
- `src/parser/` — Content extraction engine with rule-based parsing
- `src/crawler/` — HTTP fetching via reqwest
- `src/model/` — Data structures (BookSource, rules)
- `src/storage/` — SQLite (sqlx), file cache (MD5 key), filesystem ops
- `src/app/` — Config, logging, server setup
- `src/error/` — Error types
- `src/util/` — Utilities

### Request Flow
`api/handlers` → `service/` → `crawler/` (fetch) → `parser/rule_engine` (parse with BookSource rules) → JSON response

### Rule Parsing
`RuleEngine` auto-detects parsing mode:
- **CSS selectors** — default for HTML (`.class`, `#id`, `tag`)
- **JSONPath** — auto-detected for JSON (`$.data.list`)
- **XPath** — lines starting with `/` or `./`
- **JavaScript** — `js:` or `@js:` prefix (rquickjs)
- **Regex** — starts with `:`
- Explicit prefixes: `@css:`, `@json:`, `@xpath:`, `@regex:`

### Book Source Format
JSON objects with `bookSourceUrl`, `bookSourceName`, `searchUrl`/`exploreUrl` (with `${key}` placeholders), and `ruleSearch`/`ruleBookInfo`/`ruleToc`/`ruleContent` parsing rules.

## Important Notes

- **Frontend app**: `frontend/` is the Vue 3 + Vite frontend. Docker images use `frontend/dist/`.
- **`/storage/` is gitignored**: Contains user data and SQLite DB.
- **No tests currently**: `cargo test` will pass but there are no test files written yet.
