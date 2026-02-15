# 2026 Upgrade Plan

Date: February 15, 2026

## Scope

1. Replace Kafka design assumptions with Redis-based eventing.
2. Defer pgvector integration.
3. Introduce a user-friendly worker framework for cron/scheduled jobs.
4. Rework BFF handlers after `openapi/user-storage-bff.yaml` changes.
5. Switch migration source to `crates/backend-migrate/migrations` with automatic PostgreSQL vs SQLite (`h2`) detection.
6. Align `crates/backend-core/src/config.rs` with `config/default.yaml`.

## Phase 0: Stabilization (Week 1)

- Regenerate `crates/gen_oas_*` from the updated OpenAPI specs (no manual edits under generated crates).
- Bring `backend-model`/`backend-server` back to compile against the new BFF types and operation names.
- Confirm workspace baseline with `cargo check --workspace`.

Exit criteria:
- `cargo check --workspace` passes.
- No references remain to removed BFF model names.

## Phase 1: Migration Engine Upgrade (Week 1-2)

- Update `backend-migrate` to:
  - detect engine from database URL (`postgres://`, `postgresql://`, `sqlite://`, `file:`, `jdbc:h2:`),
  - use `crates/backend-migrate/migrations/postgresql` for PostgreSQL,
  - use `crates/backend-migrate/migrations/h2` for SQLite/H2-compatible path.
- Keep migration execution deterministic and idempotent.
- Add tests for engine detection and migration source selection.

Exit criteria:
- `backend migrate -c ...` runs successfully for PostgreSQL and SQLite URLs.

## Phase 2: Config Contract Alignment (Week 2)

- Refactor `backend-core::Config` to match `config/default.yaml` shape.
- Keep compatibility defaults where practical to reduce rollout risk.
- Ensure all consumers (`backend-server`, auth middleware, AWS clients, etc.) compile against the new config schema.

Exit criteria:
- `backend config -c config/default.yaml` validates and `backend serve -c ...` boots.

## Phase 3: Worker Framework (Week 3-4)

- Add a worker framework crate/module with:
  - typed job interface,
  - cron-like scheduler,
  - retry/backoff policy,
  - structured logs and metrics.
- Move existing SMS retry loop into the framework as first job.
- Add at least one additional scheduled maintenance job (for expirations/cleanup).

Exit criteria:
- Workers run from the same binary with clear configuration and observability.

## Phase 4: Redis Event Backbone (Week 4-5)

- Replace Kafka assumptions with Redis streams/queues + consumer groups.
- Use outbox-driven publish flow from database to Redis.
- Define event envelope (`event_id`, `trace_id`, `user_id`, `source`, `timestamp`) and idempotency keys.

Exit criteria:
- At least one producer and one consumer path running via Redis in non-dev mode.

## Phase 5: Pgvector Deferral (Week 5)

- Keep risk/embedding tables but remove hard dependency on `CREATE EXTENSION vector` for initial rollout.
- Store embeddings as text/JSON placeholder fields until pgvector is reintroduced.

Exit criteria:
- Migrations and runtime work without pgvector installed.

## Deliverable Sequence

1. Migration engine switch (highest unblocker for schema alignment).
2. BFF API/model rewrite from new OpenAPI.
3. Config schema sync.
4. Worker framework extraction.
5. Redis eventing rollout.
6. pgvector deferred cleanup.

## Immediate Next Tasks

1. Implement migration source/dialect detection in `backend-migrate`.
2. Verify migration run path against PostgreSQL and SQLite.
3. Commit migration refactor before starting BFF handler rewrite.
