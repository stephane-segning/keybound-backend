# syntax=docker/dockerfile:1.5

FROM rust:1-alpine as base

ENV CARGO_TERM_COLOR=always

WORKDIR /app

FROM base as builder-base

ARG TARGETARCH

RUN --mount=type=cache,target=/var/cache/apk \
    apk add \
    musl-dev \
    build-base \
    pkgconfig \
    perl \
    openssl-dev \
    openssl-libs-static \
    postgresql-dev \
    git \
    protobuf-dev \
    zlib-static \
    clang-dev \
    llvm-dev \
    cmake \
    curl

RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl

FROM builder-base as builder

RUN \
  # Mount workspace files and only the necessary crates
  --mount=type=bind,source=./Cargo.toml,target=/app/Cargo.toml \
  --mount=type=bind,source=./Cargo.lock,target=/app/Cargo.lock \
  --mount=type=bind,source=./app/,target=/app/app \
  --mount=type=cache,target=/app/target \
  --mount=type=cache,target=/usr/local/cargo/registry/cache \
  --mount=type=cache,target=/usr/local/cargo/registry/index \
  --mount=type=cache,target=/usr/local/cargo/git/db \
  case "$TARGETARCH" in \
    "amd64") \
      export RUST_TARGET=x86_64-unknown-linux-musl; \
      ;; \
    "arm64") \
      export RUST_TARGET=aarch64-unknown-linux-musl; \
      ;; \
    *) \
      echo "Unsupported TARGETARCH: $TARGETARCH"; \
      exit 1; \
      ;; \
  esac; \
  cargo build --locked --target "${RUST_TARGET}" -p backend -p sms-gateway \
  && cp ./target/"${RUST_TARGET}"/debug/backend backend \
  && cp ./target/"${RUST_TARGET}"/debug/sms-gateway sms-gateway \
  && cp ./target/"${RUST_TARGET}"/debug/health healthcheck

FROM builder-base as builder-prod

RUN \
  # Mount workspace files and only the necessary crates
  --mount=type=bind,source=./Cargo.toml,target=/app/Cargo.toml \
  --mount=type=bind,source=./Cargo.lock,target=/app/Cargo.lock \
  --mount=type=bind,source=./app/,target=/app/app \
  --mount=type=cache,target=/app/target \
  --mount=type=cache,target=/usr/local/cargo/registry/cache \
  --mount=type=cache,target=/usr/local/cargo/registry/index \
  --mount=type=cache,target=/usr/local/cargo/git/db \
  case "$TARGETARCH" in \
    "amd64") \
      export RUST_TARGET=x86_64-unknown-linux-musl; \
      ;; \
    "arm64") \
      export RUST_TARGET=aarch64-unknown-linux-musl; \
      ;; \
    *) \
      echo "Unsupported TARGETARCH: $TARGETARCH"; \
      exit 1; \
      ;; \
  esac; \
  cargo build --release --locked --target "${RUST_TARGET}" -p backend -p sms-gateway \
  && cp ./target/"${RUST_TARGET}"/release/backend backend \
  && cp ./target/"${RUST_TARGET}"/release/sms-gateway sms-gateway \
  && cp ./target/"${RUST_TARGET}"/release/health healthcheck

FROM gcr.io/distroless/static-debian12:nonroot as dev

ENV RUST_LOG=debug
ENV PORT=3000

WORKDIR /app

COPY --from=builder /app/backend /app/server
COPY --from=builder /app/sms-gateway /app/sms-gateway
COPY --from=builder /app/healthcheck /app/healthcheck

COPY flows/ /app/flows/
COPY sessions/ /app/sessions/

VOLUME ["/app/flows", "/app/sessions"]

USER nonroot:nonroot

EXPOSE $PORT

HEALTHCHECK --interval=10s --timeout=3s --start-period=2s --retries=5 CMD ["/app/healthcheck"]

ENTRYPOINT ["/app/server"]

FROM gcr.io/distroless/static-debian12:nonroot

LABEL maintainer="Stephane Segning Lambou <stephane.segning-lambou@adorsys>"
LABEL maintainer="GIS Cameroon <udm-gis@adorsys>"
LABEL org.opencontainers.image.description="adorsys GIS Cameroon"

ENV RUST_LOG=warn
ENV PORT=3000

WORKDIR /app

COPY --from=builder-prod /app/backend /app/server
COPY --from=builder-prod /app/sms-gateway /app/sms-gateway
COPY --from=builder-prod /app/healthcheck /app/healthcheck

COPY flows/ /app/flows/
COPY sessions/ /app/sessions/

VOLUME ["/app/flows", "/app/sessions"]

USER nonroot:nonroot

EXPOSE $PORT

HEALTHCHECK --interval=10s --timeout=3s --start-period=2s --retries=5 CMD ["/app/healthcheck"]

ENTRYPOINT ["/app/server"]
