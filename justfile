# ========================================================================================================
#
#    .d888888                                                    d888888P          dP                         oo                     dP   oo                       888888ba                    dP                               dP
#   d8'    88                                                       88             88                                                88                            88    `8b                   88                               88
#   88aaaaa88a d888888b .d8888b. 88d8b.d8b. 88d888b. .d8888b.       88    .d8888b. 88  .dP  .d8888b. 88d888b. dP d888888b .d8888b. d8888P dP .d8888b. 88d888b.    a88aaaa8P' .d8888b. .d8888b. 88  .dP  .d8888b. 88d888b. .d888b88
#   88     88     .d8P' 88'  `88 88'`88'`88 88'  `88 88'  `88       88    88'  `88 88888"   88ooood8 88'  `88 88    .d8P' 88'  `88   88   88 88'  `88 88'  `88     88   `8b. 88'  `88 88'  `"" 88888"   88ooood8 88'  `88 88'  `88
#   88     88   .Y8P    88.  .88 88  88  88 88       88.  .88       88    88.  .88 88  `8b. 88.  ... 88    88 88  .Y8P    88.  .88   88   88 88.  .88 88    88     88    .88 88.  .88 88.  ... 88  `8b. 88.  ... 88    88 88.  .88
#   88     88  d888888P `88888P8 dP  dP  dP dP       `88888P8       dP    `88888P' dP   `YP `88888P' dP    dP dP d888888P `88888P8   dP   dP `88888P' dP    dP     88888888P `88888P8 `88888P' dP   `YP `88888P' dP    dP `88888P8
#
#
#    ====================> Core Backend
#
#    Makefile for the project
#    Author: @stephane-segning
#
# ========================================================================================================

c := ""
compose_file := "compose.yml"
project := "user-storage-backend"
compose_e2e := ".docker/e2e/compose.e2e.yaml"
project_e2e := "user-storage-backend-e2e"

export USER_ID := `id -u`
export GROUP_ID := `id -g`

init: # Initialize docker compose services
	docker compose -p {{project}} -f {{compose_file}} build {{c}}

help: # Show this help message
	@printf 'Commands:\n  init            Initialize docker compose services\n  help            Show this help message\n  pull            Pull latest images from registries\n  build           Build all configured compose services\n  up              Start services with rebuild\n  up-single       Start a single service (pass service=...)\n  up-no-build     Start services without rebuilding\n  img             Show stored service images\n  start           Resume stopped services\n  down            Stop and remove containers\n  destroy         Snapshot removal of containers + volumes\n  stop            Stop running containers\n  restart         Restart services (stop + up)\n  logs            Follow all service logs\n  logs-keycloak   Follow Keycloak logs\n  ps              List active containers\n  ps-all          List all containers (including exited)\n  stats           Show container stats\n  dev             Run backend (dev)\n  prepare         Build backend (release)\n  test-it         Run OAS integration tests\n  test-e2e-rust   Run Rust-native crate-level e2e tests (wiremock/testcontainers)\n  test-e2e-smoke  Run Compose smoke e2e suite with Rust runner\n  test-e2e-full   Run Compose full e2e suite with Rust runner\n'

pull: # Pull latest images from registries
	docker compose -p {{project}} -f {{compose_file}} pull {{c}}

build: # Build all configured compose services
	docker compose -p {{project}} -f {{compose_file}} build {{c}}

up: # Start services with rebuild
	docker compose -p {{project}} -f {{compose_file}} up -d --remove-orphans --build {{c}}

up-single service: # Start a single service (pass service=...)
	docker compose -p {{project}} -f {{compose_file}} up -d --remove-orphans --build {{service}} {{c}}

generate: # Generate code from OpenAPI specs
	docker compose -p {{project}} -f {{compose_file}} run --rm generate-code

up-no-build: # Start services without rebuilding
	docker compose -p {{project}} -f {{compose_file}} up -d --remove-orphans {{c}}

img: # Show stored service images
	docker compose -p {{project}} -f {{compose_file}} images {{c}}

start: # Resume stopped services
	docker compose -p {{project}} -f {{compose_file}} start {{c}}

down: # Stop and remove containers
	docker compose -p {{project}} -f {{compose_file}} down {{c}}

destroy: # Snapshot removal of containers + volumes
	docker compose -p {{project}} -f {{compose_file}} down -v {{c}}

stop: # Stop running containers
	docker compose -p {{project}} -f {{compose_file}} stop {{c}}

restart: # Restart services (stop + up)
	docker compose -p {{project}} -f {{compose_file}} stop {{c}}
	docker compose -p {{project}} -f {{compose_file}} up -d {{c}}

logs: # Follow all service logs
	docker compose -p {{project}} -f {{compose_file}} logs --tail=100 -f {{c}}

logs-keycloak: # Follow Keycloak logs
	docker compose -p {{project}} -f {{compose_file}} logs --tail=100 -f keycloak-26 {{c}}

ps: # List active containers
	docker compose -p {{project}} -f {{compose_file}} ps {{c}}

ps-all: # List all containers (including exited)
	docker compose -p {{project}} -f {{compose_file}} ps --all {{c}}

stats: # Show container stats
	docker compose -p {{project}} -f {{compose_file}} stats {{c}}

