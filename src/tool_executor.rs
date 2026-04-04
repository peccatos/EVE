use std::fs;
use std::path::PathBuf;

use crate::contracts::file_exists::{FileExistsRequest, FileExistsResult};
use crate::contracts::list_dir::{ListDirRequest, ListDirResult};
use crate::contracts::read_file::{ReadFileRequest, ReadFileResult};
use crate::contracts::tool_request::ToolRequest;
use crate::error::KernelError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPayload {
    ReadFile(ReadFileRequest),
    ListDir(ListDirRequest),
    FileExists(FileExistsRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutput {
    ReadFile(ReadFileResult),
    ListDir(ListDirResult),
    FileExists(FileExistsResult),
}

impl ToolOutput {
    pub fn to_json_string(&self) -> Result<String, KernelError> {
        match self {
            Self::ReadFile(result) => {
                serde_json::to_string(result).map_err(|err| KernelError::ToolExecution {
                    tool_name: "read_file".into(),
                    message: err.to_string(),
                })
            },
            Self::ListDir(result) => {
                serde_json::to_string(result).map_err(|err| KernelError::ToolExecution {
                    tool_name: "list_dir".into(),
                    message: err.to_string(),
                })
            },
            Self::FileExists(result) => {
                serde_json::to_string(result).map_err(|err| KernelError::ToolExecution {
                    tool_name: "file_exists".into(),
                    message: err.to_string(),
                })
            },
        }
    }
}

pub fn execute_tool(request: &ToolRequest) -> Result<ToolOutput, KernelError> {
    let payload = decode_tool_payload(request)?;

    match payload {
        ToolPayload::ReadFile(payload) => execute_read_file(payload),
        ToolPayload::ListDir(payload) => execute_list_dir(payload),
        ToolPayload::FileExists(payload) => execute_file_exists(payload),
    }
}

pub(crate) fn validate_tool_payload(request: &ToolRequest) -> Result<(), KernelError> {
    decode_tool_payload(request).map(|_| ())
}

pub fn decode_tool_payload(request: &ToolRequest) -> Result<ToolPayload, KernelError> {
    match request.tool_name.as_str() {
        "read_file" => {
            let payload: ReadFileRequest =
                serde_json::from_str(&request.payload_json).map_err(|err| {
                    KernelError::MalformedPayload {
                        tool_name: request.tool_name.clone(),
                        message: err.to_string(),
                    }
                })?;
            payload
                .validate()
                .map_err(|message| KernelError::MalformedPayload {
                    tool_name: request.tool_name.clone(),
                    message,
                })?;
            Ok(ToolPayload::ReadFile(payload))
        },
        "list_dir" => {
            let payload: ListDirRequest =
                serde_json::from_str(&request.payload_json).map_err(|err| {
                    KernelError::MalformedPayload {
                        tool_name: request.tool_name.clone(),
                        message: err.to_string(),
                    }
                })?;
            payload
                .validate()
                .map_err(|message| KernelError::MalformedPayload {
                    tool_name: request.tool_name.clone(),
                    message,
                })?;
            Ok(ToolPayload::ListDir(payload))
        },
        "file_exists" => {
            let payload: FileExistsRequest =
                serde_json::from_str(&request.payload_json).map_err(|err| {
                    KernelError::MalformedPayload {
                        tool_name: request.tool_name.clone(),
                        message: err.to_string(),
                    }
                })?;
            payload
                .validate()
                .map_err(|message| KernelError::MalformedPayload {
                    tool_name: request.tool_name.clone(),
                    message,
                })?;
            Ok(ToolPayload::FileExists(payload))
        },
        other => Err(KernelError::UnknownTool(other.to_string())),
    }
}

fn execute_read_file(payload: ReadFileRequest) -> Result<ToolOutput, KernelError> {
    let contents = fs::read_to_string(&payload.path).map_err(|err| KernelError::ToolExecution {
        tool_name: "read_file".into(),
        message: err.to_string(),
    })?;

    Ok(ToolOutput::ReadFile(ReadFileResult {
        path: payload.path,
        contents,
    }))
}

fn execute_list_dir(payload: ListDirRequest) -> Result<ToolOutput, KernelError> {
    let mut entries = fs::read_dir(&payload.path)
        .map_err(|err| KernelError::ToolExecution {
            tool_name: "list_dir".into(),
            message: err.to_string(),
        })?
        .map(|entry| {
            entry
                .map_err(|err| KernelError::ToolExecution {
                    tool_name: "list_dir".into(),
                    message: err.to_string(),
                })
                .map(|item| file_name_string(item.path()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    entries.sort();

    Ok(ToolOutput::ListDir(ListDirResult {
        path: payload.path,
        entries,
    }))
}

fn execute_file_exists(payload: FileExistsRequest) -> Result<ToolOutput, KernelError> {
    Ok(ToolOutput::FileExists(FileExistsResult {
        path: payload.path.clone(),
        exists: PathBuf::from(payload.path).exists(),
    }))
}

fn file_name_string(path: PathBuf) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
