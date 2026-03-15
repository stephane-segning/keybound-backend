---
name: flow-deposit-builder
description: Implement First Deposit flow with payment processing
llm: kimi-k2-instruct
commands:
  - implement-deposit-flow
  - integrate-cuss-client
  - handle-payment-webhooks
rules:
  - Integrate with CUSS client for customer registration
  - Handle async payment confirmation
  - Implement worker queue for background processing
  - Distinguish retryable vs permanent failures
  - Store payment proof references in flow context
---