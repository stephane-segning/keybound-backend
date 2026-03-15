# AGENTS.md

## Repository Overview

Tokenization/user-storage backend with three HTTP surfaces:
- **KC**: `/kc/*` - Keycloak integration
- **BFF**: `/bff/*` - Backend for Frontend  
- **Staff**: `/staff/*` - Staff/admin operations

**Architecture**: Rust workspace with native `axum` runtime, strict `controller -> repository` layering, and Diesel-async for database access.

## Build, Test & Lint Commands

### Quick Development Cycle
```bash
# Run backend in dev mode with logs
just dev

# Run a single test by name
cargo test -p <crate> <test_name>--- --exact --nocapture

# Run tests for a specific crate
cargo test -p backend-server
cargo test -p backend-core
cargo test -p backend-auth
cargo test -p backend-repository

# Run all workspace tests
cargo test --workspace --locked

# Run only unit tests (skip integration tests)
cargo test --workspace --lib
```

### Integration & E2E Testing
```bash
# OAS3 integration tests (requires it-tests feature)
just test-it
cargo test -p backend-server --features it-tests api::it_tests::

# Rust-native E2E tests with external deps (requires e2e-tests feature)
cargo test -p backend-auth --features e2e-tests --test oidc_wiremock_e2e
cargo test -p backend-repository --features e2e-tests --test state_machine_repo_testcontainers

# Compose E2E tests (full stack)
just test-e2e-smoke  # Quick smoke tests
just test-e2e-full   # Full test suite
```

### Linting & Code Quality
```bash
# Format code
cargo fmt

# Run clippy with fixes
cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings

# Check workspace compilation
cargo check --workspace

# Run all checks (format, clippy, fix)
just all-checks
```

### Running a Single Test

For unit tests:
```bash
cargo test -p backend-server state::tests::test_name -- --exact --nocapture
```

For integration tests:
```bash
cargo test -p backend-server --features it-tests api::it_tests::test_name -- --exact --nocapture
```

For repository tests:
```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/user-storage \
  cargo test -p backend-repository --test device_repo
```

## Code Style Guidelines

### SOLID Principles (Applied in This Codebase)

The codebase follows SOLID principles for maintainable, testable Rust code:

#### **S - Single Responsibility**
Each module has one clear purpose. Controllers handle HTTP, repositories handle database, services orchestrate.

```rust
// ✅ GOOD: UserRepository only handles user database operations
pub struct UserRepository {
    pool: Pool<AsyncPgConnection>,
}

impl UserRepo for UserRepository {
    async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>> {
        // Only user retrieval logic
        users.find(id).first(&mut conn).await.optional().map_err(Into::into)
    }
    
    async fn create_user(&self, input: &UserUpsert) -> RepoResult<UserRow> {
        // Only user creation logic
    }
}

// ❌ BAD: Mixed responsibilities
struct UserHandler {
    db_pool: Pool<AsyncPgConnection>,  // Database logic
    email_client: EmailClient,         // External service
    validator: Validator,              // Validation logic
    // This would violate SRP
}
```

#### **O - Open/Closed Principle**
Extend behavior via traits and feature flags, not by modifying existing code.

```rust
// ✅ GOOD: Open for extension via trait
pub trait SmsProvider: Send + Sync {
    async fn send_otp(&self, phone: &str, otp: &str) -> Result<(), SmsError>;
}

// Extend with new implementations without modifying trait
pub struct ConsoleSmsProvider;
pub struct SnsSmsProvider;

// Feature-gated extension
#[cfg(feature = "flow-phone-otp")]
registry.register_step(Arc::new(IssuePhoneOtpStep));
```

#### **L - Liskov Substitution**
Trait objects can be substituted without breaking behavior.

```rust
// ✅ GOOD: All repositories implement same trait
pub trait UserRepo {
    async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>>;
}

// In AppState, any implementation works
pub struct AppState {
    pub user_repo: Arc<dyn UserRepo>,  // Can be mock or real
}

// Usage remains consistent
let user = state.user_repo.get_user("usr_123").await?;
```

#### **I - Interface Segregation**
Small, focused traits rather than monolithic interfaces.

