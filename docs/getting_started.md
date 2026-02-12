# Getting Started

This repo is a Rust workspace with a `backend` binary plus a small set of Docker Compose services for local development (PostgreSQL + Keycloak + tooling).

## Prerequisites

- Rust toolchain (`cargo`)
- `just` (`just --version`)
- Docker + Docker Compose (`docker compose version`)

## Quick Start

1. Start dependencies (recommended minimal set):

   ```bash
   just up-single postgres
   just up-single keycloak-26
   ```

   You can also start everything defined in `compose.yml`:

   ```bash
   just up
   ```

2. Run the CLI help:

   ```bash
   just dev -h
   ```

   Example output:

   ```text
   UserStorage App

   Usage: backend [COMMAND]

   Commands:
     serve
     config
     migrate
     help     Print this message or the help of the given subcommand(s)

   Options:
     -h, --help     Print help
     -V, --version  Print version
   ```

3. Point the app at a config file.

   Commands accept `--config-path/-c` or the `CONFIG_PATH` environment variable. A default config is provided at `config/default.yaml`.

   ```bash
   just dev config -c config/default.yaml
   ```

4. Run database migrations:

   ```bash
   just dev migrate -c config/default.yaml
   ```

5. Start the server:

   ```bash
   just dev serve -c config/default.yaml
   ```

   Note: `serve` currently loads the config and logs startup messages; wire-up of the HTTP server is expected to evolve as development continues.

## Build

- Dev build (via `cargo run` compilation):

  ```bash
  just dev -h
  ```

- Release build:

  ```bash
  just prepare
  ```

  The release binary will be at `target/release/backend`.

## Useful Docker Commands

- See available commands and their meaning:

  ```bash
  just help
  ```

- Follow logs:

  ```bash
  just logs
  just logs-keycloak
  ```

- Stop/remove containers:

  ```bash
  just down
  ```

- Stop/remove containers and volumes:

  ```bash
  just destroy
  ```

## Troubleshooting

- **Database connection errors**: ensure Postgres is running (`just ps`) and that `database.url` in `config/default.yaml` matches your environment.
- **Keycloak/JWKS URL issues**: `compose/keycloak.compose.yml` binds Keycloak to `127.0.0.1:9026`. If you run the backend on the host, you may need `oauth2.jwks_url` to use `http://127.0.0.1:9026/...` instead of Docker-specific host gateway addresses.
