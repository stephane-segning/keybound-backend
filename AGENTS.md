# AGENTS.md

## Purpose
Tokenization/user-storage backend with three HTTP surfaces:
- KC: `/kc/*`
- BFF: `/bff/*`
- Staff: `/staff/*`

## Revamp Status (2026-03-07)
- Legacy KYC SQL tables (`kyc_*`, `phone_deposit`) are replaced by a generic persisted state-machine store:
  - `sm_instance`, `sm_event`, `sm_step_attempt`
- Two KYC processes are implemented as state machines:
  - `KYC_PHONE_OTP`
  - `KYC_FIRST_DEPOSIT` (staff confirms payment then approves; worker calls CUSS `registerCustomer` then `approveAndDeposit`)
- Staff OpenAPI (`openapi/user-storage--staff.yaml`) is rewritten to expose state-machine observability and controls.
- OAS3 integration tests are implemented in `backend-server` under `app/crates/backend-server/src/api/it_tests.rs` and gated by the `it-tests` crate feature.
- Local OAS integration test command is available via `just test-it`.
- CI now runs both workspace tests and the OAS integration test suite.
- BFF OpenAPI (`openapi/user-storage-bff.yaml`) is rewritten around case-scoped routes under `/internal/kyc/*`.
- BFF handlers are split into flow modules under `app/crates/backend-server/src/api/bff/` to keep endpoint logic cohesive.
- Session listing is available via `GET /internal/kyc/sessions` so callers can reuse active sessions per flow.
- First-deposit contact routing now supports static recipient configuration via `deposit_flow.staff.recipients` (`provider`, `fullname`, `phone-number`, `regex`, `currency`) synced into `app_deposit_recipients`; provider selection is resolved by regex match against the requester phone prefix.
- Compose E2E runner is Rust-based (`app/crates/backend-e2e`), replacing the previous TypeScript runner:
  - `just test-e2e-smoke` executes smoke scenarios.
  - `just test-e2e-full` executes full scenarios.
  - `full.rs` is now split into focused scenarios guarded by `serial_test::file_serial` for deterministic sequential execution with clearer failure isolation.
  - Scenario tracking source of truth is `.docker/e2e/CHECKLIST.md`.

### Compose E2E Migration Snapshot (from `.docker/e2e/CHECKLIST.md`, 2026-03-05)
- Overall checklist status:
  - Implemented: `75`
  - Partial: `0`
  - Missing: `0`
- Implemented areas:
  - Compose infrastructure and runner flow (`test-e2e-smoke`, `test-e2e-full`, log capture on failure).
  - Health endpoint, Bearer auth enforcement (`401` cases + valid token pass-through), outside-base-path bypass behavior, blank-base-path routing bypass behavior, and `enabled=false` bypass behavior.
  - BFF deposit owner/non-owner checks, session resume idempotency, step status reads, and OTP happy path.
  - BFF OTP expiry and OTP issuance rate-limit checks.
  - BFF wrong OTP deterministic error + attempt observability checks.
  - BFF deposit expiry behavior checks.
  - BFF email magic issue/verify and uploads presign/complete/invalid-complete coverage.
  - Staff instances listing/detail/retry coverage with filters/pagination.
  - Staff summary aggregates for known fixtures.
  - KYC first-deposit staff confirm + approve flow through worker to CUSS success path.
  - Repeated staff approve is deterministic (`409 DEPOSIT_ALREADY_APPROVED`) and does not double-call CUSS approve-and-deposit.
  - KYC first-deposit CUSS failure paths (`registerCustomer`, `approveAndDeposit`) with manual retry and observability checks.
  - Worker single-consumer lock validation with dual-worker Compose startup and single active worker health verification.
  - SMS retry behavior coverage: transient provider errors back off and retry to success; permanent errors terminate without infinite retries.
  - KC signature middleware matrix and KC surface CRUD/device race/idempotency coverage.
  - Representative cross-surface validation/not-found/conflict/internal error mapping checks.
- Partial areas:
  - none.
- Major missing groups:
  - none.
- Keep this snapshot aligned with `.docker/e2e/CHECKLIST.md` whenever scenarios are added or marked complete.

`app/bins/backend` starts the server; `app/crates/backend-server` is a library crate.

## Core Architecture
- Runtime is native `axum`, using `Router::nest` to mount each API surface under a configurable base path.
- Layering is strict: `controller -> repository` (explicit service layer removed).
- Controllers: `app/crates/backend-server/src/api/mod.rs` (and submodules)
- API modules: `api/bff/mod.rs` (split by flow modules), `api/kc.rs`, `api/staff.rs`
- Repository: `app/crates/backend-repository/src/pg/mod.rs` (and submodules)
  - State machines: `app/crates/backend-repository/src/pg/state_machine.rs`