```rust
// ✅ GOOD: Separate traits for different concerns
pub trait DeviceRepo {
    async fn create_device(&self, input: &DeviceBindInput) -> RepoResult<DeviceRow>;
    async fn lookup_device(&self, id: &str, jkt: &str) -> RepoResult<Option<DeviceRow>>;
}

pub trait FlowRepo {
    async fn create_session(&self, input: FlowSessionCreateInput) -> RepoResult<FlowSessionRow>;
    async fn create_instance(&self, input: FlowInstanceCreateInput) -> RepoResult<FlowInstanceRow>;
}

// ❌ BAD: One trait for everything
pub trait BackendRepo {  // Too large!
    async fn user_ops(&self, ...) -> ...;
    async fn device_ops(&self, ...) -> ...;
    async fn flow_ops(&self, ...) -> ...;
}
```

#### **D - Dependency Inversion**
Depend on abstractions (traits) not concrete implementations.

```rust
// ✅ GOOD: Controller depends on trait
pub struct DeviceController {
    device_repo: Arc<dyn DeviceRepo>,  // Abstract dependency
}

impl DeviceController {
    pub async fn bind_device(&self, input: BindDeviceRequest) -> Result<DeviceRecordId> {
        let device = self.device_repo.create_device(&input).await?;
        Ok(device.device_record_id)
    }
}

// In app setup (main.rs or binary)
let device_repo = Arc::new(DeviceRepository::new(pool));
let controller = DeviceController { device_repo };

// For tests: inject mock
let mock_repo = Arc::new(MockDeviceRepo::new());
let controller = DeviceController { device_repo: mock_repo };
```

### Architectural Patterns

#### **Repository Pattern (Data Access Layer)**
Isolates database logic behind traits.

```rust
// Trait definition (domain interface)
#[async_trait]
pub trait UserRepo {
    async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>>;
    async fn create_user(&self, input: &UserUpsert) -> RepoResult<UserRow>;
}

// Implementation (infrastructure)
pub struct UserRepository {
    pool: Pool<AsyncPgConnection>,
}

#[async_trait]
impl UserRepo for UserRepository {
    async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>> {
        use backend_model::schema::app_user::dsl::*;
        let mut conn = self.get_conn().await?;
        
        app_user
            .filter(user_id.eq(id))
            .first::<UserRow>(&mut conn)
            .await
            .optional()
            .map_err(Into::into)  // Maps Diesel -> backend_core::Error
    }
}

// Usage in controller
pub async fn handler(state: State<AppState>, Path(user_id): Path<String>) -> Result<Json<User>> {
    let user = state.user_repo.get_user(&user_id).await?;
    Ok(Json(user))
}
```

#### **Flow SDK Pattern (Orchestration)**
Registry-based workflow engine with step-driven state machine.

```rust
// Step trait (extensible workflow building block)
#[async_trait]
pub trait Step: Send + Sync {
    fn step_type(&self) -> &'static str;
    fn actor(&self) -> Actor;
    fn feature(&self) -> Option<&'static str>;
    
    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError>;
}

// Registry pattern (central lookup)
pub struct FlowRegistry {
    steps: HashMap<String, Arc<dyn Step>>,
    flows: HashMap<String, Arc<dyn Flow>>,
}

impl FlowRegistry {
    pub fn register_step(&mut self, step: Arc<dyn Step>) {
        self.steps.insert(step.step_type().to_owned(), step);
    }
    
    pub fn get_step(&self, step_type: &str) -> Option<&dyn Step> {
        self.steps.get(step_type).map(Arc::as_ref)
    }
}

// Concrete implementation
pub struct IssuePhoneOtpStep;

#[async_trait]
impl Step for IssuePhoneOtpStep {
    fn step_type(&self) -> &'static str { "ISSUE_PHONE_OTP" }
    fn actor(&self) -> Actor { Actor::System }
    fn feature(&self) -> Option<&'static str> { Some("flow-phone-otp") }
    
    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        // Business logic here
        Ok(StepOutcome::Done { output: None, updates: None })
    }
}
```

#### **Strategy Pattern (Feature Gating)**
Conditional compilation for flow modules.

