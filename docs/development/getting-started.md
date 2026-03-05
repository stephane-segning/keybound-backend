# Getting started for everyone

This guide explains the ways you can run the Keybound backend—from a single Docker container to the full Compose stack and a Kubernetes deployment that relies on the BjW v4 helper libraries. The examples assume you are in the repository root and have already reviewed `AGENTS.md` for architectural and testing constraints.

## Prerequisites
- **Rust toolchain** (stable channel) and `cargo` for building or running the server locally.
- **Docker + docker compose** to build and run containers (the `just` scripts use the system Docker daemon).
- **Just** for coordinating the `compose.yml` workflow and Rust smoke/e2e tests.
- **Helm 3.X** and **kubectl** for the Kubernetes path.
- **Postgres/Redis/MinIO/Keycloak credentials** that match the environment you are targeting (dev, staging, production).

> Most of our runtime configuration comes from `config/*.yaml`. The development template, `config/dev.yaml`, is a good reference: copy it, inject the required secrets (`DATABASE_URL`, `KEYCLOAK_ISSUER`, `MINIO_*`, `REDIS_URL`, etc.), and then point the server at the new file with `--config-path`.

## 1. Run a single Docker container
1. **Build the statically linked release binary for your target architecture** (the Dockerfile supports `TARGETARCH`):
   ```bash
   TARGETARCH=amd64 docker build \
     --file deploy/docker/user-storage/Dockerfile \
     --build-arg TARGETARCH=amd64 \
     --tag user-storage-app:latest .
   ```
2. **Prepare the config directory** so that the container can read it. A simple pattern is to keep your custom config in `config/` and mount the directory as a volume.
3. **Run the server** (similar flags work for `worker` mode too):
   ```bash
   docker run --rm -p 3000:3000 \
     -v "$PWD/config":/app/config:ro \
     -e DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/user-storage \
     -e REDIS_URL=redis://127.0.0.1:6379 \
     -e MINIO_ENDPOINT=http://127.0.0.1:9000 \
     -e KEYCLOAK_ISSUER=http://localhost:9026/realms/e2e-testing \
     user-storage-app:latest \
     serve --config-path config/dev.yaml --mode server
   ```
   The container uses the warm config file for things like `kc.signature_secret`, the base paths for `/kc`, `/bff` and `/staff`, and the SNS/SMS provider configuration. Override the env vars above to match your infrastructure.
4. **Worker mode** uses the same binary but needs `--mode worker`. You can run it alongside the server if you mount the same config.

## 2. Run the Compose stack (`compose.yml`)
The root `compose.yml` glues together `tools`, `keycloak`, `postgres`, `redis`, `minio`, `app`, and helper services. We expose convenient `just` aliases in `justfile` for common operations.

1. **Build the Compose images (if you changed Rust code or dependencies)**:
   ```bash
   just build
   ```
2. **Bring up the full stack**:
   ```bash
   just up
   ```
   This starts the database, Keycloak, Redis, MinIO (with bucket creation), the server, and the worker. The backend services inherit the `x-env` block defined in `deploy/compose/app.compose.yml`, so you can override values such as `MINIO_BUCKET`, `MINIO_ACCESS_KEY`, or `RUST_LOG` with shell variables before invoking `just up`.
3. **Tail logs, run smoke/e2e suites, and stop the stack**:
   - `just logs` / `just logs-keycloak` to follow logs.
   - `just ps` / `just down` to inspect or tear down.
   - `just test-it`, `just test-e2e-smoke`, `just test-e2e-full` run the OAS integration and Compose Rust e2e suites.

> Tip: The Compose stack exposes the `app` service on port `3001` by default, but the backend still listens on `3000` inside the container. Use `http://localhost:3001` when hitting the BFF/KC/Staff endpoints.

## 3. Deploy to Kubernetes with BjW v4 charts
We ship a Helm chart (`deploy/charts/user-storage`) that depends on BjW’s `common` chart (locked to version `4.6.2` in `Chart.lock`). The chart uses the `bjw-s.common.loader` helpers to wire secrets, init containers, and RBAC for you.

1. **Register the BjW Helm repo and update**:
   ```bash
   helm repo add bjw-s-labs https://bjw-s-labs.github.io/helm-charts
   helm repo update
   ```
2. **Create the namespace and secrets** (example for namespace `user-storage`):
   ```bash
   kubectl create namespace user-storage
   kubectl -n user-storage create secret generic postgres-secret \
     --from-literal=uri=postgres://postgres:postgres@postgres:5432/user-storage \
     --from-literal=url=postgres://postgres:postgres@postgres:5432/user-storage
   kubectl -n user-storage create secret generic mcpo-api-key-env \
     --from-literal=DATABASE_URL=postgres://postgres:postgres@postgres:5432/user-storage \
     --from-literal=REDIS_URL=redis://redis:6379 \
     --from-literal=MINIO_ENDPOINT=http://minio:9000 \
     --from-literal=MINIO_ACCESS_KEY=minioadmin \
     --from-literal=MINIO_SECRET_KEY=minioadmin \
     --from-literal=KEYCLOAK_ISSUER=https://keycloak.example.com/realms/e2e-testing
   ```
   Add any other env vars your config needs (e.g., `KC_SIGNATURE_SECRET`, `SNS_REGION`, `SMS_PROVIDER`). Secrets prefixed with `mcpo-` flow directly into the container through the `envFrom` section in `values.yaml`.
3. **Customize `values.yaml`** so it points at your built image and exposes the ports/args you need. For example, to pin the image and force server mode:
   ```bash
   helm upgrade --install user-storage ./deploy/charts/user-storage \
     --namespace user-storage \
     --set controllers.user-storage.containers.user-storage.image.repository=ghcr.io/your-org/user-storage \
     --set controllers.user-storage.containers.user-storage.image.tag=v1.2.3 \
     --set controllers.user-storage.containers.user-storage.args[0]=serve \
     --set controllers.user-storage.containers.user-storage.args[1]=--config-path \
     --set controllers.user-storage.containers.user-storage.args[2]=config/dev.yaml \
     --set controllers.user-storage.containers.user-storage.args[3]=--mode \
     --set controllers.user-storage.containers.user-storage.args[4]=server
   ```
4. **Watch the deployment**:
   ```bash
   kubectl -n user-storage get pods
   kubectl -n user-storage logs deploy/user-storage
   ```
   The BjW chart layers things like init containers (it runs the migration image from `controllers.user-storage.initContainers.install-extensions`) and the common loader ensures your service account, RBAC, and health checks are wired correctly.

> The BjW common chart expects `global.nameOverride`, so if you set release name `user-storage` the loader rewrites it for you. Keep the dependency locked to version `4.x` so the helper macros in `init.yaml` continue to work.

## Additional tips
- Keep your configuration close to the code: copy `config/dev.yaml` for local fun, but in production build a Helm `ConfigMap/Secret` that matches the values exposed through `mcpo-api-key-env`.
- When you add or modify migrations: create a new file in `app/crates/backend-migrate/migrations` and touch `app/crates/backend-migrate/src/migrate.rs` so the embedded migrations recompile.
- Run `cargo test -p backend-server --features it-tests api::it_tests::` to verify the OpenAPI integration scenarios, and `cargo test -p backend-e2e --features e2e-tests --test full -- --nocapture` when upgrading Compose dependencies.

## Stay in sync with AGENTS.md
`AGENTS.md` reflects the live architecture, IDs, and hard rules that every team member must follow. Whenever you add new get‑started instructions or update the Helm values, update `AGENTS.md` too so the whole team knows what changed.
