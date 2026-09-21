# CLAUDE.md

This file provides guidance to Claude Code
(claude.ai/code) when working with code in this
repository.

## Requirements

- Rust stable 1.96+
- Docker or Podman (for container builds)
- Access to a Kubernetes cluster (for runtime)

## Quick Reference

```console
make build          # compile
make test           # all tests
make fmt            # format code
make lint           # clippy + fmt check
make doc            # rustdoc with -D warnings
make audit          # cargo audit
make container      # build container image
```

Run a single test:

```console
cargo test -- test_name
```

## Architecture

RBAC-Gate is a single-binary Pingora-based proxy that
enforces Kubernetes RBAC on incoming gRPC and HTTP
requests.

```text
src/
├── main.rs          # CLI, server bootstrap
├── config/          # YAML config parsing (defs, grpc, http)
├── grpc/            # gRPC proxy + policy enforcement
├── http/            # HTTP proxy + policy enforcement
├── kube/            # TokenReview + SubjectAccessReview clients
├── logging/         # tracing/logging setup
└── utils/           # shared proxy utilities
```

**Request flow:**

1. Client sends request with `Bearer` token
2. `kube::auth` validates token via TokenReview
3. Policy module matches request against configured rules
4. Variables extracted from headers/path/query
5. SubjectAccessReview issued with resolved resource attributes
6. Request proxied upstream or rejected (401/403)

## Key Patterns

- **Pingora proxy**: both gRPC and HTTP proxies implement
  Pingora's `ProxyHttp` trait
- **Caching**: TokenReview and SubjectAccessReview results
  are cached using `moka` with configurable TTL
- **Variable interpolation**: `{variable-name}` syntax in
  config for extracting and injecting request values into
  SAR resource attributes
- **Config overlay**: `--secret-config` merges on top of
  the main config for secrets management