```rust
// In backend-server/src/flow_registry.rs
pub fn build_registry() -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    
    // Static flows with feature flags
    #[cfg(feature = "flow-phone-otp")]
    {
        registry.register_flow(Arc::new(PhoneOtpFlow));
        registry.register_step(Arc::new(IssuePhoneOtpStep));
        registry.register_step(Arc::new(VerifyPhoneOtpStep));
    }
    
    #[cfg(feature = "flow-email-magic")]
    {
        registry.register_flow(Arc::new(EmailMagicFlow));
        registry.register_step(Arc::new(IssueEmailMagicStep));
        registry.register_step(Arc::new(VerifyEmailMagicStep));
    }
    
    // Dynamic flows loaded from YAML at runtime
    if let Some(dynamic_flows) = config.dynamic_flows {
        for flow_def in dynamic_flows {
            apply_flow_import(&mut registry, flow_def)?;
        }
    }
    
    registry
}
```

#### **MVC Pattern (Request Flow)**
Controller → Service → Repository → Model.

```rust
// Model (data structures - in backend-model)
#[derive(Queryable, Selectable)]
#[diesel(table_name = app_user)]
pub struct UserRow {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub metadata: Option<Value>,
}

// Controller (HTTP handlers - in backend-server/src/api/)
pub async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>
) -> Result<Json<UserResponse>, Error> {
    // Extract params, call service/repo
    let user = state.user_repo.get_user(&user_id).await?;
    let device = state.device_repo.lookup_device(&user_id, &jkt).await?;
    
    Ok(Json(UserResponse {
        user_id: user.user_id,
        devices: vec![device],
    }))
}

// Repository (data access - in backend-repository/src/pg/)
pub struct UserRepository {
    pool: Pool<AsyncPgConnection>,
}

impl UserRepo for UserRepository {
    async fn get_user(&self, id: &str) -> RepoResult<Option<UserRow>> {
        // Diesel query here
    }
}

// Request/Response DTOs
#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub user_id: String,
    pub devices: Vec<DeviceRow>,
}
```

#### **Middleware Pattern (Authentication)**
Composable HTTP layer behavior.

```rust
// In backend-auth/src/jwt_auth.rs
pub struct JwtAuthLayer {
    pub oidc_state: Arc<OidcState>,
    pub validate_expiry: bool,
}

impl<S> Layer<S> for JwtAuthLayer {
    type Service = JwtAuthService<S>;
    
    fn layer(&self, inner: S) -> Self::Service {
        JwtAuthService {
            inner,
            oidc_state: self.oidc_state.clone(),
            validate_expiry: self.validate_expiry,
        }
    }
}

// Usage in router
Router::new()
    .route("/api/staff/*", staff_routes())
    .layer(JwtAuthLayer {
        oidc_state: app_state.oidc_state.clone(),
        validate_expiry: true,
    })
```

#### **Factory Pattern (Database Connections)**
Centralized resource creation.

```rust
// In backend-migrate/src/lib.rs
pub struct DbFactory {
    pool: Pool<AsyncPgConnection>,
}

impl DbFactory {
    pub async fn postgres(url: &str) -> Result<Self, Error> {
        let config = AsyncDieselConnectionManager::new(url);
        let pool = Pool::builder(config).build()?;
        
        // Run migrations
        let mut conn = pool.get().await?;
        conn.run_pending_migrations(MIGRATIONS)?;
        
        Ok(Self { pool })
    }
    
    pub fn get_repo<T: From<Pool<AsyncPgConnection>>>(&self) -> T {
        T::from(self.pool.clone())
    }
}

// Usage
let factory = DbFactory::postgres("postgres://...").await?;
let user_repo: UserRepository = factory.get_repo();
let device_repo: DeviceRepository = factory.get_repo();
```

#### **Command Pattern (Background Jobs)**
Encapsulate operations as objects for async execution.

```rust
// Job definitions
#[derive(Serialize, Deserialize)]
pub enum NotificationJob {
    OtpSms { phone: String, otp: String },
    EmailVerification { email: String, token: String },
}

// Queue trait
pub trait NotificationQueue {
    async fn enqueue(&self, job: NotificationJob) -> Result<()>;
}

// Implementation (Redis-backed)
pub struct RedisNotificationQueue {
    redis: redis::aio::ConnectionManager,
}

#[async_trait]
impl NotificationQueue for RedisNotificationQueue {
    async fn enqueue(&self, job: NotificationJob) -> Result<()> {
        let serialized = serde_json::to_string(&job)?;
        self.redis.rpush("notification_queue", serialized).await?;
        Ok(())
    }
}

// Worker processing
pub async fn process_notification_queue(state: Arc<AppState>) -> Result<()> {
    loop {
        let job: NotificationJob = state.redis.blpop("notification_queue").await?;
        match job {
            NotificationJob::OtpSms { phone, otp } => {
                state.sms_provider.send_otp(&phone, &otp).await?;
            }
        }
    }
}
```

