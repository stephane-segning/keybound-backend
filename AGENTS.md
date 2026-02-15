# AGENTS.md

## Purpose
Tokenization/user-storage backend with three HTTP surfaces:
- KC: `/v1/*`
- BFF: `/api/registration/*`
- Staff: `/api/kyc/staff/*`

### BFF KYC incremental update (JSON Patch)
- Endpoint: `PATCH /api/registration/kyc/profile`
- Content-Type: `application/json-patch+json`
- Auth: bearer JWT (user id comes from JWT claims)
- Behavior:
  1. Load current KYC profile for authenticated user.
  2. Apply RFC 6902 patch operations (supports nested paths).
  3. Map patched JSON back to repository patch DTO.
  4. Persist through repository layer method `patch_kyc_profile`.
- Implementation points:
  - OpenAPI: `openapi/user-storage-bff.yaml`
  - BFF handler: `app/crates/backend-server/src/api/bff.rs`
  - Repository trait/API: `app/crates/backend-repository/src/traits.rs`, `app/crates/backend-repository/src/pg/kyc.rs`
  - SQL query: `app/crates/backend-repository/queries/kyc/patch_information.sql`
  - JSON Patch crate: workspace dependency `json-patch`

`app/backend` starts the server; `crates/backend-server` is a library crate.

## Core Architecture
- Runtime is native `axum` (no generated `swagger` runtime dispatch).
- Layering is strict: `controller -> repository` (explicit service layer removed).
- Controllers: `crates/backend-server/src/api/mod.rs` (and submodules)
- API modules: `api/bff.rs`, `api/kc.rs`, `api/staff.rs`
- Repository: `crates/backend-repository/src/pg/mod.rs` (and submodules)

## Crate Roles
- `backend-core`: config + shared `Error`/`Result`
- `backend-auth`: axum middleware/extractors (request context/auth)
- `backend-server`: router/controllers/state/retry worker
- `backend-repository`: SQLx-Data repository layer. SQL queries are externalized in `queries/`.
- `backend-model`: `FromRow` DB structs + `o2o` DTO mapping
- `backend-id`: prefixed CUID ID generation
- `backend-migrate`: migration runner and database factory (Postgres only)
- `gen_oas_*`: generated OA3 models/interfaces (never edit manually)

## Hard Rules
1. Never hand-edit `crates/gen_*`.
2. OpenAPI changes happen in `openapi/*` then regenerate.
3. Keep SQL in repository crate only, externalized in `.sql` files under `queries/`.
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

### Auth and Error Test Coverage (Mandatory)
- Global error/exception mapping tests live in `app/crates/backend-core/tests/error_response.rs`.
- JWT middleware tests (BFF + Staff bearer auth) live in `app/crates/backend-auth/tests/jwt_auth_exclude_paths.rs`.
- KC signature middleware tests also live in `app/crates/backend-auth/tests/jwt_auth_exclude_paths.rs`.

Required scenarios to keep covered in tests:
- `backend_core::Error` metadata mapping and `IntoResponse` payload/status behavior.
- Bearer middleware bypass cases (`enabled = false`, blank base path, path outside protected base path).
- Bearer middleware enforcement cases (missing token, non-bearer scheme, invalid token, valid token).
- KC signature middleware enforcement cases:
  - missing `x-kc-timestamp`
  - missing `x-kc-signature`
  - invalid/out-of-skew timestamp
  - invalid signature
  - request body larger than `max_body_bytes`
  - valid signature with body preservation

Suggested verification commands:
- `cargo test -p backend-core --features axum --test error_response`
- `cargo test -p backend-auth --test jwt_auth_exclude_paths`

## Caching
- In-process cache uses `lru`.
- Redis is available in compose for distributed/shared cache use.

## Migrations & Database Factory
Migrations are located in `app/crates/backend-migrate/migrations/postgresql`.
Naming convention: `YYYYMMDDHHMMSS_description.sql`.

Database indices and schema constraints must be defined within these migration files to ensure consistency across environments.

The `backend-migrate` crate provides a `DbFactory` for constructing database pools and running migrations:
- `DbFactory::postgres(url)`: Creates a factory for Postgres.
- `connect_postgres_and_migrate(url)`: Helper to connect and run migrations in one step.

