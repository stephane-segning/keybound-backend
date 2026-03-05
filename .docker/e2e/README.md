# E2E (Compose) Design Notes

This folder hosts production-like, Compose-based end-to-end testing:
- real Keycloak (custom grant / custom flow)
- real user-storage server + worker
- real Postgres/Redis/MinIO
- explicit stubs for external dependencies (CUSS/SMS)

Checklist: [CHECKLIST.md](./CHECKLIST.md)

## Recommended Approach

1. Keep **Rust OAS integration tests** (`just test-it`) as the fast contract suite.
2. Run **Compose E2E** with the Rust runner:
   - `just test-e2e-smoke` for minimal path validation
   - `just test-e2e-full` for multi-step flow coverage

## Runner: Rust (`reqwest`)

TypeScript runner coverage has been migrated to Rust integration tests under:
- `app/crates/backend-e2e/tests/smoke.rs`
- `app/crates/backend-e2e/tests/full.rs`

The Rust runner uses environment variables passed by `just`:
- `USER_STORAGE_URL`
- `USER_STORAGE_BLANK_BASE_URL` (for blank-base-path auth bypass coverage in full suite)
- `KEYCLOAK_URL`
- `CUSS_URL`
- `SMS_SINK_URL`
- `DATABASE_URL`
- `KEYCLOAK_CLIENT_ID`
- `KEYCLOAK_CLIENT_SECRET`

## Current Scenario Coverage (Rust)

`smoke.rs`:
- `/health` readiness
- Keycloak realm metadata reachability
- CUSS stub admin reachability
- SMS sink reset path

`full.rs`:
- BFF deposit create + read
- KYC session + step + phone OTP issue/verify with SMS sink polling
- Staff summary + instances list + missing instance 404
- CUSS stub request recording

## Failure Artifacts

On `just test-e2e-smoke` / `just test-e2e-full` failures, Compose logs are saved to:
- `.docker/e2e/artifacts/e2e-smoke-failure.log`
- `.docker/e2e/artifacts/e2e-full-failure.log`