#### **Observer Pattern (Event Tracking)**
Decoupled event handling for audit trails.

```rust
// Event types
#[derive(Serialize)]
pub struct FlowEvent {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub flow_type: String,
    pub step_type: String,
    pub status: String,
    pub metadata: Option<Value>,
}

// Event sink trait
pub trait EventSink: Send + Sync {
    async fn record(&self, event: FlowEvent) -> Result<()>;
}

// Multiple implementations
pub struct DatabaseEventSink {
    pool: Pool<AsyncPgConnection>,
}

pub struct LoggingEventSink {
    logger: Logger,
}

// Composite sink (Observer pattern)
pub struct CompositeEventSink {
    sinks: Vec<Box<dyn EventSink>>,
}

impl EventSink for CompositeEventSink {
    async fn record(&self, event: FlowEvent) -> Result<()> {
        for sink in &self.sinks {
            sink.record(event.clone()).await?;
        }
        Ok(())
    }
}

// Usage in flow execution
impl Step for PhoneOtpStep {
    async fn execute(&self, ctx: &StepContext) -> Result<StepOutcome, FlowError> {
        let outcome = self.execute_business_logic(ctx).await?;
        
        // Notify observers
        let event = FlowEvent {
            timestamp: Utc::now(),
            session_id: ctx.session_id.clone(),
            flow_type: "PHONE_OTP".to_string(),
            step_type: self.step_type().to_string(),
            status: outcome.status(),
            metadata: None,
        };
        ctx.event_sink.record(event).await?;
        
        Ok(outcome)
    }
}
```

### Feature Flag & Configuration Patterns

#### **Compile-Time Features**
```toml
# In Cargo.toml
[features]
default = ["all-flows"]
all-flows = ["flow-phone-otp", "flow-email-magic", "flow-first-deposit"]
flow-phone-otp = ["flow-sdk"]
flow-email-magic = ["flow-sdk"]
flow-sdk = ["dep:backend-flow-sdk"]
```

```rust
// Conditional compilation
#[cfg(feature = "flow-phone-otp")]
{
    registry.register_flow(Arc::new(PhoneOtpFlow));
}

// Runtime validation
registry.validate_features(&["flow-phone-otp", "flow-email-magic"])?;
```

#### **Environment-Based Configuration**
```rust
// In backend-core/src/config.rs
#[derive(Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub sms: SmsConfig,
}

#[derive(Deserialize)]
pub struct SmsConfig {
    pub provider: String,  // "console" or "sns"
    pub aws_region: Option<String>,
}

// Usage with expansion
let config = Config::load("config/app.yaml")?;
// ${AWS_REGION:-us-east-1} expands automatically
```

### Error Handling Patterns

#### **Result Type Aliases**
```rust
// Standard result with custom error
type Result<T> = std::result::Result<T, backend_core::Error>;
type RepoResult<T> = Result<T>;  // From repository layer

// Never use generic Box<dyn Error>
// ❌ BAD: fn foo() -> Result<T, Box<dyn std::error::Error>>
// ✅ GOOD: fn foo() -> Result<T, backend_core::Error>
```

#### **Error Mapping**
```rust
// Diesel to application error
app_user
    .filter(user_id.eq(id))
    .first::<UserRow>(&mut conn)
    .await
    .optional()
    .map_err(Into::into)  // DieselError -> backend_core::Error

// Custom error conversion
impl From<argon2::Error> for backend_core::Error {
    fn from(err: argon2::Error) -> Self {
        backend_core::Error::InvalidArgument(err.to_string())
    }
}
```