dev *args: # Run the backend binary in dev profile (pass args to the CLI)
	RUST_LOG=debug cargo run --color=always --bin backend --profile dev -- {{args}}

prepare: # Build the backend binary in release mode
	cargo build --release

test-it: # Run OAS integration tests (feature-gated)
	cargo test -p backend-server --features it-tests api::it_tests::

test-e2e-rust:
	cargo test -p backend-auth --features e2e-tests --test oidc_wiremock_e2e
	cargo test -p backend-repository --features e2e-tests --test state_machine_repo_testcontainers

e2e-build:
	docker compose -p {{project_e2e}} -f {{compose_e2e}} build

test-e2e-smoke:
	/bin/sh -ec 'set -e; \
	  cleanup() { status=$?; \
	    if [ $status -ne 0 ]; then \
	      mkdir -p .docker/e2e/artifacts; \
	      docker compose -p {{project_e2e}} -f {{compose_e2e}} logs --no-color > .docker/e2e/artifacts/e2e-smoke-failure.log || true; \
	    fi; \
	    docker compose -p {{project_e2e}} -f {{compose_e2e}} down -v || true; \
	    exit $status; \
	  }; \
	  trap cleanup EXIT; \
	  export POSTGRES_PORT=15432 REDIS_PORT=16379 MINIO_API_PORT=19000 MINIO_CONSOLE_PORT=19001; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} down -v || true; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} build user-storage-server user-storage-worker sms-gateway cuss-stub sms-sink; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} up -d \
	    postgres redis minio minio-create-bucket keycloak cuss-stub sms-sink sms-gateway user-storage-server; \
	  for i in $(seq 1 90); do \
	    if curl -fsS http://127.0.0.1:3002/health >/dev/null 2>&1; then break; fi; \
	    sleep 2; \
	    if [ $i -eq 90 ]; then echo \"user-storage-server did not become healthy\"; exit 1; fi; \
	  done; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} up -d user-storage-worker; \
	  USER_STORAGE_URL=http://127.0.0.1:3002 \
	  KEYCLOAK_URL=http://127.0.0.1:9026 \
	  CUSS_URL=http://127.0.0.1:18080 \
	  SMS_SINK_URL=http://127.0.0.1:18081 \
	  DATABASE_URL=postgres://postgres:postgres@127.0.0.1:15432/user-storage \
	  KEYCLOAK_CLIENT_ID=test-client \
	  KEYCLOAK_CLIENT_SECRET=some-secret \
	  cargo test -p backend-e2e --features e2e-tests --test smoke -- --nocapture'

test-e2e-full:
	/bin/sh -ec 'set -e; \
	  cleanup() { status=$?; \
	    if [ $status -ne 0 ]; then \
	      mkdir -p .docker/e2e/artifacts; \
	      docker compose -p {{project_e2e}} -f {{compose_e2e}} logs --no-color > .docker/e2e/artifacts/e2e-full-failure.log || true; \
	    fi; \
	    docker compose -p {{project_e2e}} -f {{compose_e2e}} down -v || true; \
	    exit $status; \
	  }; \
	  trap cleanup EXIT; \
	  export POSTGRES_PORT=15432 REDIS_PORT=16379 MINIO_API_PORT=19000 MINIO_CONSOLE_PORT=19001; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} down -v || true; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} build user-storage-server user-storage-worker sms-gateway cuss-stub sms-sink; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} up -d \
	    postgres redis minio minio-create-bucket keycloak cuss-stub sms-sink sms-gateway user-storage-server; \
	  for i in $(seq 1 90); do \
	    if curl -fsS http://127.0.0.1:3002/health >/dev/null 2>&1; then break; fi; \
	    sleep 2; \
	    if [ $i -eq 90 ]; then echo \"user-storage-server did not become healthy\"; exit 1; fi; \
	  done; \
	  docker compose -p {{project_e2e}} -f {{compose_e2e}} up -d \
	    user-storage-server-blank-base user-storage-server-auth-disabled user-storage-worker user-storage-worker-secondary; \
	  USER_STORAGE_URL=http://127.0.0.1:3002 \
	  USER_STORAGE_BLANK_BASE_URL=http://127.0.0.1:3003 \
	  USER_STORAGE_AUTH_DISABLED_URL=http://127.0.0.1:3004 \
	  WORKER_PRIMARY_URL=http://127.0.0.1:3005 \
	  WORKER_SECONDARY_URL=http://127.0.0.1:3006 \
	  KEYCLOAK_URL=http://127.0.0.1:9026 \
	  CUSS_URL=http://127.0.0.1:18080 \
	  SMS_SINK_URL=http://127.0.0.1:18081 \
	  DATABASE_URL=postgres://postgres:postgres@127.0.0.1:15432/user-storage \
	  KEYCLOAK_CLIENT_ID=test-client \
	  KEYCLOAK_CLIENT_SECRET=some-secret \
	  cargo test -p backend-e2e --features e2e-tests --test full -- --nocapture'

e2e-smoke:
	just test-e2e-smoke

e2e-full:
	just test-e2e-full

all-checks:
	@echo "Running Rust formatting, lint, and checks"
	cargo fmt
	#cargo deny check
	cargo fix --allow-dirty
	cargo clippy --all-targets --all-features --fix --allow-dirty -- -D warnings
	cargo check --all-targets --all-features
