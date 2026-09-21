# kauthgate

An authentication and authorization gateway built on [Pingora](https://github.com/cloudflare/pingora). It sits in front of gRPC and HTTP backends and enforces Kubernetes-native auth (TokenReview + SubjectAccessReview) on every request.

## How it works

1. A client sends a request with a `Bearer` token in the `authorization` header.
2. **Authentication** — the gateway validates the token via the Kubernetes TokenReview API.
3. **Policy matching** — the request is matched against configured mappings based on service/method (gRPC) or path/method/headers/query-params (HTTP).
4. **Variable extraction** — values from headers, path segments, and query strings are extracted into named variables using `{variable-name}` syntax.
5. **Authorization** — a Kubernetes SubjectAccessReview is issued with the resolved resource attributes (namespace, apiGroup, resource, verb).
6. If both checks pass, the request is proxied to the upstream backend. Otherwise, an error is returned (gRPC status 16 via trailers, or HTTP 401/403).

## Configuration

The gateway is configured via a YAML file. It supports both gRPC and HTTP proxies, each with their own upstream and mapping rules.

```yaml
auth:
  cache-ttl-secs: 300
  token-review-audiences: []

grpc:
  upstream:
    host: 127.0.0.1
    port: 50051
  mappings:
    - name: flight
      request:
        service: arrow.flight.protocol.FlightService
        grpc-methods:
          - DoAction
          - DoGet
          - GetFlightInfo
        headers:
          - name: "x-tenant-id"
            value: "{tenant-id}"
      sar-resource-attributes:
        namespace: "{tenant-id}"
        api-group: dataconnecthub.opendatahub.io
        resource: data-store
        verb: "get"

http:
  upstream:
    host: 127.0.0.1
    port: 8081
  mappings:
    - name: rest
      request:
        path: /api/v1alpha1/data/connections
        methods:
          - post
        headers:
          - name: "x-tenant-id"
            value: "{tenant-id}"
      sar-resource-attributes:
        namespace: "{tenant-id}"
        api-group: dataconnecthub.opendatahub.io
        resource: data-connections
        verb: "create"
```

### Variable extraction

Variables are extracted from request data and can be interpolated into SAR resource attributes using `{variable-name}` syntax. A header or query-param entry with a variable value (e.g. `"{tenant-id}"`) both requires the field to be present and captures its value into the named variable. A literal value (e.g. `"v2"`) requires an exact match without capturing.

**gRPC extractors** — variables can be extracted from:
- **Headers** — `name` specifies the header, `value: "{var}"` captures its value
- **Service / method** — the gRPC service and method are automatically injected as `service` and `grpc_method`

**HTTP extractors** — variables can be extracted from:
- **Path segments** — use `{variable}` in the path pattern (e.g. `/api/{version}/tenants/{tenant-id}`)
- **Headers** — same as gRPC: `name` specifies the header, `value: "{var}"` captures its value
- **Query parameters** — `name` specifies the parameter, `value: "{var}"` captures its value
- **Path / method** — the full request path and HTTP method are automatically injected as `path` and `method`

### Request matching

All request match fields are optional. When omitted, the field is not checked (any value matches).

**gRPC request fields:**

| Field | Description |
|-------|-------------|
| `service` | gRPC service name (e.g. `arrow.flight.protocol.FlightService`) |
| `grpc-methods` | List of allowed gRPC methods (e.g. `DoGet`, `DoAction`) |
| `headers` | Required headers with variable or literal values |

**HTTP request fields:**

| Field | Description |
|-------|-------------|
| `path` | URL path pattern, supports `{var}` extraction, `*` (single segment wildcard), `**` (globstar) |
| `methods` | List of allowed HTTP methods (lowercased during parsing) |
| `headers` | Required headers with variable or literal values |
| `query-params` | Required query parameters with variable or literal values |

### SAR resource attributes

Each mapping specifies `sar-resource-attributes` for the SubjectAccessReview. Fields support `{variable}` interpolation from extracted values:

```yaml
sar-resource-attributes:
  namespace: "{tenant-id}"    # resolved from extracted variable
  api-group: example.io       # literal value
  resource: widgets
  verb: "get"
```

## Usage

```bash
kauthgate --config config/config.yaml
```

The gRPC proxy listens on port 6188 (h2c) and the HTTP proxy on port 8080.

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
    verbs: ["get", "create"]
```
