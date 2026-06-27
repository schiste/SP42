//! Shared server debug response contract.

use serde::{Deserialize, Serialize};
use sp42_coordination::CoordinationSnapshot;
use sp42_platform::{DevAuthCapabilityReport, DevAuthSessionStatus, LocalOAuthConfigStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerDebugSummary {
    pub project: String,
    pub auth: DevAuthSessionStatus,
    pub oauth: LocalOAuthConfigStatus,
    pub capabilities: DevAuthCapabilityReport,
    pub coordination: CoordinationSnapshot,
}
