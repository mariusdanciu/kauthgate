use crate::config::defs::Binding;
use crate::config::http::Rule;
use crate::utils::proxy::path_to_vec;
use std::collections::HashMap;
use tracing::info;

fn check_path(rule_path: &[Binding], request_path: &[&str]) -> Option<HashMap<String, String>> {
    let mut variables = HashMap::with_capacity(10);
    let m_len = rule_path.len();
    let r_len = request_path.len();

    let last_is_any = rule_path[m_len - 1] == Binding::Literal("**".to_string())
        || rule_path[m_len - 1] == Binding::Variable("*".to_string());

    if m_len == r_len + 1 && !last_is_any {
        return None;
    }
    if m_len > r_len + 1 {
        return None;
    }

    for (i, segment) in rule_path.iter().enumerate() {
        match segment {
            Binding::Variable(var) => {
                variables.insert(var.to_string(), request_path[i].to_string());
            },
            Binding::Literal(literal) => {
                if literal == "*" {
                    continue;
                } else if literal == "**" {
                    return Some(variables);
                }
                if request_path[i] != *literal {
                    return None;
                }
            },
        }
    }
    Some(variables)
}

pub(crate) fn check_rule(
    policy: &Rule,
    path: &str,
    method: &str,
    get_header: impl Fn(&str) -> Option<String>,
    get_query: impl Fn(&str) -> Option<String>,
) -> Option<HashMap<String, String>> {
    let mut variables = HashMap::with_capacity(10);

    let parts: Vec<&str> = path_to_vec(path);

    if let Some(rule_path) = &policy.request.path {
        if let Some(p_vars) = check_path(rule_path, &parts) {
            variables.extend(p_vars);
        } else {
            info!("Path does not match rule path {:?} {:?}", path, rule_path);
            return None;
        }
    }

    if let Some(methods) = &policy.request.methods
        && !methods.iter().any(|m| m.eq_ignore_ascii_case(method))
    {
        return None;
    }

    if let Some(headers) = &policy.request.headers {
        for header in headers {
            let value = get_header(header.name.as_str());

            if let Some(value) = value {
                match &header.value {
                    Binding::Variable(var) => {
                        variables.insert(var.to_string(), value);
                    },
                    Binding::Literal(literal) => {
                        if value != *literal {
                            return None;
                        }
                    },
                }
            } else {
                return None;
            }
        }
    }

    if let Some(query_params) = &policy.request.query_params {
        for query in query_params {
            let value = get_query(query.name.as_str());

            if let Some(value) = value {
                match &query.value {
                    Binding::Variable(var) => {
                        variables.insert(var.to_string(), value);
                    },
                    Binding::Literal(literal) => {
                        if value != *literal {
                            return None;
                        }
                    },
                }
            } else {
                return None;
            }
        }
    }

    variables.insert("path".into(), path.to_string());
    variables.insert("method".into(), method.to_string());
    Some(variables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defs::{Binding, Entity, SARAttributes};
    use crate::config::http::{Rule, RequestMatch};

    fn path_bindings(parts: &[&str]) -> Vec<Binding> {
        parts.iter().map(|s| Binding::from_str(s)).collect()
    }

    fn no_header(_: &str) -> Option<String> {
        None
    }
    fn no_query(_: &str) -> Option<String> {
        None
    }

    fn rule(
        path: Option<&str>,
        methods: Option<&[&str]>,
        headers: Option<Vec<Entity>>,
        query_params: Option<Vec<Entity>>,
    ) -> Rule {
        Rule {
            name: "test".into(),
            request: RequestMatch {
                path: path.map(|p| path_bindings(&path_to_vec(p))),
                methods: methods.map(|m| m.iter().map(|s| s.to_string()).collect()),
                headers,
                query_params,
            },
            sar_resource_attributes: SARAttributes {
                namespace: Binding::Literal("ns".into()),
                api_group: Binding::Literal("g".into()),
                resource: Binding::Literal("r".into()),
                verb: Binding::Literal("v".into()),
            },
        }
    }

    fn var_entity(name: &str, var: &str) -> Entity {
        Entity {
            name: name.into(),
            value: Binding::Variable(var.into()),
        }
    }

    fn literal_entity(name: &str, literal: &str) -> Entity {
        Entity {
            name: name.into(),
            value: Binding::Literal(literal.into()),
        }
    }

    // -- check_path tests --

    #[test]
    fn path_exact_match() {
        let result = check_path(&path_bindings(&["api", "v1", "users"]), &path_to_vec("/api/v1/users"));
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn path_mismatch() {
        let result = check_path(&path_bindings(&["api", "v2", "users"]), &path_to_vec("/api/v1/users"));
        assert!(result.is_none());
    }

    #[test]
    fn path_variable_extraction() {
        let result = check_path(
            &path_bindings(&["api", "{version}", "tenants", "{tid}"]),
            &path_to_vec("/api/v1/tenants/acme"),
        );
        let vars = result.unwrap();
        assert_eq!(vars.get("version").unwrap(), "v1");
        assert_eq!(vars.get("tid").unwrap(), "acme");
    }

    #[test]
    fn path_wildcard_matches_any_segment() {
        let result = check_path(&path_bindings(&["api", "*", "users"]), &path_to_vec("/api/v1/users"));
        assert!(result.is_some());
    }

    #[test]
    fn path_wildcard_does_not_match_different_suffix() {
        let result = check_path(&path_bindings(&["api", "*", "users"]), &path_to_vec("/api/v1/items"));
        assert!(result.is_none());
    }

    #[test]
    fn path_globstar_matches_everything_after() {
        let result = check_path(&path_bindings(&["api", "**"]), &path_to_vec("/api/v1/users/123/profile"));
        assert!(result.is_some());
    }

    #[test]
    fn path_shorter_request_rejected() {
        let result = check_path(&path_bindings(&["api", "v1", "users"]), &path_to_vec("/api"));
        assert!(result.is_none());
    }

    #[test]
    fn path_longer_request_accepted_when_pattern_shorter() {
        let result = check_path(&path_bindings(&["api", "v1"]), &path_to_vec("/api/v1/users/extra"));
        assert!(result.is_some());
    }

    // -- check_rule tests --

    #[test]
    fn rule_matches_path_and_method() {
        let m = rule(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&m, "/api/v1/users", "GET", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_rejects_wrong_method() {
        let m = rule(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&m, "/api/v1/users", "POST", no_header, no_query);
        assert!(result.is_none());
    }

    #[test]
    fn rule_rejects_wrong_path() {
        let m = rule(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&m, "/api/v2/items", "GET", no_header, no_query);
        assert!(result.is_none());
    }

    #[test]
    fn rule_no_methods_accepts_any() {
        let m = rule(Some("/api"), None, None, None);
        let result = check_rule(&m, "/api", "DELETE", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_no_path_accepts_any() {
        let m = rule(None, Some(&["GET"]), None, None);
        let result = check_rule(&m, "/anything/here", "GET", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_extracts_path_variables() {
        let m = Rule {
            name: "test".into(),
            request: RequestMatch {
                path: Some(path_bindings(&["tenants", "{tid}", "resources"])),
                methods: None,
                headers: None,
                query_params: None,
            },
            sar_resource_attributes: SARAttributes {
                namespace: Binding::Variable("tid".into()),
                api_group: Binding::Literal("g".into()),
                resource: Binding::Literal("r".into()),
                verb: Binding::Literal("v".into()),
            },
        };
        let vars = check_rule(&m, "/tenants/acme/resources", "GET", no_header, no_query).unwrap();
        assert_eq!(vars.get("tid").unwrap(), "acme");
    }

    #[test]
    fn rule_requires_header_present() {
        let m = rule(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![var_entity("x-token", "tok")]),
            None,
        );
        assert!(check_rule(&m, "/api", "GET", no_header, no_query).is_none());
        let result = check_rule(
            &m,
            "/api",
            "GET",
            |h| {
                if h == "x-token" { Some("abc".into()) } else { None }
            },
            no_query,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("tok").unwrap(), "abc");
    }

    #[test]
    fn rule_literal_header_rejects_wrong_value() {
        let m = rule(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![literal_entity("x-version", "v2")]),
            None,
        );
        let result = check_rule(
            &m,
            "/api",
            "GET",
            |h| {
                if h == "x-version" { Some("v1".into()) } else { None }
            },
            no_query,
        );
        assert!(result.is_none());
    }

    #[test]
    fn rule_literal_header_accepts_matching_value() {
        let m = rule(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![literal_entity("x-version", "v2")]),
            None,
        );
        let result = check_rule(
            &m,
            "/api",
            "GET",
            |h| {
                if h == "x-version" { Some("v2".into()) } else { None }
            },
            no_query,
        );
        assert!(result.is_some());
    }

    #[test]
    fn rule_requires_query_param() {
        let m = rule(
            Some("/search"),
            Some(&["GET"]),
            None,
            Some(vec![var_entity("q", "query")]),
        );
        assert!(check_rule(&m, "/search", "GET", no_header, no_query).is_none());
        let result = check_rule(&m, "/search", "GET", no_header, |q| {
            if q == "q" { Some("rust".into()) } else { None }
        });
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("query").unwrap(), "rust");
    }

    #[test]
    fn rule_literal_query_rejects_wrong_value() {
        let m = rule(
            Some("/api"),
            Some(&["GET"]),
            None,
            Some(vec![literal_entity("format", "json")]),
        );
        let result = check_rule(&m, "/api", "GET", no_header, |q| {
            if q == "format" { Some("xml".into()) } else { None }
        });
        assert!(result.is_none());
    }

    #[test]
    fn rule_injects_path_and_method() {
        let m = rule(Some("/api"), Some(&["POST"]), None, None);
        let vars = check_rule(&m, "/api", "POST", no_header, no_query).unwrap();
        assert_eq!(vars.get("path").unwrap(), "/api");
        assert_eq!(vars.get("method").unwrap(), "POST");
    }

    #[test]
    fn rule_all_conditions_none_matches_everything() {
        let m = rule(None, None, None, None);
        let result = check_rule(&m, "/any/path", "PATCH", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_combines_path_and_header_variables() {
        let m = Rule {
            name: "test".into(),
            request: RequestMatch {
                path: Some(path_bindings(&["tenants", "{tid}", "data"])),
                methods: Some(vec!["POST".into()]),
                headers: Some(vec![var_entity("x-request-id", "rid")]),
                query_params: None,
            },
            sar_resource_attributes: SARAttributes {
                namespace: Binding::Variable("tid".into()),
                api_group: Binding::Literal("g".into()),
                resource: Binding::Literal("r".into()),
                verb: Binding::Literal("v".into()),
            },
        };
        let vars = check_rule(
            &m,
            "/tenants/acme/data",
            "POST",
            |h| {
                if h == "x-request-id" {
                    Some("req-123".into())
                } else {
                    None
                }
            },
            no_query,
        )
        .unwrap();
        assert_eq!(vars.get("tid").unwrap(), "acme");
        assert_eq!(vars.get("rid").unwrap(), "req-123");
    }
}
