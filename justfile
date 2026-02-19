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

init: # Initialize docker compose services
	docker compose -p {{project}} -f {{compose_file}} build {{c}}

help: # Show this help message
	@printf 'Commands:\n  init          Initialize docker compose services\n  help          Show this help message\n  pull          Pull latest images from registries\n  build         Build all configured compose services\n  up            Start services with rebuild\n  up-single     Start a single service (pass service=...)\n  up-no-build   Start services without rebuilding\n  img           Show stored service images\n  start         Resume stopped services\n  down          Stop and remove containers\n  destroy       Snapshot removal of containers + volumes\n  stop          Stop running containers\n  restart       Restart services (stop + up)\n  logs          Follow all service logs\n  logs-keycloak Follow Keycloak logs\n  ps            List active containers\n  ps-all        List all containers (including exited)\n  stats         Show container stats\n  dev           Run backend (dev)\n  prepare       Build backend (release)\n'

pull: # Pull latest images from registries
	docker compose -p {{project}} -f {{compose_file}} pull {{c}}

build: # Build all configured compose services
	docker compose -p {{project}} -f {{compose_file}} build {{c}}

up: # Start services with rebuild
	docker compose -p {{project}} -f {{compose_file}} up -d --remove-orphans --build {{c}}

up-single service: # Start a single service (pass service=...)
	docker compose -p {{project}} -f {{compose_file}} up -d --remove-orphans --build {{service}} {{c}}

generate: # Start a single service (pass service=...)
	docker compose -p {{project}} -f {{compose_file}} up generate-code {{c}}
	cargo fmt -p gen_oas_server_cuss -p gen_oas_server_bff -p gen_oas_server_kc -p gen_oas_server_staff
	cargo fix --allow-dirty -p gen_oas_server_cuss -p gen_oas_server_bff -p gen_oas_server_kc -p gen_oas_server_staff

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
