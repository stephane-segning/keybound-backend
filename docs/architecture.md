# Architecture

This workspace runs a native `axum` backend for KC, BFF, and Staff APIs.  
Generated OA3 crates are used for DTO/model contracts, not runtime request dispatch.

## Structure

```mermaid
flowchart TB
  root["repo/"]
  root --> app["app/backend (binary)"]
  root --> crates["crates/*"]
  root --> openapi["openapi/*.json"]
  root --> migrations["migrations/*.sql"]
  root --> docs["docs/*.md"]
  root --> compose["compose*.yml + compose/*.yml"]

  crates --> core["backend-core"]
  crates --> auth["backend-auth"]
  crates --> server["backend-server"]
  crates --> repo["backend-repository"]
  crates --> model["backend-model"]
  crates --> idc["backend-id"]
  crates --> migrate["backend-migrate"]
  crates --> gen["gen_oas_* (generated)"]
```

## Runtime Flow (Controller → Service → Repository)

```mermaid
sequenceDiagram
  autonumber
  participant C as Client
  participant R as axum Router
  participant Ctrl as Controllers (backend-server/api.rs)
  participant Svc as Services (backend-server/services.rs)
  participant Rep as PgRepository (inherent methods)
  participant SQL as PgSqlRepo #[repo]/#[dml]
  participant DB as Postgres

  C->>R: HTTP request
  R->>Ctrl: Matched handler
  Ctrl->>Svc: Orchestrate use-case
  Svc->>Rep: Call repository method
  Rep->>SQL: Execute SQLx-Data DML
  SQL->>DB: SQL query
  DB-->>SQL: Rows
  SQL-->>Rep: FromRow structs
  Rep-->>Svc: Domain/db structs
  Svc-->>Ctrl: Response model
  Ctrl-->>C: HTTP status + JSON
```

## Crate Relationship Graph

```mermaid
flowchart TD
  App["app/backend"]
  Core["backend-core"]
  Auth["backend-auth"]
  Server["backend-server"]
  Repo["backend-repository"]
  Model["backend-model"]
  Id["backend-id"]
  Migrate["backend-migrate"]
  Otlp["backend-otlp"]
  GenKC["gen_oas_server_kc"]
  GenBFF["gen_oas_server_bff"]
  GenStaff["gen_oas_server_staff"]

  App --> Core
  App --> Server
  App --> Migrate
  App --> Otlp

  Server --> Core
  Server --> Auth
  Server --> Repo
  Server --> Model
  Server --> Id
  Server --> GenKC
  Server --> GenBFF
  Server --> GenStaff

  Repo --> Core
  Repo --> Id
  Repo --> Model

  Model --> GenKC
  Model --> GenBFF
  Model --> GenStaff
  Model --> Core

  Auth --> Id
```

## Libraries and Usage

```mermaid
flowchart LR
  subgraph HTTP
    axum["axum / axum-server"]
  end

  subgraph Data
    sqlx["sqlx (pool + FromRow)"]
    sqlxdata["sqlx-data #[repo]/#[dml]"]
    postgres["Postgres"]
  end

  subgraph Mapping
    o2o["o2o (DTO mapping)"]
  end

  subgraph Cache
    lru["lru (in-process)"]
    redis["redis:latest (compose)"]
  end

  subgraph AWS
    s3["aws-sdk-s3 (presign PUT)"]
    sns["aws-sdk-sns (publish/retry)"]
    awscfg["aws-config"]
  end

  subgraph Errors
    coreerr["backend-core::Error"]
  end

  servercrate["backend-server"] --> axum
  servercrate --> lru
  servercrate --> s3
  servercrate --> sns
  servercrate --> awscfg
  servercrate --> coreerr

  repocrate["backend-repository"] --> sqlxdata
  repocrate --> sqlx
  sqlxdata --> postgres

  modelcrate["backend-model"] --> o2o
  modelcrate --> sqlx
```

## OpenAPI to Runtime Path

```mermaid
flowchart LR
  spec["openapi/*.json"] --> gen["generate-code"]
  gen --> gencrates["crates/gen_oas_*"]
  gencrates --> model["backend-model (o2o DTO mapping)"]
  gencrates --> server["backend-server handlers (request/response types)"]
```
