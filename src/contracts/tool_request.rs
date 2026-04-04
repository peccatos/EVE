use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRequest {
    pub request_id: String,
    pub tool_name: String,
    pub payload_json: String,
    pub dry_run: bool,
    pub timeout_ms: Option<u64>,
}

impl ToolRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.request_id.trim().is_empty() {
            return Err("tool_request.request_id is empty".into());
        }

        if self.tool_name.trim().is_empty() {
            return Err("tool_request.tool_name is empty".into());
        }

        if self.payload_json.trim().is_empty() {
            return Err("tool_request.payload_json is empty".into());
        }

        if matches!(self.timeout_ms, Some(0)) {
            return Err("tool_request.timeout_ms must be > 0 when provided".into());
        }

        Ok(())
    }
}
