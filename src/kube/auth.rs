use k8s_openapi::api::authentication::v1::{TokenReview, TokenReviewSpec};
use k8s_openapi::api::authorization::v1::{
    ResourceAttributes, SubjectAccessReview, SubjectAccessReviewSpec,
};
use kube::api::PostParams;
use kube::{Api, Client};
use moka::future::Cache;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{debug, info, warn, error};

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

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

pub struct KubeAuthClient {
    client: tokio::sync::OnceCell<Client>,
    token_review_audiences: Vec<String>,
    token_cache: Cache<String, Result<AuthInfo, AuthError>>,
    sar_cache: Cache<String, bool>,
}

impl KubeAuthClient {
    pub fn new(cache_ttl: Duration, token_review_audiences: Vec<String>) -> Self {
        Self {
            client: tokio::sync::OnceCell::new(),
            token_review_audiences,
            token_cache: Cache::builder()
                .time_to_live(cache_ttl)
                .max_capacity(10_000)
                .build(),
            sar_cache: Cache::builder()
                .time_to_live(cache_ttl)
                .max_capacity(10_000)
                .build(),
        }
    }

    async fn client(&self) -> Result<&Client, AuthError> {
        self.client
            .get_or_try_init(|| async {
                Client::try_default()
                    .await
                    .map_err(|e| AuthError::Internal(e.to_string()))
            })
            .await
    }

    pub async fn authenticate(&self, token: &str) -> Result<AuthInfo, AuthError> {
        let client = self.client().await?.clone();
        let token_owned = token.to_string();
        let audiences = self.token_review_audiences.clone();
        let cache_key = hash_token(token);

        self.token_cache
            .try_get_with(cache_key, async move {
                debug!("Performing TokenReview");
                let review = TokenReview {
                    spec: TokenReviewSpec {
                        token: Some(token_owned),
                        audiences: None, //Some(audiences.clone()),
                    },
                    ..Default::default()
                };

                let api: Api<TokenReview> = Api::all(client);
                let result = api
                    .create(&PostParams::default(), &review)
                    .await
                    .map_err(|e| {
                        error!("TokenReview failed: {:?}", e);
                        AuthError::Unauthenticated
                    })?;

                let status = result
                    .status
                    .ok_or(AuthError::Internal("TokenReview returned no status".into()))?;

                if !status.authenticated.unwrap_or(false) {
                    let reason = status.error.unwrap_or_else(|| "token not authenticated".into());
                    warn!("TokenReview rejected: {reason}");
                    return Ok(Err(AuthError::Unauthenticated));
                }

                if !has_compatible_audience(&audiences, status.audiences.as_deref()) {
                    warn!(
                        "TokenReview rejected: no compatible audience returned (requested={audiences:?}, returned={:?})",
                        status.audiences
                    );
                    return Ok(Err(AuthError::Unauthenticated));
                }

                let user = status
                    .user
                    .ok_or(AuthError::Internal("TokenReview returned no user info".into()))?;

                let info = AuthInfo {
                    username: user.username.unwrap_or_default(),
                    groups: user.groups.unwrap_or_default(),
                };
                debug!("TokenReview authenticated user: {}", info.username);
                Ok(Ok(info))
            })
            .await
            .map_err(|e: Arc<AuthError>| e.as_ref().clone())?
    }

    pub async fn authorize(
        &self,
        auth_info: &AuthInfo,
        resource_attributes: &ResourceAttributes,
    ) -> Result<(), AuthError> {
        let key = format!(
            "{}:{:?}:{:?}",
            auth_info.username, auth_info.groups, resource_attributes
        );
        let client = self.client().await?.clone();
        let username = auth_info.username.clone();
        let groups = auth_info.groups.clone();

        let allowed = self
            .sar_cache
            .try_get_with(key, async move {
                let sar = SubjectAccessReview {
                    spec: SubjectAccessReviewSpec {
                        user: Some(username),
                        groups: Some(groups),
                        resource_attributes: Some(resource_attributes.clone()),
                        ..Default::default()
                    },
                    ..Default::default()
                };

                info!("SAR request: user={}, groups={:?}, resource_attributes={:?}",
                    sar.spec.user.as_deref().unwrap_or(""), sar.spec.groups.as_deref().unwrap_or(&[]), sar.spec.resource_attributes);
                let api: Api<SubjectAccessReview> = Api::all(client);
                let result = api
                    .create(&PostParams::default(), &sar)
                    .await
                    .map_err(|e| AuthError::Unauthorized)?;

                let status = result
                    .status
                    .ok_or_else(|| AuthError::Internal("SAR returned no status".into()))?;

                info!("SubjectAccessReview result: allowed={}", status.allowed);
                Ok(status.allowed)
            })
            .await
            .map_err(|e: Arc<AuthError>| e.as_ref().clone())?;

        if allowed {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }
}

fn has_compatible_audience(requested: &[String], returned: Option<&[String]>) -> bool {
    if requested.is_empty() {
        return true;
    }

    returned
        .unwrap_or_default()
        .iter()
        .any(|audience| requested.iter().any(|candidate| candidate == audience))
}

#[cfg(test)]
mod tests {
    use super::has_compatible_audience;

    #[test]
    fn rejects_authenticated_response_without_returned_audience() {
        let requested = vec!["https://kubernetes.default.svc".to_string()];

        assert!(!has_compatible_audience(&requested, None));
    }

    #[test]
    fn rejects_authenticated_response_without_matching_audience() {
        let requested = vec!["https://kubernetes.default.svc".to_string()];
        let returned = vec!["https://other-audience".to_string()];

        assert!(!has_compatible_audience(
            &requested,
            Some(returned.as_slice())
        ));
    }

    #[test]
    fn accepts_authenticated_response_with_matching_audience() {
        let requested = vec!["https://kubernetes.default.svc".to_string()];
        let returned = vec![
            "https://other-audience".to_string(),
            "https://kubernetes.default.svc".to_string(),
        ];

        assert!(has_compatible_audience(
            &requested,
            Some(returned.as_slice())
        ));
    }
}
