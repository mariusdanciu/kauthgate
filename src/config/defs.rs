use anyhow::Result;
use config::{Config, File};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Extractor {
    Header { name: String, header: String },
    Path { name: String, path: String },
    Query { name: String, parameter: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Conditions {
    pub service: String,
    #[serde(rename = "allowed-actions")]
    pub allowed_actions: Vec<String>,
    #[serde(rename = "required-headers")]
    pub required_headers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SARAttributes {
    pub namespace: String,
    #[serde(rename = "api-group")]
    pub api_group: String,
    pub resource: String,
    pub verb: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthPolicy {
    pub name: String,
    pub conditions: Conditions,
    #[serde(rename = "resource-attributes")]
    pub resource_attributes: SARAttributes,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GrpcConfig {
    pub extractors: Vec<Extractor>,
    #[serde(rename = "auth-policies")]
    pub auth_policies: Vec<AuthPolicy>,
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

// Load the configuration from the config file and the secret config file
pub fn load_config(config_file: String, secret_config_file: String) -> Result<ProxyConfig> {
    let config = Config::builder()
        .add_source(File::with_name(config_file.as_str()))
        .add_source(File::with_name(secret_config_file.as_str()).required(false))
        .build()?;

    let config: ProxyConfig = config.try_deserialize()?;
    Ok(config)
}
