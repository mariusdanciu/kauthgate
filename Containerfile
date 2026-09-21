# syntax=docker/dockerfile:1

# ------------------------------------------------------------------------------
# Stage 1: Build
# ------------------------------------------------------------------------------

FROM rust:1.96-alpine AS builder

RUN apk add --no-cache musl-dev pkgconf

WORKDIR /src

# ------------------------------------------------------------------------------
# Cache: copy only manifests + stub so dependencies compile first
# ------------------------------------------------------------------------------

COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src \
    && printf '//! stub\nfn main() {}\n' > src/main.rs

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release

# ------------------------------------------------------------------------------
# Real source
# ------------------------------------------------------------------------------

COPY src ./src

RUN find src -name '*.rs' -exec touch {} +

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release \
    && cp target/release/rbac-gate /usr/local/bin/rbac-gate

# ------------------------------------------------------------------------------
# Stage 2: Runtime
# ------------------------------------------------------------------------------

FROM alpine:3.23

LABEL org.opencontainers.image.description="RBAC-Gate — Kubernetes RBAC authorization gateway"

RUN apk add --no-cache ca-certificates \
    && addgroup -S rbac-gate \
    && adduser -S -G rbac-gate -h /nonexistent -s /sbin/nologin rbac-gate

COPY --from=builder --chown=root:root --chmod=0555 \
    /usr/local/bin/rbac-gate /usr/local/bin/rbac-gate

USER rbac-gate:rbac-gate

EXPOSE 6188 8080

ENTRYPOINT ["rbac-gate"]