## Crate Roles (under `app/crates/`)
- `backend-core`: config + shared `Error`/`Result`
- `backend-auth`: axum middleware layers for authentication and authorization.
- `backend-server`: router/controllers/state/retry worker
- `backend-repository`: Diesel-async repository layer.
- `backend-model`: Diesel models (`Queryable`, `Selectable`, `Insertable`, `Identifiable`) + `o2o` DTO mapping. Contains `schema.rs`.
- `backend-id`: prefixed CUID ID generation
- `backend-e2e`: Compose-oriented Rust E2E runner tests (`reqwest` + Keycloak + stubs) under `app/crates/backend-e2e/tests`.
- `backend-migrate`: migration runner and database factory (Postgres only)
- `gen_oas_*`: generated OA3 models/interfaces (under `app/gen/`, never edit manually)

## Hard Rules
1. Never hand-edit `app/gen/*`.
2. OpenAPI changes happen in `openapi/*` then regenerate.
3. Use Diesel DSL for database operations; avoid raw SQL strings where possible.
4. Use `diesel-async` for all database interactions in the repository layer.
5. Map Diesel errors to `backend_core::Error` using `Into::into`.
6. Use `backend_core::Error` only; avoid scattered custom error mapping.
7. Keep server config source in `backend-core::Config` only.
8. Keep `backend-server` as library; app binary wires and starts it.
9. `backend-core::Config` supports environment variable expansion in YAML files using `${VAR}` or `${VAR:-default}` syntax.
10. Use `TEXT` instead of `VARCHAR` for all string columns in migrations.
11. Use Argon2 for hashing sensitive data that needs verification (e.g., SMS OTPs).
12. Always sort JWK keys alphabetically before serializing to JSON for signature payloads to ensure deterministic string representations across different platforms (Frontend, Keycloak, Backend).

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
- Device rows now use a `(device_id, public_jwk)` composite primary key and expose a deterministic `device_record_id` that wraps `device_id` + SHA‑256 of the sorted JWK. `lookup_device` must refresh `last_seen_at` on every match so usage tracking stays accurate.
- The new `backend-repository/tests/device_repo.rs` integration test requires an available Postgres instance; set `DATABASE_URL` before running `cargo test -p backend-repository --test device_repo`, otherwise the test will skip with a notice.

## Auth
- Auth logic is implemented as `axum` middleware layers in `backend-auth`.
- Each API surface (KC, BFF, Staff) has its own middleware layer applied at the router level in `backend-server`.
- `BackendApi` and `AppState` hold `Arc<OidcState>` and `Arc<SignatureState>` for runtime verification.

### Testing Coverage (Mandatory)
- Global error/exception mapping tests live in `app/crates/backend-core/tests/error_response.rs`.
- JWT middleware tests (BFF + Staff bearer auth) live in `app/crates/backend-auth/tests/jwt_auth_exclude_paths.rs`.
- KC signature middleware tests also live in `app/crates/backend-auth/tests/jwt_auth_exclude_paths.rs`.
- **Unit Tests**: `backend-server` has comprehensive unit tests for `state`, `api::{bff, staff}`, and `worker` using `test_utils` mocks.
- **OAS3 Integration Tests**: `backend-server` OAS integration scenarios live in `app/crates/backend-server/src/api/it_tests.rs` and run with `--features it-tests`.
- **Rust-native E2E Tests**: feature-gated scenarios use `--features e2e-tests`:
  - `app/crates/backend-auth/tests/oidc_wiremock_e2e.rs` (OIDC discovery/JWKS via `wiremock`)
  - `app/crates/backend-repository/tests/state_machine_repo_testcontainers.rs` (repository + migrations against ephemeral Postgres via `testcontainers`)
- **Compose E2E Tests (Rust Runner)**: `backend-e2e` integration tests run against the Compose stack:
  - `app/crates/backend-e2e/tests/smoke.rs`
  - `app/crates/backend-e2e/tests/full.rs`

#### Auth and Error Scenarios
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
  - url encoded paths
  - nested router paths
  - method mismatch
  - path mismatch
  - body mismatch

