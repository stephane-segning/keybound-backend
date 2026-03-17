# Opencode Agents and Commands

This directory contains agent configurations and commands for the opencode CLI tool to manage the flow SDK implementation project.

## 🚀 Quick Start

```bash
# List all agents
opencode agent list

# Run a command with specific agent
opencode run --agent flow-orchestrator daily-standup

# Run a flow validation
opencode run validate-flow phone_otp

# Test a flow implementation
opencode run test-flow phone_otp

# Run full workspace validation
opencode run check-workspace
```

## 👥 Agents

### Project Management
- **flow-orchestrator** (gemini-2.5-pro): Project coordinator and architectural gatekeeper
- **project-closer** (minimax-m2p2): Final polish and production readiness

### Code Generation
- **bff-generator** (gemini-3.1-flash-lite): Generate BFF OpenAPI code

### Architecture & Design
- **flow-architect** (kimi-k2-thinking): Design patterns and integration traits

### Flow Implementation
- **flow-otp-master** (deepseek-v3p2): Phone OTP flow implementation
- **flow-email-wizard** (cogito-671b-v2-p1): Email Magic flow implementation
- **flow-deposit-builder** (kimi-k2-instruct-0905): First Deposit flow implementation
- **integration-specialist** (gemini-2.5-flash): Document verification flows

### Testing
- **test-engineer** (qwen3-vl-30b-a3b-thinking): Comprehensive testing

## 📋 Available Commands

### Project Management
- `daily-standup`: Run project status check
- `check-workspace`: Full workspace validation (fmt, clippy, tests)

### Flow Development
- `validate-flow <name>`: Validate a flow implementation
- `test-flow <name>`: Run tests for a specific flow
- `implement-otp-flow`: Generate Phone OTP flow boilerplate

### Code Generation
- `generate-bff`: Generate BFF OpenAPI code

## 🔧 Project Phases

The project can be executed in any order, but recommended sequence:

**Phase 1: Foundation**
- Run: `bff-generator generate-bff`
- Agent work: flow-architect designs integration traits

**Phase 2: Core Flows**  
- Run: `flow-otp-master validate-flow phone_otp`
- Run: `flow-email-wizard validate-flow email_magic`

**Phase 3: Advanced Flows**
- Run: `flow-deposit-builder validate-flow first_deposit`
- Run: `integration-specialist validate-flow id_document`

**Phase 4: Testing**
- Run: `test-engineer test-flow phone_otp`
- Run: `test-engineer run-coverage-report`

**Phase 5: Delivery**
- Run: `project-closer final-lint`
- Run: `project-closer update-documentation`

## 🤝 Collaboration Workflow

1. **Standup**: Run `daily-standup` regularly for status
2. **Feature Branches**: Work on branches named `agent/<name>/<feature>`
3. **Code Review**: All PRs require review from flow-orchestrator
4. **Merge Strategy**: Rebase onto `feat/flow-sdk-completed` branch

## 🎯 Success Criteria

- All 8 agents complete their assigned tasks
- Flow SDK fully implemented with all flows
- Test coverage >80%
- Zero clippy warnings
- All tests pass
- Production-ready codebase

## 📖 Configuration

Edit `.opencode/config.json` to modify:
- Agent assignments
- Collaboration settings
- Project metadata