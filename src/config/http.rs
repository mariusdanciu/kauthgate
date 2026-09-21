use crate::config::defs::{Binding, Entity, SARAttributes, Upstream};
use serde::Deserialize;
use serde::Deserializer;

fn deserialize_path_segments<'de, D>(deserializer: D) -> std::result::Result<Option<Vec<Binding>>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let segments = s
        .split('/')
        .filter(|seg| !seg.is_empty())
        .map(Binding::from_str)
        .collect();
    Ok(Some(segments))
}

fn deserialize_methods<'de, D>(deserializer: D) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let methods: Vec<String> = Vec::deserialize(deserializer)?;
    Ok(Some(methods.into_iter().map(|m| m.to_lowercase()).collect()))
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestMatch {
    #[serde(default, deserialize_with = "deserialize_path_segments")]
    pub path: Option<Vec<Binding>>,
    #[serde(default, deserialize_with = "deserialize_methods")]
    pub methods: Option<Vec<String>>,
    #[serde(default)]
    pub headers: Option<Vec<Entity>>,
    #[serde(rename = "query-params", default)]
    pub query_params: Option<Vec<Entity>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub name: String,
    pub request: RequestMatch,
    #[serde(rename = "sar-resource-attributes")]
    pub sar_resource_attributes: SARAttributes,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    pub upstream: Upstream,
    pub rules: Vec<Rule>,
}
