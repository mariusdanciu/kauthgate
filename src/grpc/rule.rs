use crate::config::grpc::RequestMatch;
use crate::utils::ConfigVariables;
use crate::utils::matchers::match_entity;
use std::collections::HashMap;

pub(crate) fn check_rule(
    matches: &RequestMatch,
    service: &str,
    grpc_method: &str,
    get_header: impl Fn(&str) -> Option<String>,
) -> Option<ConfigVariables> {
    let mut variables = HashMap::with_capacity(10);

    if let Some(svc) = &matches.service
        && svc != service
    {
        return None;
    }

    if let Some(methods) = &matches.grpc_methods
        && !methods.iter().any(|m| m == grpc_method)
    {
        return None;
    }

    match match_entity(&matches.headers, get_header) {
        Some(vars) => variables.extend(vars),
        None => return None,
    }

    variables.insert("service".into(), service.to_string());
    variables.insert("grpc_method".into(), grpc_method.to_string());
    Some(variables)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::defs::Binding;
    use crate::config::defs::EntityMatch;

    fn request(service: &str, actions: &[&str], headers: Vec<EntityMatch>) -> RequestMatch {
        RequestMatch {
            service: Some(service.into()),
            grpc_methods: Some(actions.iter().map(|s| s.to_string()).collect()),
            headers: if headers.is_empty() { None } else { Some(headers) },
        }
    }

    fn var_header(name: &str, var: &str) -> EntityMatch {
        EntityMatch::EqualsOrExtract {
            name: name.into(),
            value: Binding::Variable(var.into()),
        }
    }

    fn literal_header(name: &str, literal: &str) -> EntityMatch {
        EntityMatch::EqualsOrExtract {
            name: name.into(),
            value: Binding::Literal(literal.into()),
        }
    }

    fn exists_header(name: &str) -> EntityMatch {
        EntityMatch::Exists { name: name.into() }
    }

    fn no_header(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn match_rule_matches_service_and_action() {
        let r = request("my.Service", &["GetItem", "ListItems"], vec![]);
        assert!(check_rule(&r, "my.Service", "GetItem", no_header).is_some());
        assert!(check_rule(&r, "my.Service", "ListItems", no_header).is_some());
    }

    #[test]
    fn match_rule_rejects_wrong_service() {
        let r = request("my.Service", &["GetItem"], vec![]);
        assert!(check_rule(&r, "other.Service", "GetItem", no_header).is_none());
    }

    #[test]
    fn match_rule_rejects_wrong_action() {
        let r = request("my.Service", &["GetItem"], vec![]);
        assert!(check_rule(&r, "my.Service", "DeleteItem", no_header).is_none());
    }

    #[test]
    fn match_rule_requires_all_headers() {
        let r = request(
            "my.Service",
            &["GetItem"],
            vec![
                var_header("x-tenant-id", "tenant"),
                var_header("x-request-id", "req_id"),
            ],
        );
        let get = |h: &str| match h {
            "x-tenant-id" => Some("t1".into()),
            "x-request-id" => Some("r1".into()),
            _ => None,
        };
        let result = check_rule(&r, "my.Service", "GetItem", get);
        assert!(result.is_some());
        let vars = result.unwrap();
        assert_eq!(vars.get("tenant").unwrap(), "t1");
        assert_eq!(vars.get("req_id").unwrap(), "r1");
    }

    #[test]
    fn match_rule_rejects_missing_header() {
        let r = request(
            "my.Service",
            &["GetItem"],
            vec![
                var_header("x-tenant-id", "tenant"),
                var_header("x-request-id", "req_id"),
            ],
        );
        let get = |h: &str| match h {
            "x-tenant-id" => Some("t1".into()),
            _ => None,
        };
        assert!(check_rule(&r, "my.Service", "GetItem", get).is_none());
    }

    #[test]
    fn match_rule_no_headers_always_passes() {
        let r = request("my.Service", &["GetItem"], vec![]);
        assert!(check_rule(&r, "my.Service", "GetItem", no_header).is_some());
    }

    #[test]
    fn match_rule_literal_header_rejects_wrong_value() {
        let r = request("my.Service", &["GetItem"], vec![literal_header("x-version", "v2")]);
        let get = |h: &str| match h {
            "x-version" => Some("v1".into()),
            _ => None,
        };
        assert!(check_rule(&r, "my.Service", "GetItem", get).is_none());
    }

    #[test]
    fn match_rule_literal_header_accepts_matching_value() {
        let r = request("my.Service", &["GetItem"], vec![literal_header("x-version", "v2")]);
        let get = |h: &str| match h {
            "x-version" => Some("v2".into()),
            _ => None,
        };
        assert!(check_rule(&r, "my.Service", "GetItem", get).is_some());
    }

    #[test]
    fn match_rule_all_conditions_none_matches_everything() {
        let r = RequestMatch {
            service: None,
            grpc_methods: None,
            headers: None,
        };
        assert!(check_rule(&r, "any.Service", "AnyMethod", no_header).is_some());
    }

    #[test]
    fn exists_header_matches_when_present() {
        let r = request("my.Service", &["GetItem"], vec![exists_header("x-trace-id")]);
        let get = |h: &str| match h {
            "x-trace-id" => Some("abc-123".into()),
            _ => None,
        };
        assert!(check_rule(&r, "my.Service", "GetItem", get).is_some());
    }

    #[test]
    fn exists_header_rejects_when_missing() {
        let r = request("my.Service", &["GetItem"], vec![exists_header("x-trace-id")]);
        assert!(check_rule(&r, "my.Service", "GetItem", no_header).is_none());
    }

    #[test]
    fn exists_header_does_not_extract_variable() {
        let r = request("my.Service", &["GetItem"], vec![exists_header("x-trace-id")]);
        let get = |h: &str| match h {
            "x-trace-id" => Some("abc-123".into()),
            _ => None,
        };
        let vars = check_rule(&r, "my.Service", "GetItem", get).unwrap();
        assert!(!vars.contains_key("x-trace-id"));
    }

    #[test]
    fn exists_header_combined_with_extract() {
        let r = request(
            "my.Service",
            &["GetItem"],
            vec![exists_header("x-trace-id"), var_header("x-tenant-id", "tenant")],
        );
        let get = |h: &str| match h {
            "x-trace-id" => Some("abc-123".into()),
            "x-tenant-id" => Some("acme".into()),
            _ => None,
        };
        let vars = check_rule(&r, "my.Service", "GetItem", get).unwrap();
        assert_eq!(vars.get("tenant").unwrap(), "acme");
        assert!(!vars.contains_key("x-trace-id"));
    }

    #[test]
    fn match_rule_injects_service_and_method() {
        let r = request("my.Service", &["GetItem"], vec![]);
        let vars = check_rule(&r, "my.Service", "GetItem", no_header).unwrap();
        assert_eq!(vars.get("service").unwrap(), "my.Service");
        assert_eq!(vars.get("grpc_method").unwrap(), "GetItem");
    }
}
