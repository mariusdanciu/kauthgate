use crate::config::{Binding, ProxyConfig, SarAttributes};
use crate::kube::auth::AuthError;
use crate::kube::auth::AuthInfo;
use crate::kube::auth::KubeAuthClient;
use crate::utils::ConfigVariables;
use k8s_openapi::api::authorization::v1::ResourceAttributes;
use pingora::proxy::Session;
use tracing::error;

pub(crate) struct AuthorizationInfo<'a> {
    pub sar_resource_attributes: &'a SarAttributes,
    pub vars: &'a ConfigVariables,
    pub client: &'a KubeAuthClient,
    pub auth_info: &'a AuthInfo,
    pub config: &'a ProxyConfig,
}

pub(crate) fn inject_headers(
    session: &mut Session,
    config: &ProxyConfig,
    auth_info: &AuthInfo,
) -> Result<(), Box<pingora::Error>> {
    let groups_value = auth_info.groups.join(&config.auth.groups_header_delimiter);
    let headers = session.req_header_mut();
    headers.insert_header(config.auth.user_header.clone(), &auth_info.username)?;
    headers.insert_header(config.auth.groups_header.clone(), &groups_value)?;
    Ok(())
}

pub(crate) fn get_header(session: &Session, header: &str) -> Option<String> {
    session
        .req_header()
        .headers
        .get(header)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
}

fn resolve(value: Option<&Binding>, variables: &ConfigVariables) -> Option<String> {
    match value? {
        Binding::Variable(name) => variables.get(name.as_str()).map(|s| s.to_string()),
        Binding::Literal(value) => Some(value.to_string()),
    }
}

pub(crate) fn compile_resource_attributes(
    resource_attributes: &SarAttributes,
    variables: &ConfigVariables,
) -> ResourceAttributes {
    let namespace = resolve(resource_attributes.namespace.as_ref(), variables);
    let group = resolve(resource_attributes.api_group.as_ref(), variables);
    let version = resolve(resource_attributes.api_version.as_ref(), variables);
    let resource = resolve(resource_attributes.resource.as_ref(), variables);
    let sub_resource = resolve(resource_attributes.sub_resource.as_ref(), variables);
    let verb = resolve(resource_attributes.verb.as_ref(), variables);

    ResourceAttributes {
        namespace,
        group,
        resource,
        subresource: sub_resource,
        verb,
        version,
        ..Default::default()
    }
}

pub(crate) fn parse_bearer_token(session: &Session) -> Option<&str> {
    session
        .req_header()
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

pub(crate) fn path_to_vec(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

pub(crate) async fn run_authz(session: &mut Session, authz: &AuthorizationInfo<'_>) -> Result<(), AuthError> {
    let resource_attributes = compile_resource_attributes(authz.sar_resource_attributes, authz.vars);

    let resp = authz.client.authorize(authz.auth_info, &resource_attributes).await;

    if let Err(e) = resp {
        error!("authorization failed: {:?}", e);
        return Err(e);
    }
    inject_headers(session, authz.config, authz.auth_info).map_err(|e| AuthError::Internal(e.to_string()))?;
    Ok(()) // Successfully authorized. Continue to upstream.
}

#[cfg(test)]
mod tests {
    use super::path_to_vec;

    #[test]
    fn splits_absolute_path() {
        assert_eq!(path_to_vec("/api/v1/users"), vec!["api", "v1", "users"]);
    }

    #[test]
    fn splits_relative_path() {
        assert_eq!(path_to_vec("api/v1"), vec!["api", "v1"]);
    }

    #[test]
    fn root_path_returns_empty() {
        assert!(path_to_vec("/").is_empty());
    }

    #[test]
    fn empty_string_returns_empty() {
        assert!(path_to_vec("").is_empty());
    }

    #[test]
    fn ignores_consecutive_slashes() {
        assert_eq!(path_to_vec("/api//v1///users"), vec!["api", "v1", "users"]);
    }

    #[test]
    fn trailing_slash_ignored() {
        assert_eq!(path_to_vec("/api/v1/"), vec!["api", "v1"]);
    }

    #[test]
    fn single_segment() {
        assert_eq!(path_to_vec("/health"), vec!["health"]);
    }
}
