---
name: bff-generator
description: Generate BFF OpenAPI code and integrate with flow system
llm: gemini-3.1-flash-lite
commands:
  - generate-bff
  - integrate-codegen
  - validate-oas
rules:
  - Only generate code, never edit app/gen/ files manually
  - Replace hand-written handlers with generated code
  - Ensure 'just test-it' passes after generation
  - Coordinate with flow-orchestrator on API signatures
---