Suggested verification commands:
- `cargo test -p backend-core --features axum --test error_response`
- `cargo test -p backend-auth --test jwt_auth_exclude_paths`
- `cargo test -p backend-server` (runs all unit tests with mocks)
- `just test-it` (runs OAS3 integration tests)
- `cargo test -p backend-server --features it-tests api::it_tests::`
- `cargo test -p backend-e2e --features e2e-tests --test smoke -- --nocapture`
- `cargo test -p backend-e2e --features e2e-tests --test full -- --nocapture`
- `just test-e2e-smoke` (runs Compose smoke e2e via Rust runner)
- `just test-e2e-full` (runs Compose full e2e via Rust runner)
- `cargo test -p backend-auth --features e2e-tests --test oidc_wiremock_e2e`
- `cargo test -p backend-repository --features e2e-tests --test state_machine_repo_testcontainers`

## Caching
- In-process cache uses `lru`.
- Redis is available in compose for distributed/shared cache use.

## Migrations & Database Factory
Migrations are located in `app/crates/backend-migrate/migrations`.
Naming convention: `YYYYMMDDHHMMSS_description.sql`.

Database indices and schema constraints must be defined within these migration files to ensure consistency across environments.

**Development Workflow Note**:
Migrations are compile-time checked and embedded using `diesel_migrations::embed_migrations!`. When adding a new `.sql` migration file, you **MUST** touch a Rust file in the `backend-migrate` crate (e.g., `touch app/crates/backend-migrate/src/migrate.rs`) to force Cargo to recompile the crate and include the new migration in the binary.

The `backend-migrate` crate provides a `DbFactory` for constructing database pools and running migrations:
- `DbFactory::postgres(url)`: Creates a factory for Postgres.
- `connect_postgres_and_migrate(url)`: Helper to connect and run migrations in one step.

## Repository Layer (Diesel-Async)
The repository layer uses `diesel-async` for type-safe database access.
- **Traits**: Define domain-specific operations in traits (e.g., `UserRepo`).
- **Implementation**: Implement traits using Diesel DSL and `diesel-async`.
- **Pool**: Use `deadpool_diesel::Pool<diesel_async::AsyncPgConnection>`.
- **Error Handling**: Map Diesel errors to `backend_core::Error`.

Example:
```rust
impl UserRepo for UserRepository {
    async fn get_user(&self, user_id_val: &str) -> RepoResult<Option<db::UserRow>> {
        use backend_model::schema::app_user::dsl::*;
        let mut conn = self.get_conn().await?;

        app_user
            .filter(id.eq(user_id_val))
            .first::<db::UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)
    }
}
```

The repository implementation is split into domain-specific modules under `src/pg/`:
- `device.rs`: Device binding and lookup.
- `state_machine.rs`: Generic state machine persistence (`sm_*` tables).
- `user.rs`: User management and search.

## SMS Provider Architecture
- **Trait**: `SmsProvider` (in `app/crates/backend-server/src/sms_provider.rs`) defines the contract for sending SMS.
- **Implementations**:
  - `ConsoleSmsProvider`: Logs SMS content to stdout (dev/test only).
  - `SnsSmsProvider`: Sends SMS via AWS SNS (production).
- **Configuration**: The provider is selected at runtime based on the `sms.provider` config key (`console` or `sns`).

## Abstractions & Mocking
To support unit testing without external dependencies, `backend-server` uses trait-based abstractions in `AppState`:
- **Storage**: `MinioStorage` trait (in `app/crates/backend-server/src/file_storage.rs`) abstracts S3-compatible object storage (MinIO/S3).
- **Queues**: `NotificationQueue` and `StateMachineQueue` traits abstract Redis-backed job enqueueing.
- **Repositories**: `StateMachineRepo`, `UserRepo`, and `DeviceRepo` are used via `Arc<dyn Trait>`.

Mocks for these traits are provided in `app/crates/backend-server/src/test_utils/mod.rs` using `mockall`.

## Validation Checklist
Before finalizing:
1. `cargo check --workspace`
2. No runtime use of `swagger` or generated `server::Service`
3. No manual edits under `app/gen/*`
4. Auth and error tests pass:
   - `cargo test -p backend-core --features axum --test error_response`
   - `cargo test -p backend-auth --test jwt_auth_exclude_paths`
5. OAS3 integration tests pass:
   - `just test-it`
   - or `cargo test -p backend-server --features it-tests api::it_tests::`

## Docker & Build System
- **Target**: `x86_64-unknown-linux-musl` or `aarch64-unknown-linux-musl`.
- **Base Image**: `rust:1-alpine` for building, `gcr.io/distroless/static-debian12:nonroot` for execution.
- **Dockerfiles**:
  - `deploy/docker/user-storage/Dockerfile`: builder + runtime image (builds the MUSL binaries inside the container).
  - `deploy/docker/user-storage/Dockerfile.runtime`: runtime-only image (expects prebuilt `backend` + `healthcheck` in the build context).
