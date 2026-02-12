# AGENTS.md

## 1. Project Purpose
This repository is a Rust workspace for a tokenization/user-storage backend with:
- Keycloak-facing APIs (`KC`)
- Customer-facing registration/BFF APIs (`BFF`)
- Staff KYC APIs (`Staff`)

The executable entrypoint is `app/backend`, while HTTP serving logic lives in `crates/backend-server` (library crate).

## 2. High-Level Architecture

### 2.1 Workspace Crates
- `app/backend`
  - CLI binary (`serve`, `migrate`, `config`)
  - Starts `backend_server::serve(...)`
- `crates/backend-core`
  - shared config, errors, core types
- `crates/backend-server`
  - axum host + route dispatch to generated OA3 services
  - implements generated trait handlers
- `crates/backend-model`
  - DB row structs + DTO mapping helpers (`o2o`)
- `crates/backend-repository`
  - repository traits + PostgreSQL implementation
  - the only place where SQL queries should be authored
- `crates/backend-auth`
  - service/KC request contexts for generated services
- `crates/backend-id`
  - prefixed ID generation (`prefix + cuid1`)
- `crates/backend-migrate`
  - migration runner (`sqlx::migrate!`)
- `crates/gen_oas_*`
  - generated OpenAPI code (server/client)
  - treat as auto-generated artifacts

### 2.2 Runtime Routing
`crates/backend-server/src/lib.rs` dispatches by path prefix:
- `/v1/*` -> KC generated server
- `/api/registration/*` -> BFF generated server
- `/api/kyc/staff/*` -> Staff generated server
- `/health` -> simple health endpoint

## 3. Non-Negotiable Development Rules

1. Never hand-edit files under `crates/gen_*`.
2. Change API contracts in `openapi/*`, then regenerate generated crates.
3. Keep `backend-server` a library; launch from `app/backend`.
4. Do not mix SQL in service handlers:
   - `backend-server` should call repository traits only.
   - SQL belongs in `crates/backend-repository`.
5. Use repository pattern interfaces (base and domain traits) from:
   - `crates/backend-repository/src/traits.rs`
6. ID policy:
   - Never use UUID.
   - Use explicit prefix + CUID IDs from `backend-id`.

## 4. ID Strategy (Mandatory)

Use prefixed CUID values:
- `usr_*` for users
- `dvc_*` for device records
- `apr_*` for approvals
- `sms_*` for SMS challenge hashes

Helper functions:
- `backend_id::user_id()`
- `backend_id::device_id()`
- `backend_id::approval_id()`
- `backend_id::sms_hash()`

## 5. Device Binding Safety Requirements

Device key material uniqueness must be enforced on both `jkt` and `device_id`:
- check at precheck time for UX guidance
- check again at bind/insert time for race safety and direct API safety
- handle DB conflict paths gracefully and re-check binding ownership

## 6. Auth Model

Current implementation has no bearer token gate in server config or runtime checks.

Config no longer includes `server.api.auth.*`.
Request context auth objects are populated in `backend-auth` for generated service compatibility.

## 7. Config Model

Primary local config file: `config/default.yaml`.

Main sections:
- `server.api.address`, `server.api.port`, `server.api.tls.*`
- `database.url`, `database.pool_size`
- `oauth2.jwks_url`
- `aws.region`, `aws.s3.*`, `aws.sns.*`

`backend_core::Config` is the source-of-truth model.

## 8. OpenAPI & Code Generation Workflow

1. Update OpenAPI spec(s) in `openapi/`.
2. Regenerate code:
   - `docker compose -f compose.yml run --rm generate-code`
3. Validate:
   - `cargo check --workspace`
4. Do not patch generated output manually.

## 9. Migrations & Schema

Apply migrations with:
- `cargo run -p backend -- migrate -c config/default.yaml`

Schema uses text IDs (no UUID) for:
- `devices.id`
- `approvals.request_id`
- `sms_messages.id`
- `kyc_documents.id`

## 10. Local Development Commands

- Start dependencies:
  - `just up-single postgres`
  - `just up-single keycloak-26`
- Build/check:
  - `cargo check --workspace`
- Migrate:
  - `just dev migrate -c config/default.yaml`
- Serve:
  - `just dev serve -c config/default.yaml`

## 11. Migration File Duplication Note

There are currently two naming styles for the same migration content:
- directory-style:
  - `migrations/2026-02-03-000001_init_authz/up.sql`
  - `migrations/2026-02-03-000001_init_authz/down.sql`
- flat-file style:
  - `migrations/20260203000001_init_authz.up.sql`
  - `migrations/20260203000001_init_authz.down.sql`

Keep both sets in sync unless/until a deliberate migration-format cleanup is performed.

## 12. Change Checklist

Before finishing a change:
1. `cargo check --workspace` passes.
2. If schema changed, migration files are updated consistently.
3. If API changed, OpenAPI was updated and generated crates regenerated.
4. No manual edits under `crates/gen_*`.
5. Service layer contains no raw SQL.
