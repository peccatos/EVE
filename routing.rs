use crate::contracts::tool_request::ToolRequest;
use crate::cycles::{CapacitySimulationReport, RebalanceAssignment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteReason {
    NoAssignment,
    RebalanceApplied,
    TargetOverCapacity,
    MissingCapacityProfile,
}

impl RouteReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoAssignment => "no_assignment",
            Self::RebalanceApplied => "rebalance_applied",
            Self::TargetOverCapacity => "target_over_capacity",
            Self::MissingCapacityProfile => "missing_capacity_profile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecision {
    pub source_sector: String,
    pub target_sector: String,
    pub reason: RouteReason,
}

pub fn apply_rebalance_policy(
    request: &ToolRequest,
    source_sector: &str,
    assignments: &[RebalanceAssignment],
    capacity_report: &CapacitySimulationReport,
) -> RouteDecision {
    let assignment = assignments
        .iter()
        .find(|assignment| assignment.tool_name == request.tool_name);

    let Some(assignment) = assignment else {
        return RouteDecision {
            source_sector: source_sector.into(),
            target_sector: source_sector.into(),
            reason: RouteReason::NoAssignment,
        };
    };

    let assessment = capacity_report
        .sectors
        .iter()
        .find(|sector| sector.sector_id == assignment.target_sector);

    let Some(assessment) = assessment else {
        return RouteDecision {
            source_sector: source_sector.into(),
            target_sector: source_sector.into(),
            reason: RouteReason::MissingCapacityProfile,
        };
    };

    if !assessment.within_capacity {
        return RouteDecision {
            source_sector: source_sector.into(),
            target_sector: source_sector.into(),
            reason: RouteReason::TargetOverCapacity,
        };
    }

    RouteDecision {
        source_sector: source_sector.into(),
        target_sector: assignment.target_sector.clone(),
        reason: RouteReason::RebalanceApplied,
    }
}
