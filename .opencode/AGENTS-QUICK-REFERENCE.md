# Opencode AI Agents - Quick Reference

This document provides a complete reference for using opencode agents and commands to complete the flow SDK implementation project.

## 📦 What's Been Created

### 1. Agent Configurations (10 agents)
Located in `.opencode/agents/`

| Agent | LLM Model | Primary Purpose |
|-------|-----------|-----------------|
| **agent-orchestrator** | gemini-2.5-pro | Master coordinator for all agents |
| **bff-generator** | gemini-3.1-flash-lite | Generate BFF OpenAPI code |
| **flow-architect** | kimi-k2-thinking | Design integration traits and patterns |
| **flow-otp-master** | deepseek-v3p2 | Implement Phone OTP flow |
| **flow-email-wizard** | cogito-671b-v2-p1 | Implement Email Magic flow |
| **flow-deposit-builder** | kimi-k2-instruct | Implement First Deposit flow |
| **integration-specialist** | gemini-2.5-flash | Document verification flows |
| **test-engineer** | qwen3-vl-30b-a3b-thinking | Write comprehensive tests |
| **project-closer** | minimax-m2p2 | Final polish and delivery |
| **flow-orchestrator** | gemini-2.5-pro | Project coordination (legacy) |

### 2. Executable Commands (10 commands)
Located in `.opencode/commands/` (all executable)

| Command | Usage | Description |
|---------|-------|-------------|
| `daily-standup` | Run anytime | Project status check |
| `generate-bff` | `opencode run generate-bff` | Generate OpenAPI code |
| `validate-flow` | `opencode run validate-flow <name>` | Validate flow implementation |
| `check-workspace` | Run anytime | Full workspace validation |
| `test-flow` | `opencode run test-flow <name>` | Test specific flow |
| `implement-otp-flow` | Run once | Generate OTP flow boilerplate |
| `coordinate-agents` | `opencode run coordinate-agents` | Run all agents in order |
| `coordinate-phase` | `opencode run coordinate-phase <name>` | Run specific project phase |
| `resolve-conflicts` | Resolve agent conflicts | Detect and fix agent conflicts |
| `track-progress` | View all agent status | Progress tracking across agents |

### 3. Documentation (4 files)
- `.opencode/README.md` - General usage guide
- `.opencode/config.json` - Master configuration
- `.opencode/PROJECT-EXECUTION.md` - Project phases and tasks
- `.opencode/QUICKSTART.md` - Quick reference

## 🚀 Quick Start

### Install and Configure Opencode

```bash
# Ensure opencode is installed
which opencode

# Initialize the project
opencode init

# Verify agents are loaded
opencode agent list
```

### Run First Standup

```bash
opencode run --agent agent-orchestrator daily-standup
```

Expected output shows:
- Current branch
- Uncommitted changes
- Last commit
- Compilation status
- Feature flags
- TODOs/FIXMEs

### Use Specialized Commands

```bash
# Generate BFF OpenAPI code
opencode run --agent bff-generator generate-bff

# Validate a flow
opencode run --agent flow-otp-master validate-flow phone_otp

# Run full workspace check
opencode run --agent agent-orchestrator check-workspace
```

## 📖 Key Commands Reference

### Project Management

**Daily Standup:**
```bash
opencode run --agent agent-orchestrator daily-standup
```
Checks: compilation, tests, feature flags, git status, TODOs

**Full Workspace Check:**
```bash
opencode run --agent agent-orchestrator check-workspace
```
Verifies: format, clippy, compilation, unit tests, feature flags

### Agent Orchestration

**Run All Agents in Order:**
```bash
opencode run --agent agent-orchestrator coordinate-agents
```

**Run Specific Phase:**
```bash
opencode run --agent agent-orchestrator coordinate-phase foundation
opencode run --agent agent-orchestrator coordinate-phase core-flows
opencode run --agent agent-orchestrator coordinate-phase advanced
opencode run --agent agent-orchestrator coordinate-phase testing
opencode run --agent agent-orchestrator coordinate-phase delivery
```

### Flow Development

**Validate Flow Implementation:**
```bash
opencode run --agent flow-otp-master validate-flow phone_otp
```
Checks: file exists, trait impl, feature flag, registry, compilation

**Test Flow:**
```bash
opencode run --agent test-engineer test-flow phone_otp
```
Runs: unit tests and coverage for specific flow

**Implement OTP Flow (Boilerplate):**
```bash
opencode run --agent flow-otp-master implement-otp-flow
```
Generates: full OTP flow implementation with step trait implementations

### Code Generation

**Generate BFF Code:**
```bash
opencode run --agent bff-generator generate-bff
```
Runs: just generate, verifies compilation

## 🔧 Project Phases (Flexible Order)

### Phase 1: Foundation & Code Generation
**When ready, run:**
```bash
# Generate BFF OpenAPI code
opencode run --agent bff-generator generate-bff
```

**Agent responsibilities:**
- `flow-architect`: Design integration traits (SmsProvider, EmailProvider, etc.)
- `flow-architect`: Create flow patterns documentation
- `bff-generator`: Generate and integrate OpenAPI code

