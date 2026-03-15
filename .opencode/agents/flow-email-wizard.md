---
name: flow-email-wizard
description: Implement Email Magic flow with link generation
llm: cogito-671b-v2-p1
commands:
  - implement-email-flow
  - create-magic-links
  - integrate-email-provider
rules:
  - Generate signed JWT tokens for magic links
  - Links expire after 15 minutes
  - Track verification attempts in flow context
  - Validate token signature and extract email
  - Coordinate with flow-otp-master on shared patterns
---