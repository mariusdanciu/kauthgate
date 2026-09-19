use anyhow::Result;
use config::{Config, File};
use serde::{Deserialize, Deserializer};
use crate::config::grpc::GrpcConfig;


#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Extractor {
    Header {
        name: String,
        header: String,
    },
    Path {
        #[serde(deserialize_with = "deserialize_path_segments")]
        path: Vec<String>,
    },
    Query {
        name: String,
        parameter: String,
    },
}



#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Binding {
    Variable(String),
    Literal(String),
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
    pub upstream: Upstream,
    pub auth: AuthConfig,
    pub grpc: GrpcConfig,
}

pub fn load_config(config_file: String, secret_config_file: String) -> Result<ProxyConfig> {
    let config = Config::builder()
        .add_source(File::with_name(config_file.as_str()))
        .add_source(File::with_name(secret_config_file.as_str()).required(false))
        .build()?;

    let config: ProxyConfig = config.try_deserialize()?;
    Ok(config)
}

fn deserialize_path_segments<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    Ok(s.split('/')
        .filter(|seg| !seg.is_empty())
        .map(|seg| seg.to_string())
        .collect())
}

fn deserialize_binding<'de, D>(deserializer: D) -> std::result::Result<Binding, D::Error>
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
upstream:
  host: 127.0.0.1
  port: 50051

auth:
  cache-ttl-secs: 300
  token-review-audiences:
    - aud1

grpc:
  extractors:
    - name: tenant-id
      header: x-tenant-id
    - path: /svc/{arg1}/sub/{arg2}
  auth-policies:
    - name: flight
      conditions:
        service: arrow.flight.protocol.FlightService
        allowed-actions:
          - DoAction
          - DoGet
        required-headers:
          - x-tenant-id
      resource-attributes:
        namespace: "{tenant-id}"
        api-group: example.io
        resource: widgets
        verb: get
"#;

    #[test]
    fn deserializes_full_config() {
        let cfg = parse_yaml(FULL_CONFIG);
        assert_eq!(cfg.upstream.host, "127.0.0.1");
        assert_eq!(cfg.upstream.port, 50051);
        assert_eq!(cfg.auth.cache_ttl_secs, 300);
        assert_eq!(cfg.auth.token_review_audiences, vec!["aud1"]);
    }

    #[test]
    fn deserializes_header_extractor() {
        let cfg = parse_yaml(FULL_CONFIG);
        let ext = &cfg.grpc.extractors[0];
        match ext {
            Extractor::Header { name, header } => {
                assert_eq!(name, "tenant-id");
                assert_eq!(header, "x-tenant-id");
            }
            other => panic!("expected Header extractor, got {:?}", other),
        }
    }

    #[test]
    fn deserializes_path_extractor_splits_segments() {
        let cfg = parse_yaml(FULL_CONFIG);
        let ext = &cfg.grpc.extractors[1];
        match ext {
            Extractor::Path { path } => {
                assert_eq!(path, &vec!["svc", "{arg1}", "sub", "{arg2}"]);
            }
            other => panic!("expected Path extractor, got {:?}", other),
        }
    }

    #[test]
    fn deserializes_auth_policy() {
        let cfg = parse_yaml(FULL_CONFIG);
        let policy = &cfg.grpc.auth_policies[0];
        assert_eq!(policy.name, "flight");
        assert_eq!(
            policy.conditions.service,
            "arrow.flight.protocol.FlightService"
        );
        assert_eq!(policy.conditions.allowed_actions, vec!["DoAction", "DoGet"]);
        assert_eq!(policy.conditions.required_headers, vec!["x-tenant-id"]);
        assert_eq!(policy.resource_attributes.namespace, Binding::Variable("tenant-id".into()));
        assert_eq!(policy.resource_attributes.api_group, Binding::Literal("example.io".into()));
        assert_eq!(policy.resource_attributes.resource, Binding::Literal("widgets".into()));
        assert_eq!(policy.resource_attributes.verb, Binding::Literal("get".into()));
    }

    #[test]
    fn deserializes_empty_audiences() {
        let yaml = r#"
upstream:
  host: localhost
  port: 8080
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  extractors: []
  auth-policies: []
"#;
        let cfg = parse_yaml(yaml);
        assert!(cfg.auth.token_review_audiences.is_empty());
        assert!(cfg.grpc.extractors.is_empty());
        assert!(cfg.grpc.auth_policies.is_empty());
    }

    #[test]
    fn deserializes_path_extractor_with_leading_slash() {
        let yaml = r#"
upstream:
  host: localhost
  port: 8080
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  extractors:
    - path: /{a}/{b}/{c}
  auth-policies: []
"#;
        let cfg = parse_yaml(yaml);
        match &cfg.grpc.extractors[0] {
            Extractor::Path { path } => {
                assert_eq!(path, &vec!["{a}", "{b}", "{c}"]);
            }
            other => panic!("expected Path extractor, got {:?}", other),
        }
    }

    fn parse_binding(value: &str) -> Binding {
        let yaml = format!(
            r#"
upstream:
  host: localhost
  port: 8080
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  extractors: []
  auth-policies:
    - name: test
      conditions:
        service: svc
        allowed-actions: []
        required-headers: []
      resource-attributes:
        namespace: "{value}"
        api-group: g
        resource: r
        verb: v
"#,
            value = value
        );
        let cfg = parse_yaml(&yaml);
        cfg.grpc.auth_policies[0].resource_attributes.namespace.clone()
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

    #[test]
    fn deserializes_query_extractor() {
        let yaml = r#"
upstream:
  host: localhost
  port: 8080
auth:
  cache-ttl-secs: 60
  token-review-audiences: []
grpc:
  extractors:
    - name: page-size
      parameter: pageSize
  auth-policies: []
"#;
        let cfg = parse_yaml(yaml);
        match &cfg.grpc.extractors[0] {
            Extractor::Query { name, parameter } => {
                assert_eq!(name, "page-size");
                assert_eq!(parameter, "pageSize");
            }
            other => panic!("expected Query extractor, got {:?}", other),
        }
    }
}
