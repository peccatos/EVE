use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::KernelError;

pub const AUDIT_EVENT_SCHEMA_VERSION: u16 = 1;
const TOOL_CALL_EVENT_TYPE: &str = "tool_call";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEventInput {
    pub request_id: String,
    pub tool_name: String,
    pub dry_run: bool,
    pub policy_decision: String,
    pub policy_reason: String,
    pub success: bool,
    pub duration_ms: u128,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditEvent {
    pub schema_version: u16,
    pub sequence: u64,
    pub event_type: String,
    pub request_id: String,
    pub tool_name: String,
    pub dry_run: bool,
    pub policy_decision: String,
    pub policy_reason: String,
    pub success: bool,
    pub duration_ms: u128,
    pub error_message: Option<String>,
}

#[derive(Debug, Default)]
struct AuditState {
    events: Vec<AuditEvent>,
    next_sequence: u64,
}

#[derive(Debug, Default)]
pub struct AuditLog {
    state: Mutex<AuditState>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_tool_call(&self, input: AuditEventInput) -> AuditEvent {
        let mut state = self.state.lock().expect("audit log mutex poisoned");
        let event = AuditEvent {
            schema_version: AUDIT_EVENT_SCHEMA_VERSION,
            sequence: state.next_sequence,
            event_type: TOOL_CALL_EVENT_TYPE.into(),
            request_id: input.request_id,
            tool_name: input.tool_name,
            dry_run: input.dry_run,
            policy_decision: input.policy_decision,
            policy_reason: input.policy_reason,
            success: input.success,
            duration_ms: input.duration_ms,
            error_message: input.error_message,
        };

        state.next_sequence += 1;
        state.events.push(event.clone());
        event
    }

    pub fn events(&self) -> Vec<AuditEvent> {
        self.state
            .lock()
            .expect("audit log mutex poisoned")
            .events
            .clone()
    }

    pub fn persist_jsonl(&self, path: impl AsRef<Path>) -> Result<(), KernelError> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|candidate| !candidate.as_os_str().is_empty());

        if let Some(parent) = parent {
            fs::create_dir_all(parent).map_err(|source| KernelError::AuditIo {
                path: path.display().to_string(),
                message: source.to_string(),
            })?;
        }

        let events = self.events();
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map_err(|source| KernelError::AuditIo {
                path: path.display().to_string(),
                message: source.to_string(),
            })?;

        for event in events {
            let line = serde_json::to_string(&event).map_err(|source| {
                KernelError::AuditSerialization {
                    message: source.to_string(),
                }
            })?;
            writeln!(file, "{line}").map_err(|source| KernelError::AuditIo {
                path: path.display().to_string(),
                message: source.to_string(),
            })?;
        }

        Ok(())
    }

    pub fn load_jsonl(path: impl AsRef<Path>) -> Result<Self, KernelError> {
        let path = path.as_ref();
        let file =
            OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|source| KernelError::AuditIo {
                    path: path.display().to_string(),
                    message: source.to_string(),
                })?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut next_sequence = 0;

        for line in reader.lines() {
            let line = line.map_err(|source| KernelError::AuditIo {
                path: path.display().to_string(),
                message: source.to_string(),
            })?;

            if line.trim().is_empty() {
                continue;
            }

            let event: AuditEvent =
                serde_json::from_str(&line).map_err(|source| KernelError::AuditSerialization {
                    message: source.to_string(),
                })?;

            next_sequence = next_sequence.max(event.sequence.saturating_add(1));
            events.push(event);
        }

        Ok(Self {
            state: Mutex::new(AuditState {
                events,
                next_sequence,
            }),
        })
    }
}
