use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffRequest {
    pub handoff_id: String,
    pub request_id: String,
    pub tool_name: String,
    pub source_sector: String,
    pub target_sector: String,
    pub payload_json: String,
    pub dry_run: bool,
    pub timeout_ms: Option<u64>,
}

impl HandoffRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.handoff_id.trim().is_empty() {
            return Err("handoff_request.handoff_id is empty".into());
        }

        if self.request_id.trim().is_empty() {
            return Err("handoff_request.request_id is empty".into());
        }

        if self.tool_name.trim().is_empty() {
            return Err("handoff_request.tool_name is empty".into());
        }

        if self.source_sector.trim().is_empty() {
            return Err("handoff_request.source_sector is empty".into());
        }

        if self.target_sector.trim().is_empty() {
            return Err("handoff_request.target_sector is empty".into());
        }

        if self.payload_json.trim().is_empty() {
            return Err("handoff_request.payload_json is empty".into());
        }

        if self.source_sector == self.target_sector {
            return Err("handoff_request.source_sector must differ from target_sector".into());
        }

        if matches!(self.timeout_ms, Some(0)) {
            return Err("handoff_request.timeout_ms must be > 0 when provided".into());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffReceipt {
    pub handoff_id: String,
    pub request_id: String,
    pub accepted: bool,
    pub source_sector: String,
    pub target_sector: String,
    pub reason: String,
}
