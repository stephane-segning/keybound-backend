# FLOW SDK Implementation Plan (Utoipa + SDK Runtime)

**Status:** Execution Plan  
**Date:** 2026-03-15  
**Scope:** Full migration to SDK-driven flows, signature auth for BFF, utoipa-first APIs, legacy `sm_*` removal

## 1) Non-Negotiable Constraints

1. No new file/folder/module name may contain `revamp`.
2. BFF must not use OAuth2 claims; it must use signature-derived claims.
3. Flow logic must be implemented as concrete files and loaded at startup.
4. API surfaces should be implemented directly in Axum + utoipa (no generated OAS3 server runtime for BFF/Auth).
5. Default build should activate all flows.
6. Backend must not host SNS delivery logic; SNS remains in `sms-gateway`.
7. Keep `PHONE_OTP` and `EMAIL_MAGIC` fully separated.
8. Client signature headers to support:
   - `x-auth-signature`
   - `x-auth-signature-timestamp`
   - `x-auth-public-key`
   - `x-auth-device-id`
   - `x-auth-nonce`
   - optional `x-auth-user-id`

## 2) Target Architecture

- **Execution model:** `flow_session -> flow_instance -> flow_step` is the source of truth.
- **Runtime engine:** system steps are executed through SDK `Flow`/`Step` definitions loaded in `FlowRegistry`.
- **Auth model:** BFF and `/auth` signature validation uses bound device keys and anti-replay checks.
- **API model:** utoipa documents handlers directly; generated OAS server crates are not used for BFF/Auth runtime.
- **Legacy removal:** `sm_instance`, `sm_event`, `sm_step_attempt` code paths are retired after migration.

## 3) Workstreams and Detailed Phases

## Phase A — Signature Auth Hardening (BFF + Auth)

### A.1 Canonical signature spec (single source of truth)
- Create `app/crates/backend-server/src/auth_signature/`:
  - `canonical.rs`: payload canonicalization rules.
  - `verify.rs`: JWK parsing + signature verification.
  - `replay.rs`: nonce replay guard.
  - `mod.rs`: shared entrypoints.
- Canonical payload format (versioned in code comments + docs):
  - `timestamp\nnonce\nMETHOD\nPATH\nBODY\nPUBLIC_KEY\nDEVICE_ID\nUSER_ID_HINT`
- Validate:
  - timestamp skew (`auth.max_clock_skew_seconds`)
  - nonce uniqueness within skew window
  - `x-auth-public-key` matches bound device key
  - optional `x-auth-user-id` matches device owner when provided

### A.2 Replace insecure digest-equals-header behavior
- Ensure signature is cryptographically verified using provided/bound JWK (not plain SHA-256 equality).
- Support expected key types used by clients (start with EC P-256 + RSA; reject unsupported keys explicitly).

### A.3 Replay protection backing store
- Replace in-process nonce cache with Redis-backed replay cache for multi-instance correctness.
- Keep bounded TTL equal to skew window.

### A.4 Integrate with middleware
- `bff_signature.rs` delegates verification to shared signature module.
- `/auth` protected endpoints use same verifier.

**Acceptance criteria**
- Invalid signature, nonce replay, skew violation, key mismatch all return deterministic 401 codes/messages.
- BFF claims extraction uses middleware-provided authenticated user/device IDs only.

## Phase B — SDK Execution Contracts

### B.1 Extend SDK result model for persistence-safe updates
- In `backend-flow-sdk`:
  - add `ContextUpdates`:
    - `session_context_patch`
    - `flow_context_patch`
    - `user_metadata_patch`
  - evolve `StepOutcome::Done` to optionally carry updates/output.
- Keep SDK storage-agnostic (no DB dependency in SDK types).

### B.2 Enrich `StepContext`
- Keep read-only runtime fields (`session_context`, `flow_context`, input, identifiers).
- Add typed access helpers (previous-step output, config lookup, safe JSON traversal utilities).

### B.3 Engine application of updates
- In server flow execution service/worker:
  - apply returned patches in a transaction:
    1. patch step row
    2. update flow/session context
    3. update `app_user.metadata` when requested
    4. transition next step

**Acceptance criteria**
- SDK steps can return structured updates; server persists them without SDK-side DB access.

## Phase C — Implement `WEBHOOK_HTTP` Step Fully

### C.1 Create dedicated module
- `app/crates/backend-server/src/flow_logic/webhook_http.rs`:
  - typed config structs:
    - request method/url/headers/timeout/retry
    - payload template
    - behavior (`fire_and_forget`, `wait_for_response`, `wait_and_save`)
    - extraction rules
    - save targets
    - success conditions

### C.2 Templating + extraction
- Add interpolation utility module:
  - supports `{{session.*}}`, `{{flow.context.*}}`, `{{step.<step>.output.*}}`, `{{env.*}}`, `{{config.*}}`
- Add extraction utility with JSONPath queries.
- Save extracted values into selected targets using `ContextUpdates`.

### C.3 HTTP execution policy
- Implement timeout + retry/backoff.
- Map response failures to `StepOutcome::Failed` or `StepOutcome::Retry` deterministically.

**Acceptance criteria**
- External KYC flow can call webhook, extract fields, and persist to user metadata/contexts.

## Phase D — Dynamic Registry Loading and CLI Wiring

### D.1 Startup import becomes real runtime behavior
- `backend` binary `-i/--import` should:
  - parse definitions
  - validate
  - register into runtime `FlowRegistry`
- Add support for multiple import files / directory loading.

