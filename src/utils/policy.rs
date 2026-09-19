use crate::config::Binding;
use crate::config::Extractor;
use crate::config::SARAttributes;
use k8s_openapi::api::authorization::v1::ResourceAttributes;
use std::collections::HashMap;

fn resolve(value: &Binding, variables: &HashMap<&str, String>) -> Option<String> {
    match value {
        Binding::Variable(name) => variables.get(name.as_str()).map(|s| s.to_string()),
        Binding::Literal(value) => Some(value.to_string()),
    }
}

pub(crate) fn compile_resource_attributes(
    resource_attributes: &SARAttributes,
    variables: &HashMap<&str, String>,
) -> ResourceAttributes {
    let namespace = resolve(&resource_attributes.namespace, variables);
    let group = resolve(&resource_attributes.api_group, variables);
    let resource = resolve(&resource_attributes.resource, variables);
    let verb = resolve(&resource_attributes.verb, variables);

    ResourceAttributes {
        namespace: namespace,
        group: group,
        resource: resource,
        verb: verb,
        ..Default::default()
    }
}

pub(crate) fn extract_variables<'a>(
    extractors: &'a [Extractor],
    req_path: &str,
    header_fn: impl Fn(&str) -> Option<String>,
    query_fn: impl Fn(&str) -> Option<String>,
) -> HashMap<&'a str, String> {
    let mut variables = HashMap::new();
    for extractor in extractors {
        match extractor {
            Extractor::Header { name, header } => {
                if let Some(value) = header_fn(header) {
                    variables.insert(name.as_str(), value);
                }
            }
            Extractor::Path { path } => {
                let parts: Vec<&str> = req_path.split('/').filter(|s| !s.is_empty()).collect();
                for (segment, pattern) in parts.iter().zip(path.iter()) {
                    if let Some(var_name) =
                        pattern.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
                    {
                        variables.insert(var_name, segment.to_string());
                    }
                }
            }
            Extractor::Query { name, parameter } => {
                if let Some(value) = query_fn(parameter) {
                    variables.insert(name.as_str(), value);
                }
            }
        }
    }
    variables
}

pub fn match_path(path: &str, path_pattern: Vec<String>) -> Option<HashMap<String, String>> {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    if parts.len() != path_pattern.len() {
        return None;
    }

    let mut variables = HashMap::new();

    for (part, pattern) in parts.iter().zip(path_pattern.iter()) {
        if pattern == "*" {
            continue;
        }
        if let Some(var_name) = pattern.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            variables.insert(var_name.to_string(), part.to_string());
        } else if pattern != part {
            return None;
        }
    }

    Some(variables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Binding, SARAttributes};

    fn vars<'a>(entries: &[(&'a str, &str)]) -> HashMap<&'a str, String> {
        entries
            .iter()
            .map(|(k, v)| (*k, v.to_string()))
            .collect()
    }

    // -- compile_resource_attributes tests --

    #[test]
    fn compile_resolves_variable_in_namespace() {
        let attrs = SARAttributes {
            namespace: Binding::Variable("tenant".into()),
            api_group: Binding::Literal("example.io".into()),
            resource: Binding::Literal("widgets".into()),
            verb: Binding::Literal("get".into()),
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
            namespace: Binding::Literal("fixed-ns".into()),
            api_group: Binding::Literal("example.io".into()),
            resource: Binding::Literal("widgets".into()),
            verb: Binding::Literal("list".into()),
        };
        let result = compile_resource_attributes(&attrs, &vars(&[]));
        assert_eq!(result.namespace.unwrap(), "fixed-ns");
    }

    #[test]
    fn compile_returns_none_when_variable_missing() {
        let attrs = SARAttributes {
            namespace: Binding::Variable("unknown".into()),
            api_group: Binding::Literal("example.io".into()),
            resource: Binding::Literal("widgets".into()),
            verb: Binding::Literal("get".into()),
        };
        let result = compile_resource_attributes(&attrs, &vars(&[]));
        assert_eq!(result.namespace, None);
    }

    #[test]
    fn compile_resolves_variables_in_all_fields() {
        let attrs = SARAttributes {
            namespace: Binding::Variable("ns".into()),
            api_group: Binding::Variable("group".into()),
            resource: Binding::Variable("res".into()),
            verb: Binding::Variable("verb".into()),
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
        assert_eq!(
            resolve(&Binding::Variable("tenant".into()), &variables),
            Some("my-namespace".into())
        );
    }

    #[test]
    fn resolve_returns_literal_as_is() {
        let variables = vars(&[("tenant", "my-namespace")]);
        assert_eq!(
            resolve(&Binding::Literal("literal-value".into()), &variables),
            Some("literal-value".into())
        );
    }

    #[test]
    fn resolve_returns_none_when_variable_not_found() {
        let variables = vars(&[]);
        assert_eq!(
            resolve(&Binding::Variable("missing".into()), &variables),
            None
        );
    }

    #[test]
    fn resolve_literal_ignores_variables() {
        let variables = vars(&[("key", "value")]);
        assert_eq!(
            resolve(&Binding::Literal("key".into()), &variables),
            Some("key".into())
        );
    }

    // -- match_path tests --

    fn owned_vars(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn pat(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn match_path_exact_literal_segments() {
        let result = match_path("/a/b/c", pat(&["a", "b", "c"]));
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn match_path_extracts_variables() {
        let result = match_path(
            "/svc/hello/sub/world",
            pat(&["svc", "{arg1}", "sub", "{arg2}"]),
        );
        assert_eq!(result, Some(owned_vars(&[("arg1", "hello"), ("arg2", "world")])));
    }

    #[test]
    fn match_path_wildcard_matches_any_segment() {
        let result = match_path("/v1/anything/data", pat(&["v1", "*", "data"]));
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn match_path_wildcard_and_variable_combined() {
        let result = match_path("/v1/skip/my-ns/items", pat(&["v1", "*", "{ns}", "items"]));
        assert_eq!(result, Some(owned_vars(&[("ns", "my-ns")])));
    }

    #[test]
    fn match_path_rejects_wrong_literal() {
        let result = match_path("/a/b/c", pat(&["a", "x", "c"]));
        assert_eq!(result, None);
    }

    #[test]
    fn match_path_rejects_too_few_segments() {
        let result = match_path("/a/b", pat(&["a", "b", "c"]));
        assert_eq!(result, None);
    }

    #[test]
    fn match_path_rejects_too_many_segments() {
        let result = match_path("/a/b/c/d", pat(&["a", "b", "c"]));
        assert_eq!(result, None);
    }

    #[test]
    fn match_path_handles_leading_slash() {
        let result = match_path("/x/y", pat(&["x", "y"]));
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn match_path_handles_no_leading_slash() {
        let result = match_path("x/y", pat(&["x", "y"]));
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn match_path_all_variables() {
        let result = match_path("/a/b/c", pat(&["{x}", "{y}", "{z}"]));
        assert_eq!(result, Some(owned_vars(&[("x", "a"), ("y", "b"), ("z", "c")])));
    }

    #[test]
    fn match_path_all_wildcards() {
        let result = match_path("/a/b/c", pat(&["*", "*", "*"]));
        assert_eq!(result, Some(HashMap::new()));
    }

    #[test]
    fn match_path_empty_pattern_rejects_nonempty_path() {
        let result = match_path("/a", pat(&[]));
        assert_eq!(result, None);
    }

    #[test]
    fn match_path_empty_path_matches_empty_pattern() {
        let result = match_path("", pat(&[]));
        assert_eq!(result, Some(HashMap::new()));
    }
}