## Repository Layer (SQLx-Data)
The repository layer uses `sqlx-data` to generate type-safe database access code.
- **Traits**: Define database operations in a trait annotated with `#[repo]`.
- **Queries**: Use `#[dml]` to link methods to SQL queries. Queries should be externalized in `.sql` files under `queries/`.
- **Parameters**: `sqlx-data` handles parameter binding. Use `impl IntoParams` for flexible parameter passing.
- **Return Types**: Use `sqlx_data::Result<T>` for return types. `Serial<T>` is used for streaming results.

Example:
```rust
#[repo]
pub(crate) trait PgSqlRepo {
    #[dml(file = "queries/user/get.sql", unchecked)]
    async fn get_user_db(&self, user_id: String) -> sqlx_data::Result<Option<db::UserRow>>;
}
```

The repository implementation is split into domain-specific modules under `src/pg/`:
- `approval.rs`: Approval-related operations.
- `device.rs`: Device binding and lookup.
- `kyc.rs`: KYC profile and document management.
- `sms.rs`: SMS queue and retry logic.
- `user.rs`: User management and search.

## Validation Checklist
Before finalizing:
1. `cargo check --workspace`
2. No runtime use of `swagger` or generated `server::Service`
3. No direct `sqlx::query*` usage
4. No manual edits under `crates/gen_*`
5. Auth and error tests pass:
   - `cargo test -p backend-core --features axum --test error_response`
   - `cargo test -p backend-auth --test jwt_auth_exclude_paths`

## Work flavors
Let's talk about all the rules we're having to work efficiently:
- To work here, you should take the habit of first checking the web if there's a newer version of a framework or tool, before using the "known" version

### Rust (Cargo workspace)

- Third-party dependency versions are declared **only** in the repo root `Cargo.toml` under `[workspace.dependencies]`.
- All crates and binaries under `backend/crates/*` and `backend/bins/*` must depend on third-party crates using `{ workspace = true }`.
- If a crate needs optional capabilities, add `features = [...]` on the `{ workspace = true }` dependency in that leaf `Cargo.toml`.
- Do not add `version = "..."` for third-party crates anywhere except the root `Cargo.toml`.

#### Code style

- Use stable Rust and keep code `rustfmt`-formatted.
- Prefer explicit, self-describing names; avoid single-letter identifiers except for well-understood indices.
- Keep modules small and cohesive: one primary concern per module.
- Use `tracing` for logging; avoid `println!` in production code.
- Surface errors with rich types (thiserror / anyhow patterns) rather than panicking; reserve `panic!` for truly unrecoverable situations.
- Keep async boundaries explicit and avoid blocking inside async tasks.
- Favor traits as the primary extension/abstraction mechanism:
    - Define behavior behind traits (with clear method contracts) rather than free-floating functions.
    - Prefer trait impls on small structs (or newtypes) over ad-hoc helper functions; use free functions only for pure, stateless utilities.
    - Add default methods on traits for common runners/wrappers instead of separate “helper” modules.
    - When extracting shared logic, start by defining the trait in the owning crate (e.g., peer/bootstrap, HTTP services, IPC services) and implement it per binary.

### Backends

All backends:

- use `clap` for CLI parameters and environment variable configuration.
- use `mimalloc` for allocation.

#### Roles and rules
- This backend is handling keycloak-storage, keycloak custom flow with device key
- Fineract is handling core-backing
- BFF is the client's backend
- Staff is the admin's frontend of this backend

## Implemented Features

### KYC Profile Patch (Optimistic Locking)
- **Endpoint**: `PATCH /api/registration/kyc/profile`
- **Description**: Allows partial updates to the KYC profile using JSON Patch (RFC 6902).
- **Concurrency Control**: Uses `If-Match` header with ETag (version number) to prevent lost updates.
- **Implementation**:
    - **Handler**: `app/crates/backend-server/src/api/bff.rs` handles the request, checks the version, applies the patch, and calls the repository.
    - **Repository**: `app/crates/backend-repository/src/pg/kyc.rs` executes the update.
    - **SQL**: `app/crates/backend-repository/queries/kyc/patch_information.sql` performs the atomic update with version checking.
