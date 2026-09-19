use crate::config::grpc::AuthPolicy;

pub(crate) fn check_policy(
    policy: &AuthPolicy,
    service: &str,
    action: &str,
    has_header: impl Fn(&str) -> bool,
) -> bool {
    if policy.conditions.service != service {
        return false;
    }

    if !policy
        .conditions
        .allowed_actions
        .contains(&action.to_string())
    {
        return false;
    };

    for header in &policy.conditions.required_headers {
        if !has_header(&header) {
            return false;
        }
    }

    true
}

#[cfg(test)]
pub mod tests {
    // -- match_policy tests --
    use super::*;
    use crate::config::grpc::Conditions;
    use crate::config::SARAttributes;
    use crate::config::Binding;

    fn policy(service: &str, actions: &[&str], required_headers: &[&str]) -> AuthPolicy {
        AuthPolicy {
            name: "test-policy".into(),
            conditions: Conditions {
                service: service.into(),
                allowed_actions: actions.iter().map(|s| s.to_string()).collect(),
                required_headers: required_headers.iter().map(|s| s.to_string()).collect(),
            },
            resource_attributes: SARAttributes {
                namespace: Binding::Variable("tenant".into()),
                api_group: Binding::Literal("example.io".into()),
                resource: Binding::Literal("widgets".into()),
                verb: Binding::Literal("get".into()),
            },
        }
    }

    #[test]
    fn match_policy_matches_service_and_action() {
        let p = policy("my.Service", &["GetItem", "ListItems"], &[]);
        assert!(check_policy(&p, "my.Service", "GetItem", |_| true));
        assert!(check_policy(&p, "my.Service", "ListItems", |_| true));
    }

    #[test]
    fn match_policy_rejects_wrong_service() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(!check_policy(&p, "other.Service", "GetItem", |_| true));
    }

    #[test]
    fn match_policy_rejects_wrong_action() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(!check_policy(&p, "my.Service", "DeleteItem", |_| true));
    }

    #[test]
    fn match_policy_requires_all_headers() {
        let p = policy("my.Service", &["GetItem"], &["x-tenant-id", "x-request-id"]);
        let has = |h: &str| h == "x-tenant-id" || h == "x-request-id";
        assert!(check_policy(&p, "my.Service", "GetItem", has));
    }

    #[test]
    fn match_policy_rejects_missing_header() {
        let p = policy("my.Service", &["GetItem"], &["x-tenant-id", "x-request-id"]);
        let has = |h: &str| h == "x-tenant-id";
        assert!(!check_policy(&p, "my.Service", "GetItem", has));
    }

    #[test]
    fn match_policy_no_required_headers_always_passes() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(check_policy(&p, "my.Service", "GetItem", |_| false));
    }
}
