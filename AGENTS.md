# AGENTS.md

## Repository Overview

Tokenization/user-storage backend with three HTTP surfaces:
- **KC**: `/kc/*` - Keycloak integration
- **BFF**: `/bff/*` - Backend for Frontend  
- **Staff**: `/staff/*` - Staff/admin operations

**Architecture**: Rust workspace with native `axum` runtime, strict `controller -> repository` layering, and Diesel-async for database access.

## Build, Test & Lint Commands

### Quick Development Cycle
```bash
# Run backend in dev mode with logs
just dev

# Run a single test by name
cargo test -p <crate> <test_name>--- --exact --nocapture

# Run tests for a specific crate
cargo test -p backend-server
cargo test -p backend-core
cargo test -p backend-auth
cargo test -p backend-repository

# Run all workspace tests
cargo test --workspace --locked

# Run only unit tests (skip integration tests)
cargo test --workspace --lib
```

### Integration & E2E Testing
```bash
# OAS3 integration tests (requires it-tests feature)
just test-it
cargo test -p backend-server --features it-tests api::it_tests::

# Rust-native E2E tests with external deps (requires e2e-tests feature)
cargo test -p backend-auth --features e2e-tests --test oidc_wiremock_e2e
cargo test -p backend-repository --features e2e-tests --test state_machine_repo_testcontainers

# Compose E2E tests (full stack)
just test-e2e-smoke  # Quick smoke tests
just test-e2e-full   # Full test suite
```

### Linting & Code Quality
```bash
# Format code
cargo fmt

# Run clippy with fixes
cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings

# Check workspace compilation
cargo check --workspace

# Run all checks (format, clippy, fix)
just all-checks
```

### Running a Single Test

For unit tests:
```bash
cargo test -p backend-server state::tests::test_name -- --exact --nocapture
```

For integration tests:
```bash
cargo test -p backend-server --features it-tests api::it_tests::test_name -- --exact --nocapture
```

For repository tests:
```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/user-storage \
  cargo test -p backend-repository --test device_repo
```

## Code Style Guidelines

### Imports & Dependencies
- **Workspace dependencies only**: All third-party crates must be declared in root `Cargo.toml` `[workspace.dependencies]`
- **Reference with workspace = true**: In crate Cargo.toml, use `serde.workspace = true` not `version = "..."`
- **No local path overrides**: Do not use `path` dependencies for workspace crates

### Formatting
- `rustfmt` is mandatory - always format before committing
- Line width: default (100 chars)
- Use stable Rust only, no nightly features
- No `unsafe` code unless absolutely necessary

### Naming Conventions
- `snake_case` for variables, functions, modules
- `PascalCase` for types, traits, enums
- `SCREAMING_SNAKE_CASE` for constants
- CUID with prefixes for IDs: `usr_*`, `dvc_*`, `apr_*`, `sms_*` (never UUID)
- Explicit, descriptive names preferred (no single-letter vars except indices)

### Type Definitions
- Use Diesel DSL for all database queries (no raw SQL strings)
- Map Diesel errors to `backend_core::Error` via `.map_err(Into::into)`
- Use `RepoResult<T>` type alias from repository layer
- Prefer `Result<T>` over panics; reserve `panic!` for unrecoverable errors
- Use `tracing` for logging, never `println!` in production code

### Error Handling
```rust
// Use the shared Error type
use backend_core::Error;

// Repository pattern
async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>> {
    users.find(id).first(&mut conn).await.optional().map_err(Into::into)
}

// Map Diesel errors consistently
.map_err(Into::into)  // Converts to backend_core::Error
```

### Async Code
- Keep async boundaries explicit
- Never block in async context
- Use `tokio::spawn` for concurrent tasks
- Return `impl Future` from traits when needed

### Traits & Abstraction
- Define behavior behind traits (e.g., `UserRepo`, `SmsProvider`)
- Use `mockall` for test mocks (see `test_utils/mod.rs`)
- Implement traits with `Arc<dyn Trait>` in `AppState`
- Add default methods on traits for common logic

## Database & Migrations

### Migration Workflow
```bash
# Create new migration
cd app/crates/backend-migrate
cargo run -- create_migration <name>

# Must touch a Rust file after adding SQL migration:
touch app/crates/backend-migrate/src/migrate.rs
```

### Migration Rules
- Naming: `YYYYMMDDHHMMSS_description.sql`
- Use `TEXT` not `VARCHAR` for string columns
- Define indices and constraints in migration files
- Use Diesel DSL, avoid raw SQL where possible

## Key Directories

- `app/crates/`: Library crates (`backend-server`, `backend-core`, `backend-auth`, etc.)
- `app/bins/`: Binary crates (`backend` server, `sms-gateway`)
- `app/gen/`: Generated code (OpenAPI models) - **NEVER EDIT MANUALLY**
- `openapi/`: OpenAPI spec files (source of truth)
- `app/crates/backend-migrate/migrations/`: Database migrations
- `config/`: Configuration YAML files

## Pre-Commit Checklist

Before committing code:
1. `cargo fmt` - Format code
2. `cargo clippy --all-targets --all-features -- -D warnings` - Lint
3. `cargo check --workspace` - Compile check
4. Run relevant unit tests: `cargo test -p <crate>`
5. For API changes: `just test-it` (OAS integration tests)
6. For database changes: `cargo test -p backend-repository`
7. Never edit `app/gen/*` manually

## Testing Best Practices

### Unit Tests
- Location: `src/` module files (inline) or `tests/` directory for integration tests
- Use `mockall` for mocking traits
- Test both success and failure paths
- For repository tests: set `DATABASE_URL` or tests will skip

### Integration Tests
- Feature-gated: `--features it-tests` for OAS, `--features e2e-tests` for external deps
- OAS tests: `app/crates/backend-server/src/api/it_tests.rs`
- E2E tests: `app/crates/backend-e2e/tests/`

### Required Test Coverage
- `backend_core::Error` mapping and response behavior
- Bearer/JWT middleware bypass and enforcement cases
- KC signature verification (all failure modes + success)
- Device binding unique-conflict races
- SMS retry behavior (transient vs permanent errors)

## OpenAPI Workflow

1. Modify specs in `openapi/*.yaml` (not `app/gen/`)
2. Regenerate code: `just generate`
3. Validate: `just test-it`
4. Update handlers if API contract changed

## Configuration

- Config source: `backend-core::Config` only
- Supports env var expansion: `${VAR}` or `${VAR:-default}`
- Use `clap` for CLI args in binaries
- Shared state in `AppState` with `Arc<dyn Trait>` abstractions