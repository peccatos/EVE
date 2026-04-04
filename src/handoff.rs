use crate::contracts::handoff::{HandoffReceipt, HandoffRequest};
use crate::contracts::tool_request::ToolRequest;
use crate::routing::{RouteDecision, RouteReason};
use crate::sector_registry::SectorRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffReason {
    LocalExecution,
    HandoffPrepared,
    UnknownTargetSector,
    TargetSectorDisabled,
    TargetDoesNotAcceptHandoffs,
    ToolUnsupportedInTarget,
}

impl HandoffReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalExecution => "local_execution",
            Self::HandoffPrepared => "handoff_prepared",
            Self::UnknownTargetSector => "unknown_target_sector",
            Self::TargetSectorDisabled => "target_sector_disabled",
            Self::TargetDoesNotAcceptHandoffs => "target_does_not_accept_handoffs",
            Self::ToolUnsupportedInTarget => "tool_unsupported_in_target",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffDecision {
    pub handoff: Option<HandoffRequest>,
    pub receipt: HandoffReceipt,
}

pub fn prepare_handoff(
    request: &ToolRequest,
    route: &RouteDecision,
    registry: &SectorRegistry,
) -> HandoffDecision {
    if route.source_sector == route.target_sector {
        return local_receipt(request, route, HandoffReason::LocalExecution);
    }

    let Some(target_sector) = registry.get(&route.target_sector) else {
        return local_receipt(request, route, HandoffReason::UnknownTargetSector);
    };

    if !target_sector.enabled {
        return local_receipt(request, route, HandoffReason::TargetSectorDisabled);
    }

    if !target_sector.accepts_handoffs {
        return local_receipt(request, route, HandoffReason::TargetDoesNotAcceptHandoffs);
    }

    if !registry.supports_tool(&route.target_sector, &request.tool_name) {
        return local_receipt(request, route, HandoffReason::ToolUnsupportedInTarget);
    }

    let handoff = HandoffRequest {
        handoff_id: format!(
            "handoff:{}:{}:{}",
            request.request_id, route.source_sector, route.target_sector
        ),
        request_id: request.request_id.clone(),
        tool_name: request.tool_name.clone(),
        source_sector: route.source_sector.clone(),
        target_sector: route.target_sector.clone(),
        payload_json: request.payload_json.clone(),
        dry_run: request.dry_run,
        timeout_ms: request.timeout_ms,
    };

    let accepted = handoff.validate().is_ok();
    let receipt = HandoffReceipt {
        handoff_id: handoff.handoff_id.clone(),
        request_id: request.request_id.clone(),
        accepted,
        source_sector: route.source_sector.clone(),
        target_sector: route.target_sector.clone(),
        reason: HandoffReason::HandoffPrepared.as_str().into(),
    };

    HandoffDecision {
        handoff: Some(handoff),
        receipt,
    }
}

fn local_receipt(
    request: &ToolRequest,
    route: &RouteDecision,
    reason: HandoffReason,
) -> HandoffDecision {
    HandoffDecision {
        handoff: None,
        receipt: HandoffReceipt {
            handoff_id: format!(
                "handoff:{}:{}:{}",
                request.request_id, route.source_sector, route.target_sector
            ),
            request_id: request.request_id.clone(),
            accepted: false,
            source_sector: route.source_sector.clone(),
            target_sector: route.target_sector.clone(),
            reason: match route.reason {
                RouteReason::NoAssignment
                | RouteReason::TargetOverCapacity
                | RouteReason::MissingCapacityProfile
                | RouteReason::RebalanceApplied => reason.as_str().into(),
            },
        },
    }
}
