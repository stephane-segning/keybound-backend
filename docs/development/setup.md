# Development Guide 💻

## Why?
We want you to have the best development experience possible! 🌟 Setting up your environment should be quick, easy, and even a little bit fun! 🥳✨

## Actual
Setting up for local development is straightforward! We use `just` as our command runner to make things super simple. 🛠️

### Prerequisites
- **Rust**: Latest stable version. 🦀
- **Docker & Compose**: For running PostgreSQL, Redis, and MinIO. 🐳
- **Just**: Our favorite command runner! ✨

### Getting Started
1. **Clone the repo** and head into the directory.
2. **Setup your config**: Copy `config/dev.yaml` and tweak your environment variables (especially `DATABASE_URL` and `KEYCLOAK_ISSUER`). 🛠️
3. **Spin up dependencies**:
   ```bash
   just up c="postgres redis minio keycloak-26"
   ```
4. **Generate OpenAPI code (if needed)**:
   ```bash
   just generate
   ```
5. **Run migrations (optional)**:
   - Migrations are also run on server startup, but this command is useful for CI/debugging.
   ```bash
   just dev migrate --config-path config/dev.yaml
   ```
6. **Start the server/worker**:
   ```bash
   just dev serve --config-path config/dev.yaml --mode shared
   ```

## Constraints
- **Stable Rust only**: We avoid nightly features to keep things predictable and solid! 🧱
- **Formatting**: Always run `cargo fmt` before committing. ✨
- **Clippy**: Listen to what Clippy says—she's usually right! 🦀

## Findings
We've found that using `justfile` really streamlines our workflow! 🚀 No more remembering long `cargo` commands or Docker flags! It's all right there in the `justfile`. 🛠️

## How to?
### Running Tests
- **All tests**: `cargo test --workspace`
- **OAS Integration tests**: `just test-it`
- **Smoke E2E tests**: `just test-e2e-smoke`
- **Full E2E tests**: `just test-e2e-full`

### Adding a Migration
1. Create a new `.sql` file in `app/crates/backend-migrate/migrations`.
2. Name it `YYYYMMDDHHMMSS_description.sql`.
3. **Important**: `touch app/crates/backend-migrate/src/migrate.rs` to force a recompile and include your new migration! 🛠️✨

## Conclusion
We hope this guide gets you up and running in no time! 🚀 If you hit any snags, remember: we're all in this together! Happy coding! 🥳✨
