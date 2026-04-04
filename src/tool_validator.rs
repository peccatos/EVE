use crate::contracts::tool_request::ToolRequest;
use crate::error::KernelError;

pub fn validate_tool_request(request: &ToolRequest) -> Result<(), KernelError> {
    request.validate().map_err(KernelError::ContractValidation)
}
