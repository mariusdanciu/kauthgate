use crate::auth::{AuthClient, AuthInfo, RbacAttributes};
use crate::config::{Binding, NoAuthRule, ProxyConfig, SarAttributes, SarRule};
use crate::utils::ConfigVariables;
use pingora::proxy::Session;
use tracing::error;

#[derive(Debug)]
pub(crate) struct AllowedRequest {
    pub rule_name: String,
    pub auth_info: Option<AuthInfo>,
}

pub(crate) async fn evaluate_request<R>(
    bearer_token: Option<&str>,
    rules: &[SarRule<R>],
    no_auth_rules: &[NoAuthRule<R>],
    match_rule: impl Fn(&R) -> Option<ConfigVariables>,
    client: &dyn AuthClient,
) -> Result<AllowedRequest, String> {
    match bearer_token {
        Some(token) => match client.authenticate(token).await {
            Ok(auth_info) => {
                for rule in rules {
                    if let Some(vars) = match_rule(&rule.matches) {
                        let attrs = compile_resource_attributes(&rule.sar, &vars);
                        if let Err(e) = client.authorize(&auth_info, &attrs).await {
                            error!("authorization failed: {:?}", e);
                            return Err(e.to_string());
                        }
                        return Ok(AllowedRequest {
                            rule_name: rule.name.clone(),
                            auth_info: Some(auth_info),
                        });
                    }
                }
            },
            Err(e) => {
                error!("authentication failed: {:?}", e);
                return Err(e.to_string());
            },
        },
        None => {
            for rule in no_auth_rules {
                if match_rule(&rule.matches).is_some() {
                    return Ok(AllowedRequest {
                        rule_name: rule.name.clone(),
                        auth_info: None,
                    });
                }
            }
        },
    }
    Err("no rule matched".to_string())
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
) -> RbacAttributes {
    let namespace = resolve(resource_attributes.namespace.as_ref(), variables);
    let api_group = resolve(resource_attributes.api_group.as_ref(), variables);
    let api_version = resolve(resource_attributes.api_version.as_ref(), variables);
    let resource = resolve(resource_attributes.resource.as_ref(), variables);
    let sub_resource = resolve(resource_attributes.sub_resource.as_ref(), variables);
    let verb = resolve(resource_attributes.verb.as_ref(), variables);

    RbacAttributes {
        namespace,
        api_group,
        resource,
        sub_resource,
        verb,
        api_version,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthClient, AuthError, AuthInfo, RbacAttributes};
    use crate::config::{Binding, NoAuthRule, SarAttributes, SarRule};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockAuthClient {
        auth_result: Result<AuthInfo, AuthError>,
        authz_result: Result<(), AuthError>,
        authorize_calls: Mutex<Vec<(AuthInfo, RbacAttributes)>>,
    }

    impl MockAuthClient {
        fn allowing(username: &str, groups: &[&str]) -> Self {
            Self {
                auth_result: Ok(AuthInfo {
                    username: username.to_string(),
                    groups: groups.iter().map(|s| s.to_string()).collect(),
                }),
                authz_result: Ok(()),
                authorize_calls: Mutex::new(Vec::new()),
            }
        }

        fn unauthenticated() -> Self {
            Self {
                auth_result: Err(AuthError::Unauthenticated),
                authz_result: Ok(()),
                authorize_calls: Mutex::new(Vec::new()),
            }
        }

        fn unauthorized(username: &str) -> Self {
            Self {
                auth_result: Ok(AuthInfo {
                    username: username.to_string(),
                    groups: vec![],
                }),
                authz_result: Err(AuthError::Unauthorized),
                authorize_calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AuthClient for MockAuthClient {
        async fn authenticate(&self, _token: &str) -> Result<AuthInfo, AuthError> {
            self.auth_result.clone()
        }

        async fn authorize(&self, auth_info: &AuthInfo, attrs: &RbacAttributes) -> Result<(), AuthError> {
            self.authorize_calls
                .lock()
                .unwrap()
                .push((auth_info.clone(), attrs.clone()));
            self.authz_result.clone()
        }
    }

    fn sar_attrs(namespace: &str, resource: &str, verb: &str) -> SarAttributes {
        SarAttributes {
            namespace: Some(Binding::Literal(namespace.to_string())),
            api_group: Some(Binding::Literal("example.io".to_string())),
            api_version: None,
            resource: Some(Binding::Literal(resource.to_string())),
            sub_resource: None,
            verb: Some(Binding::Literal(verb.to_string())),
        }
    }

    fn sar_rule(name: &str, sar: SarAttributes) -> SarRule<()> {
        SarRule {
            name: name.to_string(),
            matches: (),
            sar,
        }
    }

    fn no_auth_rule(name: &str) -> NoAuthRule<()> {
        NoAuthRule {
            name: name.to_string(),
            matches: (),
        }
    }

    fn always_match(_: &()) -> Option<ConfigVariables> {
        Some(HashMap::new())
    }

    fn never_match(_: &()) -> Option<ConfigVariables> {
        None
    }

    #[tokio::test]
    async fn allows_authenticated_and_authorized_request() {
        let client = MockAuthClient::allowing("alice", &["devs"]);
        let rules = vec![sar_rule("r1", sar_attrs("default", "pods", "get"))];

        let allowed = evaluate_request(Some("valid-token"), &rules, &[], always_match, &client)
            .await
            .unwrap();

        assert_eq!(allowed.rule_name, "r1");
        let info = allowed.auth_info.unwrap();
        assert_eq!(info.username, "alice");
        assert_eq!(info.groups, vec!["devs"]);
    }

    #[tokio::test]
    async fn denies_unauthenticated_token() {
        let client = MockAuthClient::unauthenticated();
        let rules = vec![sar_rule("r1", sar_attrs("default", "pods", "get"))];

        let err = evaluate_request(Some("bad-token"), &rules, &[], always_match, &client)
            .await
            .unwrap_err();

        assert_eq!(err, "Unauthenticated");
    }

    #[tokio::test]
    async fn denies_unauthorized_user() {
        let client = MockAuthClient::unauthorized("bob");
        let rules = vec![sar_rule("r1", sar_attrs("default", "pods", "get"))];

        let err = evaluate_request(Some("valid-token"), &rules, &[], always_match, &client)
            .await
            .unwrap_err();

        assert_eq!(err, "Unauthorized");
    }

    #[tokio::test]
    async fn denies_when_no_rule_matches() {
        let client = MockAuthClient::allowing("alice", &[]);
        let rules: Vec<SarRule<()>> = vec![sar_rule("r1", sar_attrs("default", "pods", "get"))];

        let err = evaluate_request(Some("valid-token"), &rules, &[], never_match, &client)
            .await
            .unwrap_err();

        assert_eq!(err, "no rule matched");
    }

    #[tokio::test]
    async fn allows_no_auth_rule_without_token() {
        let client = MockAuthClient::unauthenticated();
        let no_auth_rules = vec![no_auth_rule("health")];

        let allowed = evaluate_request::<()>(None, &[], &no_auth_rules, always_match, &client)
            .await
            .unwrap();

        assert_eq!(allowed.rule_name, "health");
        assert!(allowed.auth_info.is_none());
    }

    #[tokio::test]
    async fn denies_no_token_when_no_auth_rule_doesnt_match() {
        let client = MockAuthClient::unauthenticated();
        let no_auth_rules = vec![no_auth_rule("health")];

        let err = evaluate_request::<()>(None, &[], &no_auth_rules, never_match, &client)
            .await
            .unwrap_err();

        assert_eq!(err, "no rule matched");
    }

    #[tokio::test]
    async fn denies_no_token_and_no_rules() {
        let client = MockAuthClient::unauthenticated();

        let err = evaluate_request::<()>(None, &[], &[], never_match, &client)
            .await
            .unwrap_err();

        assert_eq!(err, "no rule matched");
    }

    #[tokio::test]
    async fn matches_first_rule_and_skips_rest() {
        let client = MockAuthClient::allowing("alice", &[]);
        let rules = vec![
            sar_rule("first", sar_attrs("ns1", "pods", "get")),
            sar_rule("second", sar_attrs("ns2", "pods", "list")),
        ];

        let allowed = evaluate_request(Some("token"), &rules, &[], always_match, &client)
            .await
            .unwrap();

        assert_eq!(allowed.rule_name, "first");
    }

    #[tokio::test]
    async fn compiles_sar_attributes_for_authorization() {
        let client = MockAuthClient::allowing("alice", &[]);
        let rules = vec![sar_rule("r1", sar_attrs("prod", "deployments", "create"))];

        evaluate_request(Some("token"), &rules, &[], always_match, &client)
            .await
            .unwrap();

        let calls = client.authorize_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (info, attrs) = &calls[0];
        assert_eq!(info.username, "alice");
        assert_eq!(attrs.namespace.as_deref(), Some("prod"));
        assert_eq!(attrs.resource.as_deref(), Some("deployments"));
        assert_eq!(attrs.verb.as_deref(), Some("create"));
        assert_eq!(attrs.api_group.as_deref(), Some("example.io"));
    }

    #[tokio::test]
    async fn resolves_variables_in_sar_attributes() {
        let client = MockAuthClient::allowing("alice", &[]);
        let sar = SarAttributes {
            namespace: Some(Binding::Variable("tenant".to_string())),
            api_group: Some(Binding::Literal("example.io".to_string())),
            api_version: None,
            resource: Some(Binding::Literal("widgets".to_string())),
            sub_resource: None,
            verb: Some(Binding::Literal("get".to_string())),
        };
        let rules = vec![sar_rule("r1", sar)];

        let match_with_vars = |_: &()| -> Option<ConfigVariables> {
            let mut vars = HashMap::new();
            vars.insert("tenant".to_string(), "acme-corp".to_string());
            Some(vars)
        };

        evaluate_request(Some("token"), &rules, &[], match_with_vars, &client)
            .await
            .unwrap();

        let calls = client.authorize_calls.lock().unwrap();
        assert_eq!(calls[0].1.namespace.as_deref(), Some("acme-corp"));
    }

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
