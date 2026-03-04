# E2E Scenario Checklist

This checklist is the tracking source of truth for Compose-based end-to-end testing.

Rules of thumb:
- Prefer asserting side-effects via **Staff observability** (`/staff/*`) over raw DB reads.
- When DB reads are unavoidable, use Diesel DSL (no ad-hoc SQL strings).
- Keep scenarios deterministic: stable IDs, controllable clocks, and fault injection via stub admin APIs.

Legend:
- `[ ]` not implemented
- `[~]` partial / flaky / missing assertions
- `[x]` implemented and stable in CI

## Infrastructure (Compose)

- [ ] `compose.e2e.yaml` boots a production-like stack (user-storage + keycloak + postgres + redis + stubs) with healthchecks.
- [ ] Keycloak realm import is deterministic and versioned (no UI-clicked config drift).
- [ ] Secrets and URLs are wired through `backend-core::Config` using `${VAR}` expansion (no hardcoded env in code).
- [ ] `just test-e2e-smoke` runs a minimal scenario set and exits non-zero on failure.
- [ ] `just test-e2e-full` runs the full scenario set (nightly/optional CI).
- [ ] Logs are collected on failure (service logs + runner logs + stub captured requests).

## Health

- [ ] `GET /health` returns `200` while the server is up.

## Auth (Bearer) Layer (BFF + Staff)

Bypass / routing:
- [ ] `enabled=false` bypasses auth layer.
- [ ] blank base path does not accidentally protect unrelated routes.
- [ ] request path outside protected base paths is not validated.

Enforcement:
- [ ] missing `Authorization` -> `401`.
- [ ] non-`Bearer` scheme -> `401`.
- [ ] invalid token -> `401`.
- [ ] valid token -> handler executes.

## KC Signature Middleware

Enforcement cases (must remain covered):
- [ ] missing `x-kc-timestamp`.
- [ ] missing `x-kc-signature`.
- [ ] invalid/out-of-skew timestamp.
- [ ] invalid signature.
- [ ] request body larger than `max_body_bytes`.
- [ ] valid signature with body preservation (downstream handler sees original body).
- [ ] url encoded paths.
- [ ] nested router paths.
- [ ] method mismatch.
- [ ] path mismatch.
- [ ] body mismatch.

## KC Surface (`/kc/*`) Functional Scenarios

Devices:
- [ ] Lookup existing device by `(device_id, jkt)` returns `200` and refreshes `last_seen_at`.
- [ ] Lookup missing device returns `404`.

Enrollment / binding:
- [ ] Bind device for user succeeds (first bind).
- [ ] Re-bind same `(device_id, jkt)` to same user is idempotent (or returns deterministic outcome).
- [ ] Bind conflict: same device already bound to different user -> `409` with deterministic error.
- [ ] Uniqueness enforced on both `device_id` and `jkt` under concurrency (race test).
- [ ] `device_record_id` is deterministic (`device_id` + SHA-256(sorted JWK)).

Users:
- [ ] Create user -> `201`.
- [ ] Get existing user -> `200`.
- [ ] Get missing user -> `404`.
- [ ] Update existing user -> `200`.
- [ ] Update missing user -> `404`.
- [ ] Delete existing user -> `204`.
- [ ] Delete missing user -> `404`.
- [ ] Search users returns expected results (and stable ordering if defined).

## BFF Surface (`/bff/*`) OpenAPI Coverage

From [openapi/user-storage-bff.yaml](../../openapi/user-storage-bff.yaml):

Deposits:
- [ ] `POST /internal/deposits/phone` (`internalCreatePhoneDeposit`) happy path.
- [ ] `POST /internal/deposits/phone` ownership/auth enforced (no bearer -> `401`).
- [ ] `GET /internal/deposits/{depositId}` (`internalGetPhoneDeposit`) happy path.
- [ ] `GET /internal/deposits/{depositId}` denies non-owner (`403` or `404`, whichever is specified).
- [ ] deposit expiry behavior (if specified) is enforced.

Sessions / steps:
- [ ] `POST /internal/kyc/sessions` (`internalStartSession`) create/resume is idempotent (or deterministic).
- [ ] `POST /internal/kyc/steps` (`internalCreateStep`) creates each supported step type (phone/email/address/identity).
- [ ] `GET /internal/kyc/steps/{stepId}` (`internalGetStep`) returns correct data/status transitions.

Phone OTP:
- [ ] `POST /internal/kyc/phone/otp/issue` (`internalIssueOtp`) issues challenge; SMS is sent (captured by stub/sink).
- [ ] verify correct OTP -> step moves to verified state.
- [ ] verify wrong OTP -> deterministic error and `sm_step_attempt` increments.
- [ ] verify expired OTP -> deterministic error.
- [ ] rate limits / max attempts enforced (if configured).

Email magic link:
- [ ] `POST /internal/kyc/email/magic/issue` (`internalIssueMagicEmail`) issues a link/token (captured by sink).
- [ ] `POST /internal/kyc/email/magic/verify` (`internalVerifyMagicEmail`) verifies and advances step.

Uploads:
- [ ] `POST /internal/uploads/presign` (`internalPresignUpload`) returns valid presign URL/fields.
- [ ] `POST /internal/uploads/complete` (`internalCompleteUpload`) completes upload metadata.
- [ ] invalid upload completion -> deterministic error.

## Staff Surface (`/staff/*`) OpenAPI Coverage

From [openapi/user-storage--staff.yaml](../../openapi/user-storage--staff.yaml):

State-machine observability:
- [ ] `GET /api/kyc/instances` (`staffKycInstancesGet`) returns instances with filters/pagination (as defined).
- [ ] `GET /api/kyc/instances/{instanceId}` (`staffKycInstancesInstanceIdGet`) returns instance + events + attempts.
- [ ] `POST /api/kyc/instances/{instanceId}/retry` (`staffKycInstancesInstanceIdRetryPost`) schedules/retries and is observable.

Deposit flow (KYC_FIRST_DEPOSIT):
- [ ] `POST /api/kyc/deposits/{instanceId}/confirm-payment` (`staffKycDepositsInstanceIdConfirmPaymentPost`) moves instance state.
- [ ] `POST /api/kyc/deposits/{instanceId}/approve` (`staffKycDepositsInstanceIdApprovePost`) triggers worker path and is observable.

Reports:
- [ ] `GET /api/kyc/reports/summary` (`staffKycReportsSummaryGet`) returns correct aggregates for known fixtures.

## Worker / Queue / Retry Scenarios

Redis readiness / locking:
- [ ] worker enforces single-consumer via Redis lock (two workers started -> only one processes).

SMS retries:
- [ ] transient SMS provider error is retried with backoff until success.
- [ ] permanent SMS error moves to terminal state (no infinite retries).

KYC_FIRST_DEPOSIT -> CUSS integration:
- [ ] success: staff confirm -> approve -> worker calls CUSS `registerCustomer` then `approveAndDeposit` -> instance completes.
- [ ] CUSS failure on `registerCustomer` retries and remains observable.
- [ ] CUSS failure on `approveAndDeposit` retries and remains observable.
- [ ] idempotency: repeated approve does not double-deposit (or is rejected deterministically).

## Error Mapping (Representative)

Across at least one BFF and one Staff endpoint:
- [ ] validation errors map to stable status + payload shape.
- [ ] not found maps to stable status + payload shape.
- [ ] conflict maps to stable status + payload shape.
- [ ] unexpected internal error maps to stable status + payload shape.

