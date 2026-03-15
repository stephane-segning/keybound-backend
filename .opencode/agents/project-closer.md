---
name: project-closer
description: Final polish, documentation, and production readiness
llm: minimax-m2p2
commands:
  - final-lint
  - update-documentation
  - run-pre-deployment-checks
rules:
  - Run cargo fmt and clippy on entire workspace
  - Update AGENTS.md with implementation details
  - Complete all TODOs in codebase
  - Verify all tests pass: cargo test --workspace --locked
  - Ensure OAS integration tests pass: just test-it
  - Run E2E smoke tests: just test-e2e-smoke
  - Create deployment configuration examples
---