use crate::config::defs::Binding;
use crate::config::http::RequestMatch;
use crate::utils::ConfigVariables;
use crate::utils::matchers::match_entity;
use crate::utils::proxy::path_to_vec;
use std::collections::HashMap;
use tracing::info;

fn check_path(rule_path: &[Binding], request_path: &[&str]) -> Option<ConfigVariables> {
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
    matches: &RequestMatch,
    path: &str,
    method: &str,
    get_header: impl Fn(&str) -> Option<String>,
    get_query: impl Fn(&str) -> Option<String>,
) -> Option<ConfigVariables> {
    let mut variables = HashMap::with_capacity(10);

    let parts: Vec<&str> = path_to_vec(path);

    if let Some(rule_path) = &matches.path {
        if let Some(p_vars) = check_path(rule_path, &parts) {
            variables.extend(p_vars);
        } else {
            info!("Path does not match rule path {:?} {:?}", path, rule_path);
            return None;
        }
    }

    if let Some(methods) = &matches.methods
        && !methods.iter().any(|m| m.eq_ignore_ascii_case(method))
    {
        return None;
    }

    match match_entity(&matches.headers, get_header) {
        Some(vars) => variables.extend(vars),
        None => return None,
    }

    match match_entity(&matches.query_params, get_query) {
        Some(vars) => variables.extend(vars),
        None => return None,
    }

    variables.insert("path".into(), path.to_string());
    variables.insert("method".into(), method.to_string());
    Some(variables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::defs::Binding;
    use crate::config::defs::EntityMatch;

    fn path_bindings(parts: &[&str]) -> Vec<Binding> {
        parts.iter().map(|s| Binding::from_str(s)).collect()
    }

    fn no_header(_: &str) -> Option<String> {
        None
    }
    fn no_query(_: &str) -> Option<String> {
        None
    }

    fn request(
        path: Option<&str>,
        methods: Option<&[&str]>,
        headers: Option<Vec<EntityMatch>>,
        query_params: Option<Vec<EntityMatch>>,
    ) -> RequestMatch {
        RequestMatch {
            path: path.map(|p| path_bindings(&path_to_vec(p))),
            methods: methods.map(|m| m.iter().map(|s| s.to_string()).collect()),
            headers,
            query_params,
        }
    }

    fn var_entity(name: &str, var: &str) -> EntityMatch {
        EntityMatch::EqualsOrExtract {
            name: name.into(),
            value: Binding::Variable(var.into()),
        }
    }

    fn exists_entity(name: &str) -> EntityMatch {
        EntityMatch::Exists { name: name.into() }
    }

    fn literal_entity(name: &str, literal: &str) -> EntityMatch {
        EntityMatch::EqualsOrExtract {
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
        let result = check_path(
            &path_bindings(&["api", "**"]),
            &path_to_vec("/api/v1/users/123/profile"),
        );
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
        let r = request(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&r, "/api/v1/users", "GET", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_rejects_wrong_method() {
        let r = request(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&r, "/api/v1/users", "POST", no_header, no_query);
        assert!(result.is_none());
    }

    #[test]
    fn rule_rejects_wrong_path() {
        let r = request(Some("/api/v1/users"), Some(&["GET"]), None, None);
        let result = check_rule(&r, "/api/v2/items", "GET", no_header, no_query);
        assert!(result.is_none());
    }

    #[test]
    fn rule_no_methods_accepts_any() {
        let r = request(Some("/api"), None, None, None);
        let result = check_rule(&r, "/api", "DELETE", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_no_path_accepts_any() {
        let r = request(None, Some(&["GET"]), None, None);
        let result = check_rule(&r, "/anything/here", "GET", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_extracts_path_variables() {
        let r = RequestMatch {
            path: Some(path_bindings(&["tenants", "{tid}", "resources"])),
            methods: None,
            headers: None,
            query_params: None,
        };
        let vars = check_rule(&r, "/tenants/acme/resources", "GET", no_header, no_query).unwrap();
        assert_eq!(vars.get("tid").unwrap(), "acme");
    }

    #[test]
    fn rule_requires_header_present() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![var_entity("x-token", "tok")]),
            None,
        );
        assert!(check_rule(&r, "/api", "GET", no_header, no_query).is_none());
        let result = check_rule(
            &r,
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
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![literal_entity("x-version", "v2")]),
            None,
        );
        let result = check_rule(
            &r,
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
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![literal_entity("x-version", "v2")]),
            None,
        );
        let result = check_rule(
            &r,
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
        let r = request(
            Some("/search"),
            Some(&["GET"]),
            None,
            Some(vec![var_entity("q", "query")]),
        );
        assert!(check_rule(&r, "/search", "GET", no_header, no_query).is_none());
        let result = check_rule(&r, "/search", "GET", no_header, |q| {
            if q == "q" { Some("rust".into()) } else { None }
        });
        assert!(result.is_some());
        assert_eq!(result.unwrap().get("query").unwrap(), "rust");
    }

    #[test]
    fn rule_literal_query_rejects_wrong_value() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            None,
            Some(vec![literal_entity("format", "json")]),
        );
        let result = check_rule(&r, "/api", "GET", no_header, |q| {
            if q == "format" { Some("xml".into()) } else { None }
        });
        assert!(result.is_none());
    }

    #[test]
    fn exists_header_matches_when_present() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![exists_entity("x-trace-id")]),
            None,
        );
        let result = check_rule(
            &r,
            "/api",
            "GET",
            |h| if h == "x-trace-id" { Some("abc".into()) } else { None },
            no_query,
        );
        assert!(result.is_some());
    }

    #[test]
    fn exists_header_rejects_when_missing() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![exists_entity("x-trace-id")]),
            None,
        );
        assert!(check_rule(&r, "/api", "GET", no_header, no_query).is_none());
    }

    #[test]
    fn exists_header_does_not_extract_variable() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![exists_entity("x-trace-id")]),
            None,
        );
        let vars = check_rule(
            &r,
            "/api",
            "GET",
            |h| if h == "x-trace-id" { Some("abc".into()) } else { None },
            no_query,
        )
        .unwrap();
        assert!(!vars.contains_key("x-trace-id"));
    }

    #[test]
    fn exists_query_param_matches_when_present() {
        let r = request(Some("/search"), Some(&["GET"]), None, Some(vec![exists_entity("page")]));
        let result = check_rule(&r, "/search", "GET", no_header, |q| {
            if q == "page" { Some("1".into()) } else { None }
        });
        assert!(result.is_some());
    }

    #[test]
    fn exists_query_param_rejects_when_missing() {
        let r = request(Some("/search"), Some(&["GET"]), None, Some(vec![exists_entity("page")]));
        assert!(check_rule(&r, "/search", "GET", no_header, no_query).is_none());
    }

    #[test]
    fn exists_query_param_does_not_extract_variable() {
        let r = request(Some("/search"), Some(&["GET"]), None, Some(vec![exists_entity("page")]));
        let vars = check_rule(&r, "/search", "GET", no_header, |q| {
            if q == "page" { Some("3".into()) } else { None }
        })
        .unwrap();
        assert!(!vars.contains_key("page"));
    }

    #[test]
    fn exists_combined_with_extract_header_and_query() {
        let r = request(
            Some("/api"),
            Some(&["GET"]),
            Some(vec![exists_entity("x-trace-id"), var_entity("x-tenant-id", "tenant")]),
            Some(vec![exists_entity("debug"), var_entity("q", "query")]),
        );
        let vars = check_rule(
            &r,
            "/api",
            "GET",
            |h| match h {
                "x-trace-id" => Some("tr-1".into()),
                "x-tenant-id" => Some("acme".into()),
                _ => None,
            },
            |q| match q {
                "debug" => Some("true".into()),
                "q" => Some("rust".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(vars.get("tenant").unwrap(), "acme");
        assert_eq!(vars.get("query").unwrap(), "rust");
        assert!(!vars.contains_key("x-trace-id"));
        assert!(!vars.contains_key("debug"));
    }

    #[test]
    fn rule_injects_path_and_method() {
        let r = request(Some("/api"), Some(&["POST"]), None, None);
        let vars = check_rule(&r, "/api", "POST", no_header, no_query).unwrap();
        assert_eq!(vars.get("path").unwrap(), "/api");
        assert_eq!(vars.get("method").unwrap(), "POST");
    }

    #[test]
    fn rule_all_conditions_none_matches_everything() {
        let r = request(None, None, None, None);
        let result = check_rule(&r, "/any/path", "PATCH", no_header, no_query);
        assert!(result.is_some());
    }

    #[test]
    fn rule_combines_path_and_header_variables() {
        let r = RequestMatch {
            path: Some(path_bindings(&["tenants", "{tid}", "data"])),
            methods: Some(vec!["POST".into()]),
            headers: Some(vec![var_entity("x-request-id", "rid")]),
            query_params: None,
        };
        let vars = check_rule(
            &r,
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
