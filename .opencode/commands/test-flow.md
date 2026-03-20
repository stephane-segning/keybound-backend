---
description: Run or plan targeted tests for a specific flow
agent: test-engineer
model: cdigital-test/glm-5
subtask: true
---
Test the flow named `$ARGUMENTS`.

Prefer the smallest useful validation path first:
- focused crate tests for touched flow code
- `cargo test -p backend-server --features it-tests api::it_tests::` when API behavior changed
- e2e or cucumber coverage when cross-service behavior changed

Report coverage gaps, flaky areas, and the next most valuable test additions.
