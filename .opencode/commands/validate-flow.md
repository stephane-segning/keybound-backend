---
description: Validate a flow implementation from YAML to Rust wiring, API exposure, and tests
model: cdigital-test/glm-5
---
Validate the flow named `$ARGUMENTS` in this repository.

Check:
- YAML under `flows/`
- any matching session manifest under `sessions/`
- Rust registration and definitions under `app/crates/backend-server/src/flows/`
- related KC, BFF, and Staff endpoints
- targeted tests and e2e coverage

Call out missing wiring, broken invariants, or stale tests before marking the flow as valid.