#### **Error Context**
```rust
// Add context to errors
let user = state.user_repo.get_user(&user_id).await
    .map_err(|e| e.with_context(|| format!("Failed to get user: {}", user_id)))?;

// Pattern matching
match result {
    Err(backend_core::Error::NotFound(_)) => {
        // Handle not found
    }
    Err(e) => return Err(e),
    Ok(user) => user,
}
```

## Opencode AI Agents

This project includes 10 specialized AI agents in `.opencode/agents/` for automated code generation, architecture design, flow implementation, testing, and project coordination.

### Available Agents

| Agent | LLM Model | Primary Purpose | When to Use |
|-------|-----------|-----------------|-------------|
| **agent-orchestrator** | gemini-2.5-pro | Master coordinator for all agents | When you need to orchestrate multiple agents or resolve conflicts |
| **bff-generator** | gemini-3.1-flash-lite | Generate BFF OpenAPI code | To generate BFF API handlers from OpenAPI specs |
| **flow-architect** | kimi-k2-thinking | Design integration traits and patterns | To define SmsProvider, EmailProvider traits and flow patterns |
| **flow-otp-master** | deepseek-v3p2 | Implement Phone OTP flow | To add OTP generation, SMS integration, rate limiting |
| **flow-email-wizard** | cogito-671b-v2-p1 | Implement Email Magic flow | To add magic link generation and email verification |
| **flow-deposit-builder** | kimi-k2-instruct | Implement First Deposit flow | To add payment processing and CUSS integration |
| **integration-specialist** | gemini-2.5-flash | Document verification flows | To add ID document and address proof flows |
| **test-engineer** | qwen3-vl-30b-a3b-thinking | Write comprehensive tests | To achieve >80% test coverage |
| **project-closer** | minimax-m2p2 | Final polish and delivery | For final lint, docs, and pre-deployment checks |
| **flow-orchestrator** | gemini-2.5-pro | Project coordination (legacy) | For architectural decisions and daily standups |

See `.opencode/AGENTS-QUICK-REFERENCE.md` for comprehensive agent documentation with examples.

### Project Phases & Agent Usage

**Phase 1: Foundation (Start Here)**
```bash
# Generate BFF OpenAPI code (if not already generated)
opencode run --agent bff-generator generate-bff

# Check architecture readiness
opencode run --agent flow-architect design-integration-traits
```

**Phase 2: Core Flow Implementation**
```bash
# Implement Phone OTP flow
opencode run --agent flow-otp-master implement-otp-flow

# Validate the implementation
opencode run --agent flow-otp-master validate-flow phone_otp

# Test the flow
opencode run --agent test-engineer test-flow phone_otp
```

**Phase 3: Advanced Flows**
```bash
# Validate deposit flow implementation
opencode run --agent flow-deposit-builder validate-flow first_deposit

# Validate document flows
opencode run --agent integration-specialist validate-flow id_document
opencode run --agent integration-specialist validate-flow address_proof
```

**Phase 4: Testing**
```bash
# Test all flows
for flow in phone_otp email_magic first_deposit id_document address_proof; do
  opencode run --agent test-engineer test-flow $flow
done
```

**Phase 5: Delivery**
```bash
# Final quality checks
opencode run --agent project-closer final-lint
opencode run --agent project-closer update-documentation
```

### Managing Multiple Agents

**Use agent-orchestrator for coordination:**

```bash
# Run all agents in optimal order based on dependencies
opencode run --agent agent-orchestrator run-all-agents

# Coordinate a specific phase
opencode run --agent agent-orchestrator coordinate-phase foundation
opencode run --agent agent-orchestrator coordinate-phase core-flows
opencode run --agent agent-orchestrator coordinate-phase advanced
opencode run --agent agent-orchestrator coordinate-phase testing
opencode run --agent agent-orchestrator coordinate-phase delivery

# Track progress across all agents
opencode run --agent agent-orchestrator track-progress

# Resolve conflicts between agents
opencode run --agent agent-orchestrator resolve-conflicts flow-otp-master flow-email-wizard
```

### Daily Project Management

**Monitor status:**
```bash
# Daily standup (should be run regularly)
opencode run --agent agent-orchestrator daily-standup

# Full workspace validation
opencode run --agent agent-orchestrator check-workspace
```

### Quick Reference Commands

