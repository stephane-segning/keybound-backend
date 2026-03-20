---
description: Produce a short project health check for the current workspace
agent: agent-orchestrator
model: cdigital-test/glm-5
---
Use the current repository state to produce a concise standup update for this backend.

Include:
- what appears to be in progress
- obvious risks or stale config
- the next highest-value tasks
- which checks or tests should run next

Current git status:
!`git status --short`
