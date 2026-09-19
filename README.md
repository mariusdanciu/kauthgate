# kauthgate

A gRPC authentication and authorization gateway built on [Pingora](https://github.com/cloudflare/pingora). It sits in front of a gRPC backend and enforces Kubernetes-native auth (TokenReview + SubjectAccessReview) on every request.

## How it works

1. A gRPC client sends a request with a `Bearer` token in the `authorization` header.
2. **Authentication** — the gateway validates the token via the Kubernetes TokenReview API.
3. **Policy matching** — the request path is parsed into gRPC service/action, and matched against configured auth policies.
4. **Variable extraction** — headers (e.g., `x-tenant-id`) are extracted into variables that can be interpolated into resource attributes.
5. **Authorization** — a Kubernetes SubjectAccessReview is issued with the resolved resource attributes (namespace, apiGroup, resource, verb).
6. If both checks pass, the request is proxied to the upstream gRPC backend over HTTP/2. Otherwise, a gRPC error (status 16 `UNAUTHENTICATED`) is returned via HTTP/2 trailers.

## Configuration

The gateway is configured via a YAML file:

```yaml
upstream:
  host: 127.0.0.1
  port: 50051

auth:
  cache-ttl-secs: 300
  token-review-audiences: []

grpc:
  extractors:
    - name: tenant-id
      header: x-tenant-id

  auth-policies:
    - name: flight
      conditions:
        service: arrow.flight.protocol.FlightService
        allowed-actions:
          - DoAction
          - DoGet
          - GetFlightInfo
        required-headers:
          - x-tenant-id
      resource-attributes:
        namespace: "{tenant-id}"
        api-group: dataconnecthub.opendatahub.io
        resource: data-connections
        verb: "get"
```

### Extractors

Extractors pull values from the request into named variables. These variables can be referenced in resource attributes using `{variable-name}` syntax.

| Type | Field | Description |
|------|-------|-------------|
| Header | `header` | Extracts the value of an HTTP header |

### Auth policies

Each policy defines:

- **conditions** — which gRPC service, actions, and required headers must be present for the policy to apply
- **resource-attributes** — the Kubernetes RBAC resource to check via SubjectAccessReview. Supports `{variable}` interpolation from extractors.

## Usage

```bash
kauthgate --config config/config.yaml
```

### CLI options

| Flag | Default | Description |
|------|---------|-------------|
| `-c, --config` | `config/config.yaml` | Path to the config file |
| `--secret-config` | `/secrets/secret-config.yaml` | Optional secret config overlay (merged on top) |
| `-j, --json-logs` | `false` | Enable JSON-formatted log output |

## Building

```bash
cargo build --release
```

## Kubernetes RBAC setup

The gateway's ServiceAccount needs permissions to create TokenReview and SubjectAccessReview resources:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: ClusterRole
metadata:
  name: kauthgate
rules:
  - apiGroups: ["authentication.k8s.io"]
    resources: ["tokenreviews"]
    verbs: ["create"]
  - apiGroups: ["authorization.k8s.io"]
    resources: ["subjectaccessreviews"]
    verbs: ["create"]
```

Users/ServiceAccounts that should be authorized need a Role granting access to the configured resource:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: dch-ingest
  namespace: my-namespace
rules:
  - apiGroups: ["dataconnecthub.opendatahub.io"]
    resources: ["data-connections"]
    verbs: ["get"]
```
