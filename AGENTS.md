# AGENTS.md

## Purpose
Tokenization/user-storage backend with three HTTP surfaces:
- KC: `/v1/*`
- BFF: `/api/registration/*`
- Staff: `/api/kyc/staff/*`

`app/backend` starts the server; `crates/backend-server` is a library crate.

## Core Architecture
- Runtime is native `axum` (no generated `swagger` runtime dispatch).
- Layering is strict: `controller -> service -> repository`.
- Controllers: `crates/backend-server/src/api.rs`
- Services: `crates/backend-server/src/services.rs`
- Repository: `crates/backend-repository/src/pg.rs`

## Crate Roles
- `backend-core`: config + shared `Error`/`Result`
- `backend-auth`: axum middleware/extractors (request context/auth)
- `backend-server`: router/controllers/services/state/retry worker
- `backend-repository`: SQLx-Data repository layer
- `backend-model`: `FromRow` DB structs + `o2o` DTO mapping
- `backend-id`: prefixed CUID ID generation
- `backend-migrate`: migration runner
- `gen_oas_*`: generated OA3 models/interfaces (never edit manually)

## Hard Rules
1. Never hand-edit `crates/gen_*`.
2. OpenAPI changes happen in `openapi/*` then regenerate.
3. Keep SQL in repository crate only.
4. Do not use `sqlx::query*` directly; use SQLx-Data `#[repo]` + `#[dml]`.
5. Use one macro trait (`PgSqlRepo`) and one concrete API (`PgRepository` inherent methods).
6. Use `backend_core::Error` only; avoid scattered custom error mapping.
7. Keep server config source in `backend-core::Config` only.
8. Keep `backend-server` as library; app binary wires and starts it.

## IDs (Mandatory)
Always use prefix + CUID from `backend-id`:
- `usr_*` users
- `dvc_*` devices
- `apr_*` approvals
- `sms_*` SMS hashes

Never use UUID for backend IDs.

## Device Binding Safety
- Device uniqueness is on both `device_id` and `jkt`.
- Enforce at precheck and bind time.
- Bind must re-check and handle unique-conflict races deterministically.

## Auth
- Auth logic/middleware lives in `backend-auth`.
- `backend-server` composes middleware only; no swagger context types.

## Caching
- In-process cache uses `lru`.
- Redis is available in compose for distributed/shared cache use.

## Migrations
Two migration file naming schemes currently exist for the same migration content:
- `migrations/2026-02-03-000001_init_authz/{up.sql,down.sql}`
- `migrations/20260203000001_init_authz.{up.sql,down.sql}`

Keep both synchronized until migration format cleanup.

## Validation Checklist
Before finalizing:
1. `cargo check --workspace`
2. No runtime use of `swagger` or generated `server::Service`
3. No direct `sqlx::query*` usage
4. No manual edits under `crates/gen_*`
