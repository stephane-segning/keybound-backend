# Keybound Backend

This repository powers the user/device storage side of the Keycloak device-binding flow. The service exposes three HTTP surfaces:

- `/kc/*` handles the Keycloak-specific enrollment/lookup flow.
- `/bff/*` is the customer-facing backend for web/mobile.
- `/staff/*` is the administrative interface.

The runtime is Rust/`axum` with Diesel-async for the database layer and `diesel_migrations` for schema management. Every crate lives under `app/crates/` and depends on shared workspace dependencies defined at the workspace root.

## Repository layout

- `app/crates/backend-server`: Controllers, state, middleware wiring, and background worker logic.
- `app/crates/backend-repository`: Diesel-async repository implementations plus integration tests.
- `app/crates/backend-model`: Diesel schema, DTO mapping, and shared helpers such as `device_record_id`.
- `app/crates/backend-migrate`: Migration runner and embedded migration artifacts (`app/crates/backend-migrate/migrations/**`).
- `app/bins/backend`: Binary that instantiates the server using the shared libraries.
- `app/gen/`: Auto-generated OpenAPI clients/server scaffolding (do **not** edit manually).
- `config/`, `deploy/`, `docs/`, `openapi/`: configuration, deployment manifests, architecture docs, and OpenAPI sources.

## Getting started

1. Copy `config/dev.yaml` (or your preferred environment) and set `DATABASE_URL`, `KEYCLOAK_ISSUER`, and any other required env vars.
2. Run the database migrations: `just build` or programmatically via `backend-migrate`’s `DbFactory`.
3. Start the server: `just run` (uses the `app/bins/backend` entry point) or call `cargo run -p backend-bins --bin backend -- --help` to see CLI options.

## Testing

- Unit/integration tests: `cargo test --workspace`.
- Repository integration tests that touch the database (e.g., `backend-repository/tests/device_repo.rs`) require `DATABASE_URL`; the test skips if the env var is unset. Use a local Postgres instance for full coverage.
- Specialized tests: `cargo test -p backend-core --features axum --test error_response` and `cargo test -p backend-auth --test jwt_auth_exclude_paths`.

## Documentation

- Refer back to `AGENTS.md` for up-to-date architectural constraints and workflows.
- Refer to `AGENTS.md` section "Opencode AI Agents" for AI-powered development workflows.
- Migrations live in `app/crates/backend-migrate/migrations` and follow the `YYYYMMDDHHMMSS_description.sql` naming scheme.
- Keep `app/gen/*` files in sync only through automated OpenAPI generation (changes should start in `openapi/`).

## AI-Powered Development

This project includes 10 specialized AI agents via the opencode CLI tool to assist with implementation:

**Quick start with agents:**
```bash
# List all available agents
opencode agent list

# Run daily project status check
opencode run --agent agent-orchestrator daily-standup

# Generate BFF OpenAPI code
opencode run --agent bff-generator generate-bff

# Implement Phone OTP flow
opencode run --agent flow-otp-master implement-otp-flow

# Validate flow implementation
opencode run --agent flow-otp-master validate-flow phone_otp
```

See `AGENTS.md` "Opencode AI Agents" section for complete agent documentation, project phases, and usage examples.

## Notes

- Device binding now relies on deterministic record IDs derived from the stored JWK; `lookup_device` updates `last_seen_at` so downstream systems can display accurate activity.
- Use `backend-id` helpers (`usr_*`, `dvc_*`, etc.) for all generated IDs to stay consistent with the system’s conventions.
