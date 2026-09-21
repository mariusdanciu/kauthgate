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
pub struct SARAttributes {
    #[serde(deserialize_with = "deserialize_binding")]
    pub namespace: Binding,
    #[serde(rename = "api-group")]
    #[serde(deserialize_with = "deserialize_binding")]
    pub api_group: Binding,
    #[serde(deserialize_with = "deserialize_binding")]
    pub resource: Binding,
    #[serde(deserialize_with = "deserialize_binding")]
    pub verb: Binding,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    pub auth: AuthConfig,
    pub grpc: GrpcConfig,
    pub http: HttpConfig,
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
    if s.starts_with('{') && s.ends_with('}') {
        return Ok(Binding::Variable(
            s.strip_prefix('{')
                .unwrap()
                .strip_suffix('}')
                .unwrap()
                .to_string(),
        ));
    }
    Ok(Binding::Literal(s))
}

///----------------------------------------
/// Tests
///----------------------------------------

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
  upstream:
    host: 127.0.0.1
    port: 50051
  mappings:
    - name: flight
      conditions:
        service: arrow.flight.protocol.FlightService
        grpc-methods:
          - DoAction
          - DoGet
        headers:
          - name: x-tenant-id
            value: "{tenant-id}"
      sar-resource-attributes:
        namespace: "{tenant-id}"
        api-group: example.io
        resource: widgets
        verb: get

http:
  upstream:
    host: 127.0.0.1
    port: 8081
  mappings: []
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
    fn deserializes_auth_policy() {
        let cfg = parse_yaml(FULL_CONFIG);
        let policy = &cfg.grpc.mappings[0];
        assert_eq!(policy.name, "flight");
        assert_eq!(
            policy.conditions.service.as_deref(),
            Some("arrow.flight.protocol.FlightService")
        );
        assert_eq!(
            policy.conditions.grpc_methods,
            Some(vec!["DoAction".into(), "DoGet".into()])
        );
        let headers = policy.conditions.headers.as_ref().unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].name, "x-tenant-id");
        assert_eq!(headers[0].value, Binding::Variable("tenant-id".into()));
        assert_eq!(
            policy.sar_resource_attributes.namespace,
            Binding::Variable("tenant-id".into())
        );
        assert_eq!(
            policy.sar_resource_attributes.api_group,
            Binding::Literal("example.io".into())
        );
        assert_eq!(
            policy.sar_resource_attributes.resource,
            Binding::Literal("widgets".into())
        );
        assert_eq!(
            policy.sar_resource_attributes.verb,
            Binding::Literal("get".into())
        );
    }

    #[test]
    fn deserializes_empty_audiences() {
        let yaml = r#"
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  upstream:
    host: localhost
    port: 50051
  mappings: []
http:
  upstream:
    host: localhost
    port: 8081
  mappings: []
"#;
        let cfg = parse_yaml(yaml);
        assert!(cfg.auth.token_review_audiences.is_empty());
        assert!(cfg.grpc.mappings.is_empty());
    }

    fn parse_binding(value: &str) -> Binding {
        let yaml = format!(
            r#"
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  upstream:
    host: localhost
    port: 8080
  mappings:
    - name: test
      conditions:
        service: svc
        grpc-methods: []
        headers: []
      sar-resource-attributes:
        namespace: "{value}"
        api-group: g
        resource: r
        verb: v
http:
  upstream:
    host: localhost
    port: 8081
  mappings: []
"#,
            value = value
        );
        let cfg = parse_yaml(&yaml);
        cfg.grpc.mappings[0]
            .sar_resource_attributes
            .namespace
            .clone()
    }

    #[test]
    fn binding_variable_from_braces() {
        assert_eq!(
            parse_binding("{tenant}"),
            Binding::Variable("tenant".into())
        );
    }

    #[test]
    fn binding_literal_from_plain_string() {
        assert_eq!(
            parse_binding("my-namespace"),
            Binding::Literal("my-namespace".into())
        );
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
        assert_eq!(
            parse_binding("{{inner}}"),
            Binding::Variable("{inner}".into())
        );
    }
}