- **Static Linking**:
    - OpenSSL is statically linked via `openssl = { version = "0.10", features = ["vendored"] }`.
    - libpq is statically linked via `diesel = { version = "2.3", features = ["postgres", ..., "pq-src"] }`.
- **Build Command**: `just build` (uses Docker Compose).

## CI/CD (GitHub Actions)
- Main workflow: `.github/workflows/ci.yaml`
- CI is structured as: parallel tests -> native per-arch MUSL binaries -> per-arch image builds -> manifest publish.
- Tests:
  - `test-workspace`: `cargo test --workspace --locked`
  - `test-oas`: `cargo test -p backend-server --features it-tests api::it_tests:: --locked`
- Caching:
  - Uses `Swatinem/rust-cache@v2` on test/build jobs to cache `target/` and the Cargo registry.
- MUSL binaries:
  - `build-musl` matrix builds `x86_64-unknown-linux-musl` on an amd64 runner and `aarch64-unknown-linux-musl` on an arm64 runner.
  - Outputs are uploaded as artifacts: `musl-binaries-amd64` and `musl-binaries-arm64`.
  - Caveat: if the amd64 MUSL link step references `/usr/lib/x86_64-linux-gnu/libc.a` or `__gcc_personality_v0`, the build is accidentally pulling glibc libs. In that case, prefer building the binaries in the `deploy/docker/user-storage/Dockerfile` builder stage (Alpine + musl) and then packaging with `Dockerfile.runtime`.
- Containers:
  - Per-arch container builds use `deploy/docker/user-storage/Dockerfile.runtime` and the downloaded MUSL artifacts.
  - Multi-arch tags are created via `docker buildx imagetools create` (manifest list assembly).
  - QEMU is intentionally not used; builds run natively per architecture.
- Version gating (master only):
  - `cargo-version` compares `workspace.package.version` in `Cargo.toml` against the latest git tag (supports `vX.Y.Z` tags).
  - Prod image publishing is gated on "workspace version != latest tag version".

## Work flavors
Let's talk about all the rules we're having to work efficiently:
- To work here, you should take the habit of first checking the web if there's a newer version of a framework or tool, before using the "known" version
- When searching for code, use `grep` only in `{app,config,deploy,docs,openapi}` directories to avoid noise from `target/` or other ignored directories.

### Work Methodology
- **Analyze First**: Always read the existing implementation and related queries before starting a migration.
- **Incremental Migration**: Migrate one module at a time.
- **Type Safety**: Leverage Diesel DSL for type-safe queries. Avoid raw SQL.
- **Error Mapping**: Consistently map Diesel errors to `backend_core::Error` using `Into::into`.
- **Verification**: Run `cargo check --workspace` and relevant tests after every significant change.
- **Documentation**: Keep `AGENTS.md` updated with the current state of the project (e.g., which modules are migrated).
  - When you add new README/overview docs, include them at the workspace root so future contributors know where to look.

### Rust (Cargo workspace)

- Third-party dependency versions are declared **only** in the repo root `Cargo.toml` under `[workspace.dependencies]`.
- All crates and binaries under `backend/crates/*` and `backend/bins/*` must depend on third-party crates using `{ workspace = true }`.
- If a crate needs optional capabilities, add `features = [...]` on the `{ workspace = true }` dependency in that leaf `Cargo.toml`.
- Do not add `version = "..."` for third-party crates anywhere except the root `Cargo.toml`.
- Integration test features:
  - `it-tests` for feature-gated OAS integration suites (do not overload default/unit test paths with OAS matrix scenarios).
  - `e2e-tests` for Rust-native external-dependency integration tests (`wiremock`, `testcontainers`) that should stay out of default/unit test paths.

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

### KYC State Machines (Current)
- **Persistence**: `sm_instance`, `sm_event`, `sm_step_attempt` (legacy `kyc_*` and `phone_deposit` tables are removed by the state-machine revamp migration).
- **Processes**:
  - `KYC_PHONE_OTP`
  - `KYC_FIRST_DEPOSIT`

### BFF KYC Sessions & Steps (Current)
- **Endpoints (under the `/bff` surface)**:
  - `GET /internal/kyc/sessions` (filter by `userId`, `flow`, `activeOnly`)
  - `POST /internal/kyc/sessions`
  - `GET /internal/kyc/sessions/{sessionId}`
  - `POST /internal/kyc/phone-otp/steps`
  - `POST /internal/kyc/email-magic/steps`
  - `GET /internal/kyc/steps/{stepId}`
