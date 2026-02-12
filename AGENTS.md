# AGENTS.md

## Purpose
Contributor guide for humans and coding agents working in this repository.

## Project Summary
- Rust workspace for a tokenization/user-storage backend.
- Main runnable binary: `app/backend` (`backend` CLI).
- HTTP serving is implemented in `crates/backend-server` as a library and started by `app/backend`.
- OpenAPI-driven server/client crates are generated into `crates/gen_*`.

## Workspace Map
- `app/backend`: CLI entrypoint (`serve`, `migrate`, `config`).
- `crates/backend-core`: shared config/types/errors.
- `crates/backend-server`: axum host + adapters to generated OA3 server traits.
- `crates/backend-model`: DB row/domain mapping DTOs (`o2o`-based mapping).
- `crates/backend-migrate`: sqlx migration runner.
- `crates/gen_oas_server_{kc,bff,staff}`: generated server interfaces/models (do not hand-edit).
- `crates/gen_oas_client_cuss_registration`: generated API client (do not hand-edit).
- `openapi/`: source OpenAPI specs.
- `migrations/`: SQL migrations.
- `config/default.yaml`: local default configuration.

## Non-Negotiable Rules
1. Never manually edit files under `crates/gen_*`.
2. To change generated APIs/models, edit `openapi/*.json` and regenerate.
3. Keep `crates/backend-server` as a library crate; start it from `app/backend`.
4. Keep config adapters derived from `backend_core::Config` (single source of truth).
5. Prefer `o2o` mappings for DTO boundaries; avoid ad-hoc manual mapping unless transformation is non-trivial.

## Local Development
- Start dependencies:
  - `just up-single postgres`
  - `just up-single keycloak-26`
- Run backend CLI:
  - `just dev -h`
  - `just dev migrate -c config/default.yaml`
  - `just dev serve -c config/default.yaml`
- Build:
  - `cargo check --workspace`
  - `just prepare`

## Code Generation Workflow
1. Modify OpenAPI files in `openapi/`.
2. Regenerate:
   - `docker compose -f compose.yml run --rm generate-code`
3. Verify:
   - `cargo check --workspace`
4. Do not patch generated output directly.

## Database & Migrations
- Apply migrations via CLI:
  - `cargo run -p backend -- migrate -c config/default.yaml`
- Current base schema lives in:
  - `migrations/2026-02-03-000001_init_authz/up.sql`
  - `migrations/2026-02-03-000001_init_authz/down.sql`
- DB-facing structs are in `crates/backend-model/src/db.rs`.

## Runtime Behavior Notes
- Route dispatch in `backend-server`:
  - KC: `/v1/*`
  - BFF: `/api/registration/*`
  - Staff: `/api/kyc/staff/*`
- Auth:
  - BFF/Staff use static bearer token validation from `server.api.auth.static_bearer_tokens`.
  - KC routes are unauthenticated.
- AWS settings come from `Config.aws`:
  - S3 presign upload intent for KYC documents.
  - SNS SMS publish with in-process retry worker.

## Configuration
- Primary file: `config/default.yaml`.
- Key sections used by backend-server:
  - `server.api.address`, `server.api.port`, `server.api.tls.*`
  - `server.api.auth.static_bearer_tokens`
  - `database.url`, `database.pool_size`
  - `aws.region`, `aws.s3.*`, `aws.sns.*`

## Change Checklist
1. `cargo check --workspace` passes.
2. If schema changed, migration up/down updated and migration applied locally.
3. If API contract changed, OpenAPI updated and `crates/gen_*` regenerated.
4. No manual edits in generated crates.
5. Keep changes scoped; avoid unrelated refactors.
