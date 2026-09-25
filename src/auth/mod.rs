use anyhow::Result;
use async_trait::async_trait;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum AuthError {
    #[error("Unauthenticated")]
    Unauthenticated,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Internal auth error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub username: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RbacAttributes {
    pub namespace: Option<String>,
    pub api_group: Option<String>,
    pub api_version: Option<String>,
    pub resource: Option<String>,
    pub sub_resource: Option<String>,
    pub verb: Option<String>,
}

#[async_trait]
pub trait AuthClient: Send + Sync {
    async fn authenticate(&self, token: &str) -> Result<AuthInfo, AuthError>;
    async fn authorize(&self, auth_info: &AuthInfo, attrs: &RbacAttributes) -> Result<(), AuthError>;
}
