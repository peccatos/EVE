use crate::contracts::tool_request::ToolRequest;
use crate::cycles::{build_cut_candidate_report, build_wave_report};
use std::fmt;

use crate::tool_registry::builtin_tools;
use crate::KernelState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyReason {
    ToolsDisabledInConfig,
    UnknownTool,
    ExplicitAllow,
    ExplicitDeny,
    DefaultAllow,
    DefaultDeny,
    ThrottledByPolicy,
}

impl PolicyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolsDisabledInConfig => "tools_disabled_in_config",
            Self::UnknownTool => "unknown_tool",
            Self::ExplicitAllow => "explicit_allow",
            Self::ExplicitDeny => "explicit_deny",
            Self::DefaultAllow => "default_allow",
            Self::DefaultDeny => "default_deny",
            Self::ThrottledByPolicy => "throttled_by_policy",
        }
    }
}

impl fmt::Display for PolicyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub reason: PolicyReason,
}

pub fn evaluate_tool_policy(state: &KernelState, tool_name: &str) -> PolicyDecision {
    if !state.config.tools.enabled {
        return PolicyDecision {
            allowed: false,
            reason: PolicyReason::ToolsDisabledInConfig,
        };
    }

    if !builtin_tools().iter().any(|tool| tool.name == tool_name) {
        return PolicyDecision {
            allowed: false,
            reason: PolicyReason::UnknownTool,
        };
    }

    if let Some(tool) = state
        .tool_policy
        .tools
        .iter()
        .find(|tool| tool.name == tool_name)
    {
        return PolicyDecision {
            allowed: tool.enabled,
            reason: if tool.enabled {
                PolicyReason::ExplicitAllow
            } else {
                PolicyReason::ExplicitDeny
            },
        };
    }

    let default_allow = state.tool_policy.policy.default_action.trim() == "allow";

    PolicyDecision {
        allowed: default_allow,
        reason: if default_allow {
            PolicyReason::DefaultAllow
        } else {
            PolicyReason::DefaultDeny
        },
    }
}

pub fn evaluate_request_policy(state: &KernelState, request: &ToolRequest) -> PolicyDecision {
    let base = evaluate_tool_policy(state, &request.tool_name);

    if !base.allowed {
        return base;
    }

    if !state.tool_policy.throttle.enabled {
        return base;
    }

    if state.tool_policy.throttle.exempt_dry_run && request.dry_run {
        return base;
    }

    let events = state.audit_log.events();
    if events.is_empty() {
        return base;
    }

    let bottlenecks = match state.memory.bottleneck_report(
        &state.config.eve.sector,
        state.tool_policy.throttle.candidate_limit,
    ) {
        Ok(report) => report,
        Err(_) => return base,
    };
    let wave = build_wave_report(
        &events,
        state.tool_policy.throttle.window_size,
        state.config.runtime.critical_delay_ms,
    );
    let cut_candidates = build_cut_candidate_report(
        &bottlenecks,
        &wave,
        state.tool_policy.throttle.candidate_limit,
    );

    if !cut_candidates
        .iter()
        .any(|candidate| candidate.tool_name == request.tool_name)
    {
        return base;
    }

    let recent_events = events
        .iter()
        .rev()
        .take(state.tool_policy.throttle.window_size)
        .filter(|event| event.tool_name == request.tool_name);

    let mut critical_delay_count = 0_u64;
    let mut failure_count = 0_u64;

    for event in recent_events {
        if event.duration_ms >= u128::from(state.config.runtime.critical_delay_ms) {
            critical_delay_count += 1;
        }

        if !event.success {
            failure_count += 1;
        }
    }

    if critical_delay_count >= state.tool_policy.throttle.critical_delay_threshold
        || failure_count >= state.tool_policy.throttle.failure_threshold
    {
        return PolicyDecision {
            allowed: false,
            reason: PolicyReason::ThrottledByPolicy,
        };
    }

    base
}
