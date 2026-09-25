use crate::config::defs::Binding;
use crate::config::defs::EntityMatch;
use crate::utils::ConfigVariables;
use std::collections::HashMap;

pub(crate) fn match_entity(
    headers: &Option<Vec<EntityMatch>>,
    get_fn: impl Fn(&str) -> Option<String>,
) -> Option<ConfigVariables> {
    let mut variables = HashMap::with_capacity(10);

    if let Some(headers) = headers {
        for e_match in headers {
            match e_match {
                EntityMatch::EqualsOrExtract { name, value } => {
                    let h = get_fn(name.as_str());
                    if let Some(h) = h {
                        match &value {
                            Binding::Variable(var) => {
                                variables.insert(var.to_string(), h);
                            },
                            Binding::Literal(literal) => {
                                if h != *literal {
                                    return None;
                                }
                            },
                        }
                    } else {
                        return None;
                    }
                },
                EntityMatch::Exists { name } => {
                    get_fn(name.as_str())?;
                },
            }
        }
    }
    Some(variables)
}
