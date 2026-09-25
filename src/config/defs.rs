use crate::config::grpc::GrpcConfig;
use crate::config::http::HttpConfig;
use anyhow::Result;
use config::{Config, File};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, Deserialize)]
pub struct Entity {
    pub name: String,
    #[serde(deserialize_with = "deserialize_binding")]
    pub value: Binding,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Binding {
    Variable(String),
    Literal(String),
}

impl Binding {
    pub fn from_str(s: &str) -> Self {
        if let Some(inner) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            return Binding::Variable(inner.trim().to_string());
        }
        Binding::Literal(s.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SarAttributes {
    #[serde(default, deserialize_with = "deserialize_optional_binding")]
    pub namespace: Option<Binding>,
    #[serde(rename = "api-group", default, deserialize_with = "deserialize_optional_binding")]
    pub api_group: Option<Binding>,
    #[serde(rename = "api-version", default, deserialize_with = "deserialize_optional_binding")]
    pub api_version: Option<Binding>,
    #[serde(default, deserialize_with = "deserialize_optional_binding")]
    pub resource: Option<Binding>,
    #[serde(rename = "sub-resource", default, deserialize_with = "deserialize_optional_binding")]
    pub sub_resource: Option<Binding>,
    #[serde(default, deserialize_with = "deserialize_optional_binding")]
    pub verb: Option<Binding>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upstream {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(rename = "cache-ttl-secs")]
    pub cache_ttl_secs: u64,
    #[serde(rename = "token-review-audiences")]
    pub token_review_audiences: Vec<String>,
    #[serde(rename = "user-header", default = "default_user_header")]
    pub user_header: String,
    #[serde(rename = "groups-header", default = "default_groups_header")]
    pub groups_header: String,
    #[serde(rename = "groups-header-delimiter", default = "default_groups_header_delimiter")]
    pub groups_header_delimiter: String,
}

fn default_user_header() -> String {
    "x-remote-user".into()
}
fn default_groups_header() -> String {
    "x-remote-groups".into()
}
fn default_groups_header_delimiter() -> String {
    "|".into()
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TLSConfig {
    pub cert_file: String,
    pub key_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Listener {
    pub host: String,
    pub port: u16,
    #[allow(dead_code)]
    pub tls: Option<TLSConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrometheusConfig {
    pub listener: Listener,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    pub auth: AuthConfig,
    pub prometheus: Option<PrometheusConfig>,
    pub grpc: GrpcConfig,
    pub http: HttpConfig,
}

#[derive(Deserialize)]
enum Access {
    #[serde(rename = "sar-resource-attributes")]
    Sar(Box<SarAttributes>),
    #[serde(rename = "no-auth")]
    NoAuth,
}

#[derive(Debug, Clone)]
pub struct NoAuthRule<R> {
    pub name: String,
    pub request: R,
}

#[derive(Debug, Clone)]
pub struct SarRule<R> {
    pub name: String,
    pub request: R,
    pub sar: SarAttributes,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(from = "RawProtocolConfig<R>")]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
pub struct ProtocolConfig<R> {
    pub listener: Listener,
    pub upstream: Upstream,
    pub rules: Vec<SarRule<R>>,
    pub no_auth_rules: Vec<NoAuthRule<R>>,
}

#[derive(Deserialize)]
struct RawRule<R> {
    name: String,
    request: R,
    access: Access,
}

#[derive(Deserialize)]
struct RawProtocolConfig<R> {
    listener: Listener,
    upstream: Upstream,
    rules: Vec<RawRule<R>>,
}

impl<R> From<RawProtocolConfig<R>> for ProtocolConfig<R> {
    fn from(raw: RawProtocolConfig<R>) -> Self {
        let mut rules = Vec::new();
        let mut no_auth_rules = Vec::new();
        for raw_rule in raw.rules {
            match raw_rule.access {
                Access::Sar(sar) => rules.push(SarRule {
                    name: raw_rule.name,
                    request: raw_rule.request,
                    sar: *sar,
                }),
                Access::NoAuth => no_auth_rules.push(NoAuthRule {
                    name: raw_rule.name,
                    request: raw_rule.request,
                }),
            }
        }
        ProtocolConfig {
            listener: raw.listener,
            upstream: raw.upstream,
            rules,
            no_auth_rules,
        }
    }
}

pub fn load_config(config_file: String, secret_config_file: String) -> Result<ProxyConfig> {
    let config = Config::builder()
        .add_source(File::with_name(config_file.as_str()))
        .add_source(File::with_name(secret_config_file.as_str()).required(false))
        .build()?;

    let config: ProxyConfig = config.try_deserialize()?;
    Ok(config)
}

pub(crate) fn deserialize_binding<'de, D>(deserializer: D) -> std::result::Result<Binding, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Binding::from_str(&s))
}

pub(crate) fn deserialize_optional_binding<'de, D>(deserializer: D) -> std::result::Result<Option<Binding>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(Some(Binding::from_str(&s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml(yaml: &str) -> ProxyConfig {
        let config = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap();
        config.try_deserialize().unwrap()
    }

    const FULL_CONFIG: &str = r#"

auth:
  cache-ttl-secs: 300
  token-review-audiences:
    - aud1

grpc:
  listener:
    host: 0.0.0.0
    port: 6188
  upstream:
    host: 127.0.0.1
    port: 50051
  rules:
    - name: flight
      request:
        service: arrow.flight.protocol.FlightService
        grpc-methods:
          - DoAction
          - DoGet
        headers:
          - name: x-tenant-id
            value: "{tenant-id}"
      access:
        sar-resource-attributes:
          namespace: "{tenant-id}"
          api-group: example.io
          resource: widgets
          verb: get

http:
  listener:
    host: 0.0.0.0
    port: 8080
  upstream:
    host: 127.0.0.1
    port: 8081
  rules: []
"#;

    #[test]
    fn deserializes_full_config() {
        let cfg = parse_yaml(FULL_CONFIG);
        assert_eq!(cfg.grpc.upstream.host, "127.0.0.1");
        assert_eq!(cfg.grpc.upstream.port, 50051);
        assert_eq!(cfg.auth.cache_ttl_secs, 300);
        assert_eq!(cfg.auth.token_review_audiences, vec!["aud1"]);
    }

    #[test]
    fn deserializes_auth_rule() {
        let cfg = parse_yaml(FULL_CONFIG);
        let rule = &cfg.grpc.rules[0];
        assert_eq!(rule.name, "flight");
        assert_eq!(
            rule.request.service.as_deref(),
            Some("arrow.flight.protocol.FlightService")
        );
        assert_eq!(rule.request.grpc_methods, Some(vec!["DoAction".into(), "DoGet".into()]));
        let headers = rule.request.headers.as_ref().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].name, "x-tenant-id");
        assert_eq!(headers[0].value, Binding::Variable("tenant-id".into()));
        assert_eq!(rule.sar.namespace, Some(Binding::Variable("tenant-id".into())));
        assert_eq!(rule.sar.api_group, Some(Binding::Literal("example.io".into())));
        assert_eq!(rule.sar.resource, Some(Binding::Literal("widgets".into())));
        assert_eq!(rule.sar.verb, Some(Binding::Literal("get".into())));
    }