### D.2 Registry composition strategy
- Build base registry from built-in flow logic modules.
- Overlay imported definitions (with strict conflict policy and explicit override option).

### D.3 Export from active registry
- `export` command reads from actual active registry model, not hardcoded list.

**Acceptance criteria**
- Imported YAML/JSON changes executable flow graph at startup.
- Export reflects effective runtime registry definitions.

## Phase E — Worker Migration to `flow_step` Runtime

### E.1 New worker job model
- Poll/claim `flow_step` where:
  - `actor = SYSTEM`
  - `status` eligible for execution (`RUNNING`/`WAITING` + retry due)
- Resolve step definition via `FlowRegistry`.

### E.2 Execute + transition
- Execute step.
- Apply returned `StepOutcome` + context updates.
- Create/advance next steps using flow transitions.

### E.3 Retire legacy worker path
- Remove `sm_*` execution logic from `state_machine/engine.rs`.
- Keep deposit/otp behavior by re-implementing as SDK steps where needed.

**Acceptance criteria**
- No worker read/write dependency on `sm_step_attempt` for active flows.

## Phase F — API Surface Alignment (Utoipa-first)

### F.1 BFF
- Keep/use `api/bff_flow/*` as canonical runtime.
- Ensure handlers are fully split by responsibility (session/flow/step/auth utilities).

### F.2 Auth
- Keep axum+utoipa handlers in `api/auth.rs` (or split into `api/auth/*` modules):
  - enroll
  - bind
  - devices list/revoke
  - token
  - jwks
  - approve
  - add `userinfo`

### F.3 Security enforcement
- `/auth/approve/{stepId}` must validate JWT via existing OIDC middleware.
- Avoid Bearer-prefix-only checks.

### F.4 OpenAPI files policy
- Runtime should not depend on generated OAS server code for BFF/Auth.
- Keep OpenAPI specs for contract/reference; do not let stale specs drive runtime.

**Acceptance criteria**
- BFF/Auth runtime is pure Axum+utoipa and security-enforced.

## Phase G — Legacy Removal and Schema Finalization

### G.1 Code cleanup
- Remove legacy BFF modules based on generated BFF traits.
- Remove stale generated BFF runtime hooks from server code paths.

### G.2 DB/schema cleanup
- Add migration to drop `sm_instance`, `sm_event`, `sm_step_attempt` after data cutover validation.
- Remove `sm_*` from Diesel schema and repository traits/impls where no longer used.

### G.3 Config cleanup
- Ensure SNS logic is not pulled into backend runtime paths.
- Keep SMS sending responsibility in `sms-gateway`.

**Acceptance criteria**
- No runtime dependency on `sm_*` tables or legacy BFF generated API flow.

## 4) File-Level Change Map (Planned)

- `app/crates/backend-server/src/auth_signature/*` (new)
- `app/crates/backend-server/src/bff_signature.rs` (refactor to shared verifier)
- `app/crates/backend-server/src/api/mod.rs` (claims extraction and auth helpers)
- `app/crates/backend-server/src/api/bff_flow/*` (execution + SOLID split)
- `app/crates/backend-server/src/api/auth.rs` or `api/auth/*` (split + userinfo + JWT protection)
- `app/crates/backend-server/src/flow_logic/webhook_http.rs` (new)
- `app/crates/backend-server/src/flow_registry.rs` (import overlay + activation logic)
- `app/crates/backend-server/src/state.rs` (registry injection from startup imports)
- `app/crates/backend-server/src/state_machine/engine.rs` (migrate worker to `flow_step`)
- `app/crates/backend-flow-sdk/src/{step.rs,context.rs,flow.rs,error.rs}` (contract evolution)
- `app/bins/backend/src/main.rs` (real import/export wiring)
- `app/crates/backend-model/src/schema.rs` (remove `sm_*` after cutover)
- `app/crates/backend-repository/src/{traits.rs,pg/*}` (legacy repo cleanup)
- `app/crates/backend-migrate/migrations/*` (final drop/cutover migrations)

## 5) Delivery Order and Stop Conditions

1. Phase A (security)  
2. Phase B + C (SDK execution capabilities)  
3. Phase D (dynamic registry)  
4. Phase E (worker migration)  
5. Phase F (API/security completion)  
6. Phase G (legacy removal)

Stop only when all are true:
- Signature verification is cryptographic and replay-safe.
- System-step execution is `flow_step` + SDK-driven.
- Imported definitions are executable at runtime.
- `/auth/approve` is truly OAuth-validated.
- `WEBHOOK_HTTP` behaves as specified.
- No active production path uses `sm_*`.

## 6) Verification Gates (while tests are temporarily skipped)

- `cargo fmt --all`
- `cargo check --workspace`
- Manual API smoke via curl for:
  - signature failures/success
  - flow session/flow/step creation and transition
  - `/auth/token`, `/auth/jwks`, `/auth/approve`, `/auth/userinfo`
- Migration dry-run in dev database and schema introspection

## 7) Risks and Mitigations

- **Risk:** breaking existing clients during signature changes  
  **Mitigation:** keep header contract unchanged; harden verifier internals only.
- **Risk:** dynamic import conflicts with built-ins  
  **Mitigation:** explicit conflict policy (`reject` default, optional `override` flag).
- **Risk:** worker migration race conditions  
  **Mitigation:** transactional claim/update model and deterministic state transitions.
- **Risk:** metadata injection into JWT becomes unbounded  
  **Mitigation:** whitelist metadata keys and enforce claim size limits.

