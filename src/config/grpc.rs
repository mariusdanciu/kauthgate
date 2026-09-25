use serde::Deserialize;

use crate::config::defs::Entity;
use crate::config::defs::ProtocolConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct RequestMatch {
    #[serde(default)]
    pub service: Option<String>,
    #[serde(rename = "grpc-methods", default)]
    pub grpc_methods: Option<Vec<String>>,
    #[serde(default)]
    pub headers: Option<Vec<Entity>>,
}

pub type GrpcConfig = ProtocolConfig<RequestMatch>;