    #[test]
    fn deserializes_empty_audiences() {
        let yaml = r#"
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  listener:
    host: 0.0.0.0
    port: 6188
  upstream:
    host: localhost
    port: 50051
  rules: []
http:
  listener:
    host: 0.0.0.0
    port: 8080
  upstream:
    host: localhost
    port: 8081
  rules: []
"#;
        let cfg = parse_yaml(yaml);
        assert!(cfg.auth.token_review_audiences.is_empty());
        assert!(cfg.grpc.rules.is_empty());
    }

    fn parse_binding(value: &str) -> Binding {
        let yaml = format!(
            r#"
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  listener:
    host: 0.0.0.0
    port: 6188
  upstream:
    host: localhost
    port: 8080
  rules:
    - name: test
      request:
        service: svc
        grpc-methods: []
        headers: []
      access:
        sar-resource-attributes:
          namespace: "{value}"
          api-group: g
          resource: r
          verb: v
http:
  listener:
    host: 0.0.0.0
    port: 8080
  upstream:
    host: localhost
    port: 8081
  rules: []
"#,
            value = value
        );
        let cfg = parse_yaml(&yaml);
        cfg.grpc.rules[0].sar.namespace.clone().unwrap()
    }

    #[test]
    fn binding_variable_from_braces() {
        assert_eq!(parse_binding("{tenant}"), Binding::Variable("tenant".into()));
    }

    #[test]
    fn binding_literal_from_plain_string() {
        assert_eq!(parse_binding("my-namespace"), Binding::Literal("my-namespace".into()));
    }

    #[test]
    fn binding_partial_open_brace_is_literal() {
        assert_eq!(parse_binding("{tenant"), Binding::Literal("{tenant".into()));
    }

    #[test]
    fn binding_partial_close_brace_is_literal() {
        assert_eq!(parse_binding("tenant}"), Binding::Literal("tenant}".into()));
    }

    #[test]
    fn binding_empty_string_is_literal() {
        assert_eq!(parse_binding(""), Binding::Literal("".into()));
    }

    #[test]
    fn binding_empty_braces_is_variable() {
        assert_eq!(parse_binding("{}"), Binding::Variable("".into()));
    }

    #[test]
    fn binding_nested_braces() {
        assert_eq!(parse_binding("{{inner}}"), Binding::Variable("{inner}".into()));
    }
}