| Command | Purpose | Typical Agent |
|---------|---------|---------------|
| `generate-bff` | Generate OpenAPI code | bff-generator |
| `validate-flow <name>` | Check flow implementation | Any flow-* agent |
| `test-flow <name>` | Run flow tests | test-engineer |
| `implement-otp-flow` | Generate OTP boilerplate | flow-otp-master |
| `check-workspace` | Full validation | agent-orchestrator |
| `track-progress` | View all agent status | agent-orchestrator |

### Configuration

Agents are configured in `.opencode/agents/` as Markdown files with YAML frontmatter:

```yaml
---
name: agent-name
description: Agent purpose
llm: model-name
commands:
  - cmd-1
  - cmd-2
rules:
  - Rule 1
  - Rule 2
---
```

### Customization

**Add new agent:**
```bash
# Create new agent config
echo "---\nname: my-agent\ndescription: Custom agent\nllm: claude-3-opus\ncommands: [my-cmd]\n---" > .opencode/agents/my-agent.md

# Create corresponding command
touch .opencode/commands/my-cmd
chmod +x .opencode/commands/my-cmd
```

### Troubleshooting Agent Issues

**Agent not loading:**
```bash
# Verify agent config syntax
json_pp -t null < .opencode/agents/agent-name.md 2>/dev/null || echo "Invalid YAML frontmatter"
```

**Command not found:**
```bash
# Check command permissions
ls -l .opencode/commands/
# Ensure executable: chmod +x .opencode/commands/command-name
```

**Progress tracking:**
```bash
# View detailed agent status
opencode run --agent agent-orchestrator track-progress
```

For complete agent documentation, see `.opencode/README.md`, `.opencode/QUICKSTART.md`, and `.opencode/PROJECT-EXECUTION.md`.

## Database & Migrations

### Migration Workflow
```bash
# Create new migration
cd app/crates/backend-migrate
cargo run -- create_migration <name>

# Must touch a Rust file after adding SQL migration:
touch app/crates/backend-migrate/src/migrate.rs
```

### Migration Rules
- Naming: `YYYYMMDDHHMMSS_description.sql`
- Use `TEXT` not `VARCHAR` for string columns
- Define indices and constraints in migration files
- Use Diesel DSL, avoid raw SQL where possible

## Key Directories

- `app/crates/`: Library crates (`backend-server`, `backend-core`, `backend-auth`, etc.)
- `app/bins/`: Binary crates (`backend` server, `sms-gateway`)
- `app/gen/`: Generated code (OpenAPI models) - **NEVER EDIT MANUALLY**
- `openapi/`: OpenAPI spec files (source of truth)
- `app/crates/backend-migrate/migrations/`: Database migrations
- `config/`: Configuration YAML files

## Pre-Commit Checklist

Before committing code:
1. `cargo fmt` - Format code
2. `cargo clippy --all-targets --all-features -- -D warnings` - Lint
3. `cargo check --workspace` - Compile check
4. Run relevant unit tests: `cargo test -p <crate>`
5. For API changes: `just test-it` (OAS integration tests)
6. For database changes: `cargo test -p backend-repository`
7. Never edit `app/gen/*` manually

## Testing Best Practices

### Unit Tests
- Location: `src/` module files (inline) or `tests/` directory for integration tests
- Use `mockall` for mocking traits
- Test both success and failure paths
- For repository tests: set `DATABASE_URL` or tests will skip

### Integration Tests
- Feature-gated: `--features it-tests` for OAS, `--features e2e-tests` for external deps
- OAS tests: `app/crates/backend-server/src/api/it_tests.rs`
- E2E tests: `app/crates/backend-e2e/tests/`

### Required Test Coverage
- `backend_core::Error` mapping and response behavior
- Bearer/JWT middleware bypass and enforcement cases
- KC signature verification (all failure modes + success)
- Device binding unique-conflict races
- SMS retry behavior (transient vs permanent errors)

## OpenAPI Workflow

1. Modify specs in `openapi/*.yaml` (not `app/gen/`)
2. Regenerate code: `just generate`
3. Validate: `just test-it`
4. Update handlers if API contract changed

## Configuration

- Config source: `backend-core::Config` only
- Supports env var expansion: `${VAR}` or `${VAR:-default}`
- Use `clap` for CLI args in binaries
- Shared state in `AppState` with `Arc<dyn Trait>` abstractions