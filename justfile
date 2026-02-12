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

# Variable for passing commands like `just build c="app"`
c := ""

# ----------------------------------------------------------

# Initialize the project
init:
	docker compose -p user-storage-backend -f compose.yaml build {{c}}

# Show this help
help:
	@just --summary

# Pull the image
pull:
	docker compose -p user-storage-backend -f compose.yaml pull {{c}}

# Build the project
build:
	docker compose -p user-storage-backend -f compose.yaml build {{c}}

# Start the project
up:
	docker compose -p user-storage-backend -f compose.yaml up -d --remove-orphans --build {{c}}

# Start a single service
up-single app:
	docker compose -p user-storage-backend -f compose.yaml up -d --remove-orphans --build {{app}} {{c}}

# Start the project (without rebuild)
up-no-build:
	docker compose -p user-storage-backend -f compose.yaml up -d --remove-orphans {{c}}

# Show images
img:
	docker compose -p user-storage-backend -f compose.yaml images {{c}}

# Start the project (without rebuild)
start:
	docker compose -p user-storage-backend -f compose.yaml start {{c}}

# Stop the project
down:
	docker compose -p user-storage-backend -f compose.yaml down {{c}}

# Destroy the project
destroy:
	docker compose -p user-storage-backend -f compose.yaml down -v {{c}}

# Stop containers
stop:
	docker compose -p user-storage-backend -f compose.yaml stop {{c}}

# Restart the project
restart:
	docker compose -p user-storage-backend -f compose.yaml stop {{c}}
	docker compose -p user-storage-backend -f compose.yaml up -d {{c}}

# Show logs
logs:
	docker compose -p user-storage-backend -f compose.yaml logs --tail=100 -f {{c}}

# Show API logs
logs-api:
	docker compose -p user-storage-backend -f compose.yaml logs --tail=100 -f authz-api {{c}}

# Show OPA logs
logs-opa:
	docker compose -p user-storage-backend -f compose.yaml logs --tail=100 -f authz-opa {{c}}

# Show status
ps:
	docker compose -p user-storage-backend -f compose.yaml ps {{c}}

# Show all containers
ps-all:
	docker compose -p user-storage-backend -f compose.yaml ps --all {{c}}

# Run migrations once
migrate:
	docker compose -p user-storage-backend -f compose.yaml run --rm authz-migrate

# Show stats
stats:
	docker compose -p user-storage-backend -f compose.yaml stats {{c}}
