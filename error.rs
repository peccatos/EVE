use thiserror::Error;

use crate::policy::PolicyReason;

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("contract validation failed: {0}")]
    ContractValidation(String),
    #[error("audit I/O failed for '{path}': {message}")]
    AuditIo { path: String, message: String },
    #[error("audit serialization failed: {message}")]
    AuditSerialization { message: String },
    #[error("memory sector '{sector_id}' is not registered")]
    UnknownMemorySector { sector_id: String },
    #[error("memory access denied: kernel sector '{kernel_sector}' cannot access sector '{requested_sector}'")]
    MemorySectorAccessDenied {
        kernel_sector: String,
        requested_sector: String,
    },
    #[error("memory node limit reached for sector '{sector_id}': {limit}")]
    MemoryNodeLimitReached { sector_id: String, limit: usize },
    #[error("memory edge limit reached for sector '{sector_id}': {limit}")]
    MemoryEdgeLimitReached { sector_id: String, limit: usize },
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool disabled by policy: {0}")]
    ToolDisabled(String),
    #[error("policy denied tool '{tool_name}' with reason '{reason}'")]
    PolicyDenied {
        tool_name: String,
        reason: PolicyReason,
    },
    #[error("tool throttled by policy: {tool_name}")]
    ToolThrottled { tool_name: String },
    #[error("malformed payload for tool '{tool_name}': {message}")]
    MalformedPayload { tool_name: String, message: String },
    #[error("tool execution failed for '{tool_name}': {message}")]
    ToolExecution { tool_name: String, message: String },
}
