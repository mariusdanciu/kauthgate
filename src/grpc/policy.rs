use crate::config::defs::Binding;
use crate::config::grpc::RequestMatch;
use crate::utils::ConfigVariables;
use std::collections::HashMap;

pub(crate) fn check_rule(
    request: &RequestMatch,
    service: &str,
    grpc_method: &str,
    get_header: impl Fn(&str) -> Option<String>,
) -> Option<ConfigVariables> {
    let mut variables = HashMap::with_capacity(10);

    if let Some(svc) = &request.service
        && svc != service
    {
        return None;
    }

    if let Some(methods) = &request.grpc_methods
        && !methods.iter().any(|m| m == grpc_method)
    {
        return None;
    }

    if let Some(headers) = &request.headers {
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
    variables.insert("service".into(), service.to_string());
    variables.insert("grpc_method".into(), grpc_method.to_string());
    Some(variables)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::defs::{Binding, Entity};

    fn request(service: &str, actions: &[&str], headers: Vec<Entity>) -> RequestMatch {
        RequestMatch {
            service: Some(service.into()),
            grpc_methods: Some(actions.iter().map(|s| s.to_string()).collect()),
            headers: if headers.is_empty() { None } else { Some(headers) },
        }
    }

    fn var_header(name: &str, var: &str) -> Entity {
        Entity {
            name: name.into(),
            value: Binding::Variable(var.into()),
        }
    }

    fn literal_header(name: &str, literal: &str) -> Entity {
        Entity {
            name: name.into(),
            value: Binding::Literal(literal.into()),
        }
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
    fn match_rule_injects_service_and_method() {
        let r = request("my.Service", &["GetItem"], vec![]);
        let vars = check_rule(&r, "my.Service", "GetItem", no_header).unwrap();
        assert_eq!(vars.get("service").unwrap(), "my.Service");
        assert_eq!(vars.get("grpc_method").unwrap(), "GetItem");
    }
}