- **Session create behavior**:
  - Requires `flow` in request payload.
  - Persists normalized `context.flow` and `context.step_ids`.
  - Reuses active session via `ensure_active_instance` keying by `{kind}:{flow}:{userId}`.
- **Step IDs**: deterministic `"{sessionId}__{stepType}"` stored in session `context.step_ids`.
- **Flow-to-kind mapping**:
  - `PHONE_OTP` and `EMAIL_MAGIC` map to `KYC_PHONE_OTP`.
  - `FIRST_DEPOSIT` maps to `KYC_FIRST_DEPOSIT`.
  - `ID_DOCUMENT` and `ADDRESS_PROOF` map to dedicated kinds for future extension.

### KYC Challenges & Uploads (Current)
- **Phone OTP**:
  - `POST /internal/kyc/phone-otp/challenges`
  - `POST /internal/kyc/phone-otp/verifications`
- **Email magic**:
  - `POST /internal/kyc/email-magic/challenges`
  - `POST /internal/kyc/email-magic/verifications`
- **Uploads**:
  - `POST /internal/kyc/uploads/presign`
  - `POST /internal/kyc/uploads/complete`
- **Identity requirement**: challenge/verification/upload payloads carry both `sessionId` and `stepId`.

### Phone Deposit Requests (Current)
- **Endpoints (under the `/bff` surface)**:
  - `POST /internal/kyc/deposits/phone/requests`
  - `GET /internal/kyc/deposits/phone/requests/{depositRequestId}`
- **Session requirement**: create requires a `sessionId` that belongs to the caller and is a `FIRST_DEPOSIT` flow session.
- **Provider routing**: request payload does not require provider; backend resolves provider/recipient from caller phone number using configured `deposit_flow.staff.recipients[*].regex` rules.
- **Persistence**: stored in `KYC_FIRST_DEPOSIT` state-machine context/attempts, not in a dedicated legacy table.
- **Ownership**: API enforces JWT user ownership for create/get access.

### Background Worker for SMS Retries
- **Description**: A background worker, powered by the `apalis` crate, handles the retrying of SMS messages.
- **Concurrency Control**: Uses Redis for distributed locking to ensure that only one worker instance processes the SMS queue at a time.
- **Implementation**:
    - **CLI**: The application can be started in `server`, `worker`, or `shared` mode via a CLI flag.
    - **Worker Logic**: The worker logic is located in `app/crates/backend-server/src/worker.rs`.
    - **Queueing**: SMS messages are enqueued into a Redis-backed queue for the worker to process.

### SMS Provider Trait & Argon2 Hashing
- **Description**: A pluggable SMS provider system supports both local development (console logging) and production (AWS SNS).
- **Security**: SMS OTPs are generated as 6-digit codes and hashed using Argon2 before storage. Verification compares the hash of the input against the stored hash.
- **Implementation**:
    - **Trait**: `SmsProvider` in `app/crates/backend-server/src/sms_provider.rs`.
    - **Hashing**: Uses the `argon2` crate for secure password hashing.

### OIDC Discovery & Keycloak Signature Verification
- **OIDC Discovery**: `backend-auth` supports automatic OIDC discovery. It fetches the discovery document from the configured `issuer` to obtain the `jwks_uri` and caches it. The `jwks_url` configuration field has been removed as it is now fully inferred from the `issuer`.
- **Signature Verification**: Keycloak requests are verified using a HMAC-SHA256 signature.
    - **Headers**: `x-kc-signature` and `x-kc-timestamp`.
    - **Canonical Payload**: `timestamp + "\n" + method + "\n" + path + "\n" + body`.
    - **Encoding**: The resulting HMAC digest is Base64URL encoded (no padding).
- **JWT Validation**: Tokens are validated against the JWKS obtained via OIDC discovery.
- **Integration**: `backend-server`'s `AppState` and `BackendApi` now hold `OidcState` and `SignatureState`. API handlers use these for robust JWT verification and signature checking.

### Backend-Server Unit Testing Expansion
- **Coverage**: Expanded coverage for `state`, `api::{bff, staff}`, and `worker` modules.
- **Mocks**: Uses `mockall` and `test_utils` to exercise handlers without hitting real AWS/SNS/Redis/HTTP dependencies.
- **Traits**: Introduced `MinioStorage`, `NotificationQueue`, and `StateMachineQueue` traits to enable clean dependency injection in `AppState`.
