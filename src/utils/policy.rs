use crate::config::SARAttributes;
use crate::config::{AuthPolicy, Extractor};
use k8s_openapi::api::authorization::v1::ResourceAttributes;
use std::collections::HashMap;

pub(crate) fn match_policy(
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

fn resolve(value: &str, variables: &HashMap<String, String>) -> String {
    value
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .and_then(|key| variables.get(key))
        .cloned()
        .unwrap_or_else(|| value.to_string())
}

pub(crate) fn compile_resource_attributes(
    resource_attributes: &SARAttributes,
    variables: &HashMap<String, String>,
) -> ResourceAttributes {
    let namespace = resolve(&resource_attributes.namespace, variables);
    let group = resolve(&resource_attributes.api_group, variables);
    let resource = resolve(&resource_attributes.resource, variables);
    let verb = resolve(&resource_attributes.verb, variables);

    ResourceAttributes {
        namespace: Some(namespace),
        group: Some(group),
        resource: Some(resource),
        verb: Some(verb),
        ..Default::default()
    }
}

pub(crate) fn extract_variables(
    extractors: Vec<Extractor>,
    header_fn: impl Fn(&str) -> Option<String>,
) -> HashMap<String, String> {
    let mut variables = HashMap::new();
    for extractor in extractors {
        match extractor {
            Extractor::Header { name, header } => {
                if let Some(value) = header_fn(&header) {
                    variables.insert(name, value);
                }
            }
            Extractor::Path { name, path } => {}
            Extractor::Query { name, parameter } => {}
        }
    }
    variables
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    use crate::config::{AuthPolicy, Conditions, SARAttributes};

    fn policy(service: &str, actions: &[&str], required_headers: &[&str]) -> AuthPolicy {
        AuthPolicy {
            name: "test-policy".into(),
            conditions: Conditions {
                service: service.into(),
                allowed_actions: actions.iter().map(|s| s.to_string()).collect(),
                required_headers: required_headers.iter().map(|s| s.to_string()).collect(),
            },
            resource_attributes: SARAttributes {
                namespace: "{tenant}".into(),
                api_group: "example.io".into(),
                resource: "widgets".into(),
                verb: "get".into(),
            },
        }
    }

    // -- match_policy tests --

    #[test]
    fn match_policy_matches_service_and_action() {
        let p = policy("my.Service", &["GetItem", "ListItems"], &[]);
        assert!(match_policy(&p, "my.Service", "GetItem", |_| true));
        assert!(match_policy(&p, "my.Service", "ListItems", |_| true));
    }

    #[test]
    fn match_policy_rejects_wrong_service() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(!match_policy(&p, "other.Service", "GetItem", |_| true));
    }

    #[test]
    fn match_policy_rejects_wrong_action() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(!match_policy(&p, "my.Service", "DeleteItem", |_| true));
    }

    #[test]
    fn match_policy_requires_all_headers() {
        let p = policy("my.Service", &["GetItem"], &["x-tenant-id", "x-request-id"]);
        let has = |h: &str| h == "x-tenant-id" || h == "x-request-id";
        assert!(match_policy(&p, "my.Service", "GetItem", has));
    }

    #[test]
    fn match_policy_rejects_missing_header() {
        let p = policy("my.Service", &["GetItem"], &["x-tenant-id", "x-request-id"]);
        let has = |h: &str| h == "x-tenant-id";
        assert!(!match_policy(&p, "my.Service", "GetItem", has));
    }

    #[test]
    fn match_policy_no_required_headers_always_passes() {
        let p = policy("my.Service", &["GetItem"], &[]);
        assert!(match_policy(&p, "my.Service", "GetItem", |_| false));
    }

    // -- compile_resource_attributes tests --

    #[test]
    fn compile_resolves_variable_in_namespace() {
        let attrs = SARAttributes {
            namespace: "{tenant}".into(),
            api_group: "example.io".into(),
            resource: "widgets".into(),
            verb: "get".into(),
        };
        let variables = vars(&[("tenant", "my-namespace")]);
        let result = compile_resource_attributes(&attrs, &variables);
        assert_eq!(result.namespace.unwrap(), "my-namespace");
        assert_eq!(result.group.unwrap(), "example.io");
        assert_eq!(result.resource.unwrap(), "widgets");
        assert_eq!(result.verb.unwrap(), "get");
    }

    #[test]
    fn compile_leaves_literals_unchanged() {
        let attrs = SARAttributes {
            namespace: "fixed-ns".into(),
            api_group: "example.io".into(),
            resource: "widgets".into(),
            verb: "list".into(),
        };
        let result = compile_resource_attributes(&attrs, &vars(&[]));
        assert_eq!(result.namespace.unwrap(), "fixed-ns");
    }

    #[test]
    fn compile_keeps_placeholder_when_variable_missing() {
        let attrs = SARAttributes {
            namespace: "{unknown}".into(),
            api_group: "example.io".into(),
            resource: "widgets".into(),
            verb: "get".into(),
        };
        let result = compile_resource_attributes(&attrs, &vars(&[]));
        assert_eq!(result.namespace.unwrap(), "{unknown}");
    }

    #[test]
    fn compile_resolves_variables_in_all_fields() {
        let attrs = SARAttributes {
            namespace: "{ns}".into(),
            api_group: "{group}".into(),
            resource: "{res}".into(),
            verb: "{verb}".into(),
        };
        let variables = vars(&[
            ("ns", "prod"),
            ("group", "apps"),
            ("res", "deployments"),
            ("verb", "create"),
        ]);
        let result = compile_resource_attributes(&attrs, &variables);
        assert_eq!(result.namespace.unwrap(), "prod");
        assert_eq!(result.group.unwrap(), "apps");
        assert_eq!(result.resource.unwrap(), "deployments");
        assert_eq!(result.verb.unwrap(), "create");
    }

    // -- resolve tests --

    #[test]
    fn resolve_substitutes_variable() {
        let variables = vars(&[("tenant", "my-namespace")]);
        assert_eq!(resolve("{tenant}", &variables), "my-namespace");
    }

    #[test]
    fn resolve_returns_literal_when_no_braces() {
        let variables = vars(&[("tenant", "my-namespace")]);
        assert_eq!(resolve("literal-value", &variables), "literal-value");
    }

    #[test]
    fn resolve_returns_original_when_variable_not_found() {
        let variables = vars(&[]);
        assert_eq!(resolve("{missing}", &variables), "{missing}");
    }

    #[test]
    fn resolve_does_not_substitute_partial_braces() {
        let variables = vars(&[("tenant", "my-namespace")]);
        assert_eq!(resolve("{tenant", &variables), "{tenant");
        assert_eq!(resolve("tenant}", &variables), "tenant}");
    }

    #[test]
    fn resolve_handles_empty_string() {
        let variables = vars(&[("", "empty-key")]);
        assert_eq!(resolve("", &variables), "");
    }

    #[test]
    fn resolve_handles_empty_braces() {
        let variables = vars(&[("", "empty-key")]);
        assert_eq!(resolve("{}", &variables), "empty-key");
    }

    #[test]
    fn resolve_does_not_recurse() {
        let variables = vars(&[("a", "{b}"), ("b", "resolved")]);
        assert_eq!(resolve("{a}", &variables), "{b}");
    }

    #[test]
    fn resolve_treats_nested_braces_as_literal_key() {
        // strip_prefix('{') / strip_suffix('}') yields "{inner}",
        // so it looks up the key "{inner}" in the map
        let variables = vars(&[("{inner}", "matched"), ("inner", "yes")]);
        assert_eq!(resolve("{{inner}}", &variables), "matched");
    }

    #[test]
    fn resolve_nested_braces_not_found_returns_original() {
        let variables = vars(&[("inner", "yes")]);
        assert_eq!(resolve("{{inner}}", &variables), "{{inner}}");
    }
}
