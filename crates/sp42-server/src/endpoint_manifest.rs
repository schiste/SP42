use sp42_core::routes as route_contracts;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OperatorEndpointDescriptor {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) purpose: String,
    pub(crate) available: bool,
}

pub(crate) fn operator_endpoint_manifest(default_wiki_id: &str) -> Vec<OperatorEndpointDescriptor> {
    route_contracts::operator_endpoint_routes(default_wiki_id)
        .into_iter()
        .map(|route| OperatorEndpointDescriptor {
            method: route.method.as_str().to_string(),
            path: route.path,
            purpose: route.purpose.to_string(),
            available: route.available,
        })
        .collect()
}