**Deliverables:**
- Generated BFF handlers in `app/gen/oas_server_bff/`
- Integration trait definitions
- Flow pattern documentation

### Phase 2: Core Flow Implementation  
**After foundation is complete:**
```bash
# Generate OTP flow boilerplate
opencode run --agent flow-otp-master implement-otp-flow

# Validate flow implementation
opencode run --agent flow-otp-master validate-flow phone_otp
```

**Agent responsibilities:**
- `flow-otp-master`: Implement Phone OTP flow (IssuePhoneOtpStep, VerifyPhoneOtpStep)
- `flow-email-wizard`: Implement Email Magic flow
- `flow-otp-master`: Add rate limiting and retry logic
- `flow-email-wizard`: Create magic link generation and verification

**Deliverables:**
- Working Phone OTP flow with SMS integration
- Working Email Magic flow with secure tokens
- Flow registered in registry
- Unit tests for both flows

### Phase 3: Advanced Flows
**After core flows work:**
```bash
# Validate deposit flow
opencode run --agent flow-deposit-builder validate-flow first_deposit

# Validate document flows
opencode run --agent integration-specialist validate-flow id_document
opencode run --agent integration-specialist validate-flow address_proof
```

**Agent responsibilities:**
- `flow-deposit-builder`: Implement First Deposit flow
- `flow-deposit-builder`: Integrate with CUSS client
- `integration-specialist`: Implement ID Document flow
- `integration-specialist`: Implement Address Proof flow
- `integration-specialist`: Create file upload integration

**Deliverables:**
- First Deposit flow with payment processing
- Document upload flows
- Admin verification workflows
- File storage integration

### Phase 4: Testing
**After flows are implemented:**
```bash
# Test all flows
for flow in phone_otp email_magic first_deposit id_document address_proof; do
    opencode run --agent test-engineer test-flow $flow
done
```

**Agent responsibilities:**
- `test-engineer`: Write unit tests for flow SDK core
- `test-engineer`: Create integration tests for orchestration
- `test-engineer`: Add OAS integration tests
- `test-engineer`: Run coverage reports (target: >80%)

**Deliverables:**
>80% test coverage for flow logic
All flows have unit + integration tests
E2E test scenarios documented

### Phase 5: Final Delivery
**After all flows and tests pass:**
```bash
# Full workspace validation
opencode run --agent agent-orchestrator check-workspace

# Final lint and documentation
opencode run --agent project-closer final-lint
opencode run --agent project-closer update-documentation
```

**Agent responsibilities:**
- `project-closer`: Run cargo fmt and clippy
- `project-closer`: Update AGENTS.md with implementation details
- `project-closer`: Complete all TODOs
- `project-closer`: Verify all tests pass
- `project-closer`: Create deployment configs

**Deliverables:**
Zero clippy warnings
Updated documentation
All tests passing
Production-ready codebase

## 🔧 Agent Customization

### Add New Command
```bash
# Create new command file
touch .opencode/commands/my-command
chmod +x .opencode/commands/my-command

# Edit and add logic
nano .opencode/commands/my-command
```

### Modify Agent LLM
Edit agent JSON file to change LLM models based on your needs.

## ✅ Success Verification

Run this to verify completion:

```bash
#!/bin/bash
echo "=== Flow SDK Project Completion Check ==="

# All flows validated
flows=("phone_otp" "email_magic" "first_deposit" "id_document" "address_proof")
for flow in "${flows[@]}"; do
    echo "Validating: $flow"
    opencode run --agent agent-orchestrator validate-flow "$flow"
done

# Workspace checks
echo ""
echo "Running workspace checks..."
opencode run --agent agent-orchestrator check-workspace

# Integration tests
echo ""
echo "Running integration tests..."
just test-it
just test-e2e-smoke

echo ""
echo "=== Project Delivery Complete ==="
```

**Final Delivery Checklist:**
- [ ] All flows implemented with actual business logic
- [ ] BFF API code generated and integrated
- [ ] Test coverage >80% for flow logic
- [ ] Zero clippy warnings across workspace
- [ ] All tests pass: `cargo test --workspace --locked`
- [ ] OAS integration tests pass: `just test-it`
- [ ] E2E smoke tests pass: `just test-e2e-smoke`
- [ ] Documentation updated in AGENTS.md
- [ ] Deployment configs verified

## 📞 Troubleshooting

### Command Not Found
```bash
# Check PATH
echo $PATH | grep opencode

# Find opencode
which opencode
```

### Agent Not Loading
```bash
# Check agent file validity
json_pp -t null < .opencode/agents/agent-name.md 2>/dev/null || echo "Invalid YAML frontmatter"
```

### Permission Denied
```bash
# Make commands executable
chmod +x .opencode/commands/*
```

## 🎉 Next Steps

1. **Read**: `.opencode/README.md` for usage guide
2. **Read**: `.opencode/PROJECT-EXECUTION.md` for project phases
3. **Run**: `opencode run --agent agent-orchestrator daily-standup`
4. **Execute**: Start with Phase 1 tasks
5. **Monitor**: Use standup command regularly

---

**Full documentation**: `.opencode/README.md`
**Execution plan**: `.opencode/PROJECT-EXECUTION.md`
**Agent configs**: `.opencode/agents/*.md`