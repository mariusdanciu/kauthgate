use serde::Deserialize;

use crate::config::defs::{Extractor, SARAttributes};

#[derive(Debug, Clone, Deserialize)]
pub struct Conditions {
    pub service: String,
    #[serde(rename = "allowed-actions")]
    pub allowed_actions: Vec<String>,
    #[serde(rename = "required-headers")]
    pub required_headers: Vec<String>,
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
