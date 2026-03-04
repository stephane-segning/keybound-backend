# E2E (Compose) Design Notes

This folder is intended for production-like, Compose-based end-to-end testing:
- real Keycloak (custom grant / custom flow)
- real user-storage server + (optional) worker
- real Postgres/Redis/MinIO
- explicit stubs for external dependencies (CUSS/SMS/etc.) with fault injection

Checklist: [CHECKLIST.md](./CHECKLIST.md)

## Recommended Approach (Fast Feedback + Full Coverage)

1. Keep **Rust OAS integration tests** (`just test-it`) as the fast contract suite.
2. Add **Compose E2E** as the "it really works with Keycloak" suite:
   - `smoke`: minimal happy paths + auth/signature enforcement
   - `full`: all edge cases + retries + concurrency

## Framework Ideas (Speeding Up Scenario Authoring)

### Runner: Node.js (TypeScript) + `vitest`

Why:
- fast iteration on multi-step flows (Keycloak token endpoint + user-storage surfaces)
- easy concurrency tests (Promise.all) to force unique races
- easy JSON assertions (JSONPath-like helpers) and snapshots

Suggested stack:
- `vitest` as the test runner
- `undici` for HTTP
- `jose` for JWT decoding/verification (for claims assertions; real verification remains in user-storage)
- `zod` for response shape checks (keeps assertions crisp)

Pattern:
- tests read env (`KEYCLOAK_URL`, `USER_STORAGE_URL`, `REALM`, `CLIENT_ID`, secrets)
- helper library provides:
  - `waitForHealth()`
  - `kcToken(customGrantParams)` and `kcAdmin(...)` (if needed)
  - request builders for `/kc/*`, `/bff/*`, `/staff/*`
  - deterministic JWK sorting utilities (important for signature payload determinism)

### Stubs: Fastify (TypeScript) for Stateful + Faulty Dependencies

Keep WireMock for "dumb" static mocks. Use a TS stub service when you need:
- stateful flows (record requests, return responses based on previous calls)
- fault injection (first call 500, then 200; timeouts; malformed JSON)
- introspection (`GET /__admin/requests`, `POST /__admin/reset`, etc.)

Suggested stub layout (future):
- `.docker/e2e/stubs/cuss/` (registerCustomer / approveAndDeposit)
- `.docker/e2e/stubs/sms-sink/` (captures messages for OTP/magic link assertions)

### Optional: OpenAPI-driven fuzz/regression (nightly)

If you want automated negative tests with low manual effort:
- `schemathesis` (OpenAPI property-based testing) is a good nightly tool.

It is intentionally noisy; keep it out of the PR gate unless you have time to tune/seed it.

## Directory Layout (Proposed)

This repository already has Keycloak realm import under `.docker/keycloak-config/`.
For E2E, the intended structure is:
- `.docker/e2e/compose.e2e.yaml` (or root `compose.e2e.yaml` that includes this)
- `.docker/e2e/config/` (e2e config files for user-storage/worker)
- `.docker/e2e/runner/` (TS runner container)
- `.docker/e2e/stubs/` (TS stub containers with admin APIs)

