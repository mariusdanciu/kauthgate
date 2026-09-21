use pingora::proxy::Session;
use crate::config::Binding;
use crate::config::SARAttributes;
use k8s_openapi::api::authorization::v1::ResourceAttributes;
use std::collections::HashMap;


pub(crate) fn get_header(session: &Session, header: &str) -> Option<String> {
    session
        .req_header()
        .headers
        .get(header)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string())
}

fn resolve(value: &Binding, variables: &HashMap<String, String>) -> Option<String> {
    match value {
        Binding::Variable(name) => variables.get(name.as_str()).map(|s| s.to_string()),
        Binding::Literal(value) => Some(value.to_string()),
    }
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
        namespace: namespace,
        group: group,
        resource: resource,
        verb: verb,
        ..Default::default()
    }
}