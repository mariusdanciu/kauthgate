use serde::Deserialize;

use crate::config::defs::Entity;
use crate::config::defs::SARAttributes;
use crate::config::defs::Upstream;

#[derive(Debug, Clone, Deserialize)]
pub struct RequestMatch {
    #[serde(default)]
    pub service: Option<String>,
    #[serde(rename = "grpc-methods", default)]
    pub grpc_methods: Option<Vec<String>>,
    #[serde(default)]
    pub headers: Option<Vec<Entity>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RBACMapping {
    pub name: String,
    pub request: RequestMatch,
    #[serde(rename = "sar-resource-attributes")]
    pub sar_resource_attributes: SARAttributes,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GrpcConfig {
    pub upstream: Upstream,
    pub mappings: Vec<RBACMapping>,
}
