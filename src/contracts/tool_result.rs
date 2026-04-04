use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResult {
    pub request_id: String,
    pub tool_name: String,
    pub success: bool,
    pub output_json: String,
    pub error_message: Option<String>,
    pub duration_ms: u128,
}

impl ToolResult {
    pub fn validate(&self) -> Result<(), String> {
        if self.request_id.trim().is_empty() {
            return Err("tool_result.request_id is empty".into());
        }

        if self.tool_name.trim().is_empty() {
            return Err("tool_result.tool_name is empty".into());
        }

        if self.success {
            if self.output_json.trim().is_empty() {
                return Err("tool_result.output_json is empty on success".into());
            }

            if self.error_message.is_some() {
                return Err("tool_result.error_message must be None on success".into());
            }
        } else {
            match &self.error_message {
                Some(msg) if !msg.trim().is_empty() => {},
                _ => {
                    return Err("tool_result.error_message is required on failure".into());
                },
            }
        }

        Ok(())
    }
}
