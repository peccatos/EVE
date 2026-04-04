use std::time::Instant;

use serde_json::json;

use crate::audit::AuditEventInput;
use crate::contracts::tool_request::ToolRequest;
use crate::contracts::tool_result::ToolResult;
use crate::error::KernelError;
use crate::policy::{evaluate_request_policy, PolicyDecision, PolicyReason};
use crate::tool_executor::{execute_tool, validate_tool_payload};
use crate::tool_validator::validate_tool_request;
use crate::KernelState;

pub fn execute_tool_request(
    state: &KernelState,
    request: &ToolRequest,
) -> Result<ToolResult, KernelError> {
    let started_at = Instant::now();

    if let Err(err) = validate_tool_request(request) {
        record_failure_audit(
            state,
            request,
            started_at.elapsed().as_millis(),
            "not_evaluated",
            "request_validation_failed",
            &err,
        );
        return Err(err);
    }

    let policy_decision = evaluate_request_policy(state, request);

    if !policy_decision.allowed {
        let err = denial_error(&request.tool_name, policy_decision);
        record_failure_audit(
            state,
            request,
            started_at.elapsed().as_millis(),
            "deny",
            policy_decision.reason.as_str(),
            &err,
        );
        return Err(err);
    }

    if let Err(err) = validate_tool_payload(request) {
        record_failure_audit(
            state,
            request,
            started_at.elapsed().as_millis(),
            "allow",
            policy_decision.reason.as_str(),
            &err,
        );
        return Err(err);
    }

    let output_json = if request.dry_run {
        json!({
            "dry_run": true,
            "tool_name": request.tool_name,
            "request_id": request.request_id,
        })
        .to_string()
    } else {
        match execute_tool(request) {
            Ok(output) => match output.to_json_string() {
                Ok(json) => json,
                Err(err) => {
                    record_failure_audit(
                        state,
                        request,
                        started_at.elapsed().as_millis(),
                        "allow",
                        policy_decision.reason.as_str(),
                        &err,
                    );
                    return Err(err);
                },
            },
            Err(err) => {
                record_failure_audit(
                    state,
                    request,
                    started_at.elapsed().as_millis(),
                    "allow",
                    policy_decision.reason.as_str(),
                    &err,
                );
                return Err(err);
            },
        }
    };

    let result = ToolResult {
        request_id: request.request_id.clone(),
        tool_name: request.tool_name.clone(),
        success: true,
        output_json,
        error_message: None,
        duration_ms: started_at.elapsed().as_millis(),
    };

    if let Err(err) = result.validate().map_err(KernelError::ContractValidation) {
        record_failure_audit(
            state,
            request,
            started_at.elapsed().as_millis(),
            "allow",
            policy_decision.reason.as_str(),
            &err,
        );
        return Err(err);
    }

    let audit_event = state.audit_log.record_tool_call(AuditEventInput {
        request_id: request.request_id.clone(),
        tool_name: request.tool_name.clone(),
        dry_run: request.dry_run,
        policy_decision: "allow".into(),
        policy_reason: policy_decision.reason.as_str().into(),
        success: true,
        duration_ms: result.duration_ms,
        error_message: None,
    });
    let _ = state.memory.record_tool_activity(
        request,
        &audit_event,
        state.config.runtime.critical_delay_ms,
    );

    Ok(result)
}

fn denial_error(tool_name: &str, decision: PolicyDecision) -> KernelError {
    match decision.reason {
        PolicyReason::UnknownTool => KernelError::UnknownTool(tool_name.into()),
        PolicyReason::ExplicitDeny
        | PolicyReason::DefaultDeny
        | PolicyReason::ToolsDisabledInConfig => KernelError::PolicyDenied {
            tool_name: tool_name.into(),
            reason: decision.reason,
        },
        PolicyReason::ThrottledByPolicy => KernelError::ToolThrottled {
            tool_name: tool_name.into(),
        },
        PolicyReason::ExplicitAllow | PolicyReason::DefaultAllow => {
            KernelError::ToolDisabled(tool_name.into())
        },
    }
}

fn record_failure_audit(
    state: &KernelState,
    request: &ToolRequest,
    duration_ms: u128,
    policy_decision: &str,
    policy_reason: &str,
    err: &KernelError,
) {
    let audit_event = state.audit_log.record_tool_call(AuditEventInput {
        request_id: request.request_id.clone(),
        tool_name: request.tool_name.clone(),
        dry_run: request.dry_run,
        policy_decision: policy_decision.into(),
        policy_reason: policy_reason.into(),
        success: false,
        duration_ms,
        error_message: Some(err.to_string()),
    });
    let _ = state.memory.record_tool_activity(
        request,
        &audit_event,
        state.config.runtime.critical_delay_ms,
    );
}
