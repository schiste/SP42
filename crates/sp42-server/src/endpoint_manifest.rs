use crate::{ACTION_HISTORY_PATH, ACTION_STATUS_PATH, OPERATOR_REPORT_PATH};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OperatorEndpointDescriptor {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) purpose: String,
    pub(crate) available: bool,
}

pub(crate) fn operator_endpoint_manifest(default_wiki_id: &str) -> Vec<OperatorEndpointDescriptor> {
    let mut endpoints = operator_core_endpoints();
    endpoints.extend(operator_storage_endpoints());
    endpoints.extend(operator_dev_endpoints(default_wiki_id));
    endpoints
}

fn operator_core_endpoints() -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/healthz".to_string(),
            purpose: "Minimal health indicator for probes and process supervisors.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/debug/summary".to_string(),
            purpose: "Shared auth, capability, and coordination summary.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/debug/runtime".to_string(),
            purpose: "Runtime-oriented operator state with cache and room counts.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: crate::OPERATOR_READINESS_PATH.to_string(),
            purpose: "Consolidated operator readiness report.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: OPERATOR_REPORT_PATH.to_string(),
            purpose: "Full operator report with debug summary and endpoint manifest.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/live/{wiki_id}".to_string(),
            purpose: "Authoritative live patrol queue, selected review details, backend auth status, and shell state.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/runtime/{wiki_id}".to_string(),
            purpose: "Persistent backlog and stream checkpoint inspection for the selected wiki.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: format!("{}/{{wiki_id}}", crate::OPERATOR_STORAGE_LAYOUT_PATH),
            purpose: "Canonical personal/shared on-wiki storage layout and sample page renderings.".to_string(),
            available: true,
        },
    ]
}

fn operator_storage_endpoints() -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/document/{wiki_id}?title=...".to_string(),
            purpose: "Load a canonical public SP42 on-wiki document and parse its machine payload."
                .to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/document/{wiki_id}".to_string(),
            purpose: "Save a canonical public SP42 on-wiki document with conflict-aware writes."
                .to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/logical/{wiki_id}/{realm}/{kind}".to_string(),
            purpose: "Resolve a canonical SP42 public document by logical kind and load its current on-wiki content.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/logical/{wiki_id}/{realm}/{kind}".to_string(),
            purpose: "Save a canonical SP42 public document by logical kind without exposing raw wiki titles to clients.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/operator/storage/public/{wiki_id}/{kind}".to_string(),
            purpose: "Load a typed public SP42 document like preferences, registry, team, rules, or audit ledger.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "PUT".to_string(),
            path: "/operator/storage/public/{wiki_id}/{kind}".to_string(),
            purpose: "Save a typed public SP42 document while keeping durable state on canonical wiki pages.".to_string(),
            available: true,
        },
    ]
}

fn operator_dev_endpoints(default_wiki_id: &str) -> Vec<OperatorEndpointDescriptor> {
    vec![
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/dev/auth/bootstrap/status".to_string(),
            purpose: "Authoritative local token bootstrap and source-report status.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: format!("/dev/auth/capabilities/{default_wiki_id}"),
            purpose: "Capability probe for the configured default wiki.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: ACTION_STATUS_PATH.to_string(),
            purpose: "Current shell feedback and latest action result.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: ACTION_HISTORY_PATH.to_string(),
            purpose: "Recent local action execution history.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/coordination/rooms".to_string(),
            purpose: "Coordination room inventory and summaries.".to_string(),
            available: true,
        },
        OperatorEndpointDescriptor {
            method: "GET".to_string(),
            path: "/coordination/inspections".to_string(),
            purpose: "Room-by-room coordination inspection collection.".to_string(),
            available: true,
        },
    ]
}
