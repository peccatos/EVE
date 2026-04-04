use std::collections::HashMap;

use crate::audit::{AuditEvent, AuditEventInput};
use crate::contracts::tool_request::ToolRequest;
use crate::error::KernelError;
use crate::memory::BottleneckEntry;
use crate::KernelState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticCycleObservation {
    pub cycle_index: u64,
    pub tool_name: String,
    pub duration_ms: u128,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveWindowReport {
    pub window_index: usize,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub event_count: usize,
    pub total_duration_ms: u128,
    pub avg_duration_ms: u128,
    pub max_duration_ms: u128,
    pub critical_delay_count: u64,
    pub failure_count: u64,
    pub dominant_tool_name: String,
    pub dominant_tool_total_duration_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutCandidateEntry {
    pub rank: usize,
    pub tool_name: String,
    pub relation: String,
    pub from_key: String,
    pub to_key: String,
    pub call_count: u64,
    pub failure_count: u64,
    pub critical_delay_count: u64,
    pub total_duration_ms: u128,
    pub avg_duration_ms: u128,
    pub max_duration_ms: u128,
    pub dominant_window_count: u64,
    pub dominant_window_critical_delay_count: u64,
    pub dominant_window_peak_avg_duration_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalanceAssignment {
    pub tool_name: String,
    pub target_sector: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorSimulationSummary {
    pub sector_id: String,
    pub event_count: usize,
    pub total_duration_ms: u128,
    pub avg_duration_ms: u128,
    pub max_duration_ms: u128,
    pub critical_delay_count: u64,
    pub failure_count: u64,
    pub wave_windows: Vec<WaveWindowReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorCapacityProfile {
    pub sector_id: String,
    pub max_events: usize,
    pub max_avg_duration_ms: u128,
    pub max_critical_delay_count: u64,
    pub max_failure_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorCapacityAssessment {
    pub sector_id: String,
    pub within_capacity: bool,
    pub exceeded_dimensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebalanceSimulationReport {
    pub source_sector_before: SectorSimulationSummary,
    pub sectors_after: Vec<SectorSimulationSummary>,
    pub moved_assignments: Vec<RebalanceAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacitySimulationReport {
    pub sectors: Vec<SectorCapacityAssessment>,
}

pub fn record_synthetic_cycle(
    state: &KernelState,
    observation: &SyntheticCycleObservation,
) -> Result<AuditEvent, KernelError> {
    let request = ToolRequest {
        request_id: format!("cycle-{:04}", observation.cycle_index),
        tool_name: observation.tool_name.clone(),
        payload_json: "{\"path\":\"/synthetic\"}".into(),
        dry_run: true,
        timeout_ms: Some(100),
    };

    let audit_event = state.audit_log.record_tool_call(AuditEventInput {
        request_id: request.request_id.clone(),
        tool_name: request.tool_name.clone(),
        dry_run: true,
        policy_decision: "allow".into(),
        policy_reason: "synthetic_cycle".into(),
        success: observation.success,
        duration_ms: observation.duration_ms,
        error_message: (!observation.success).then(|| "synthetic_failure".into()),
    });

    state.memory.record_tool_activity(
        &request,
        &audit_event,
        state.config.runtime.critical_delay_ms,
    )?;

    Ok(audit_event)
}

pub fn build_wave_report(
    events: &[AuditEvent],
    window_size: usize,
    critical_delay_ms: u64,
) -> Vec<WaveWindowReport> {
    if window_size == 0 {
        return Vec::new();
    }

    events
        .chunks(window_size)
        .enumerate()
        .filter_map(|(window_index, chunk)| {
            let first = chunk.first()?;
            let last = chunk.last()?;
            let mut total_duration_ms = 0_u128;
            let mut max_duration_ms = 0_u128;
            let mut critical_delay_count = 0_u64;
            let mut failure_count = 0_u64;
            let mut tool_totals: HashMap<String, u128> = HashMap::new();

            for event in chunk {
                total_duration_ms += event.duration_ms;
                max_duration_ms = max_duration_ms.max(event.duration_ms);

                if event.duration_ms >= u128::from(critical_delay_ms) {
                    critical_delay_count += 1;
                }

                if !event.success {
                    failure_count += 1;
                }

                *tool_totals.entry(event.tool_name.clone()).or_default() += event.duration_ms;
            }

            let (dominant_tool_name, dominant_tool_total_duration_ms) = tool_totals
                .into_iter()
                .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
                .unwrap_or_else(|| (String::new(), 0));

            Some(WaveWindowReport {
                window_index,
                start_sequence: first.sequence,
                end_sequence: last.sequence,
                event_count: chunk.len(),
                total_duration_ms,
                avg_duration_ms: total_duration_ms / chunk.len() as u128,
                max_duration_ms,
                critical_delay_count,
                failure_count,
                dominant_tool_name,
                dominant_tool_total_duration_ms,
            })
        })
        .collect()
}

pub fn build_cut_candidate_report(
    bottlenecks: &[BottleneckEntry],
    wave_windows: &[WaveWindowReport],
    limit: usize,
) -> Vec<CutCandidateEntry> {
    if limit == 0 {
        return Vec::new();
    }

    let mut candidates = bottlenecks
        .iter()
        .filter_map(|entry| {
            let tool_name = infer_tool_name(entry)?;
            let mut dominant_window_count = 0_u64;
            let mut dominant_window_critical_delay_count = 0_u64;
            let mut dominant_window_peak_avg_duration_ms = 0_u128;

            for window in wave_windows {
                if window.dominant_tool_name == tool_name {
                    dominant_window_count += 1;
                    dominant_window_critical_delay_count += window.critical_delay_count;
                    dominant_window_peak_avg_duration_ms =
                        dominant_window_peak_avg_duration_ms.max(window.avg_duration_ms);
                }
            }

            Some(CutCandidateEntry {
                rank: 0,
                tool_name,
                relation: entry.relation.clone(),
                from_key: entry.from_key.clone(),
                to_key: entry.to_key.clone(),
                call_count: entry.call_count,
                failure_count: entry.failure_count,
                critical_delay_count: entry.critical_delay_count,
                total_duration_ms: entry.total_duration_ms,
                avg_duration_ms: entry.avg_duration_ms,
                max_duration_ms: entry.max_duration_ms,
                dominant_window_count,
                dominant_window_critical_delay_count,
                dominant_window_peak_avg_duration_ms,
            })
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .critical_delay_count
            .cmp(&left.critical_delay_count)
            .then_with(|| right.dominant_window_count.cmp(&left.dominant_window_count))
            .then_with(|| {
                right
                    .dominant_window_critical_delay_count
                    .cmp(&left.dominant_window_critical_delay_count)
            })
            .then_with(|| right.total_duration_ms.cmp(&left.total_duration_ms))
            .then_with(|| {
                right
                    .dominant_window_peak_avg_duration_ms
                    .cmp(&left.dominant_window_peak_avg_duration_ms)
            })
            .then_with(|| right.failure_count.cmp(&left.failure_count))
            .then_with(|| right.max_duration_ms.cmp(&left.max_duration_ms))
            .then_with(|| left.relation.cmp(&right.relation))
            .then_with(|| left.from_key.cmp(&right.from_key))
            .then_with(|| left.to_key.cmp(&right.to_key))
    });

    candidates
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, mut entry)| {
            entry.rank = index + 1;
            entry
        })
        .collect()
}

pub fn build_rebalance_plan(
    cut_candidates: &[CutCandidateEntry],
    target_sector: &str,
    limit: usize,
) -> Vec<RebalanceAssignment> {
    if limit == 0 || target_sector.trim().is_empty() {
        return Vec::new();
    }

    let mut assignments = Vec::new();

    for candidate in cut_candidates {
        if assignments.len() >= limit {
            break;
        }

        if assignments
            .iter()
            .any(|assignment: &RebalanceAssignment| assignment.tool_name == candidate.tool_name)
        {
            continue;
        }

        assignments.push(RebalanceAssignment {
            tool_name: candidate.tool_name.clone(),
            target_sector: target_sector.into(),
        });
    }

    assignments
}

pub fn simulate_sector_rebalance(
    events: &[AuditEvent],
    source_sector: &str,
    assignments: &[RebalanceAssignment],
    window_size: usize,
    critical_delay_ms: u64,
) -> RebalanceSimulationReport {
    let source_sector_before =
        build_sector_summary(source_sector, events, window_size, critical_delay_ms);
    let assignment_map = assignments
        .iter()
        .map(|assignment| {
            (
                assignment.tool_name.as_str(),
                assignment.target_sector.as_str(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut events_by_sector: HashMap<String, Vec<AuditEvent>> = HashMap::new();

    for event in events {
        let target_sector = assignment_map
            .get(event.tool_name.as_str())
            .copied()
            .unwrap_or(source_sector);

        events_by_sector
            .entry(target_sector.into())
            .or_default()
            .push(event.clone());
    }

    let mut sectors_after = events_by_sector
        .into_iter()
        .map(|(sector_id, sector_events)| {
            build_sector_summary(&sector_id, &sector_events, window_size, critical_delay_ms)
        })
        .collect::<Vec<_>>();
    sectors_after.sort_by(|left, right| left.sector_id.cmp(&right.sector_id));

    RebalanceSimulationReport {
        source_sector_before,
        sectors_after,
        moved_assignments: assignments.to_vec(),
    }
}

pub fn assess_sector_capacity(
    sectors: &[SectorSimulationSummary],
    capacity_profiles: &[SectorCapacityProfile],
) -> CapacitySimulationReport {
    let profiles = capacity_profiles
        .iter()
        .map(|profile| (profile.sector_id.as_str(), profile))
        .collect::<HashMap<_, _>>();
    let mut assessments = sectors
        .iter()
        .map(|sector| {
            let Some(profile) = profiles.get(sector.sector_id.as_str()) else {
                return SectorCapacityAssessment {
                    sector_id: sector.sector_id.clone(),
                    within_capacity: false,
                    exceeded_dimensions: vec!["missing_capacity_profile".into()],
                };
            };

            let mut exceeded_dimensions = Vec::new();

            if sector.event_count > profile.max_events {
                exceeded_dimensions.push("max_events".into());
            }

            if sector.avg_duration_ms > profile.max_avg_duration_ms {
                exceeded_dimensions.push("max_avg_duration_ms".into());
            }

            if sector.critical_delay_count > profile.max_critical_delay_count {
                exceeded_dimensions.push("max_critical_delay_count".into());
            }

            if sector.failure_count > profile.max_failure_count {
                exceeded_dimensions.push("max_failure_count".into());
            }

            SectorCapacityAssessment {
                sector_id: sector.sector_id.clone(),
                within_capacity: exceeded_dimensions.is_empty(),
                exceeded_dimensions,
            }
        })
        .collect::<Vec<_>>();
    assessments.sort_by(|left, right| left.sector_id.cmp(&right.sector_id));

    CapacitySimulationReport {
        sectors: assessments,
    }
}

fn infer_tool_name(entry: &BottleneckEntry) -> Option<String> {
    if let Some(tool_name) = entry.to_key.strip_prefix("tool:") {
        return Some(tool_name.into());
    }

    if let Some(tool_name) = entry.from_key.strip_prefix("tool:") {
        return Some(tool_name.into());
    }

    None
}

fn build_sector_summary(
    sector_id: &str,
    events: &[AuditEvent],
    window_size: usize,
    critical_delay_ms: u64,
) -> SectorSimulationSummary {
    let mut total_duration_ms = 0_u128;
    let mut max_duration_ms = 0_u128;
    let mut critical_delay_count = 0_u64;
    let mut failure_count = 0_u64;

    for event in events {
        total_duration_ms += event.duration_ms;
        max_duration_ms = max_duration_ms.max(event.duration_ms);

        if event.duration_ms >= u128::from(critical_delay_ms) {
            critical_delay_count += 1;
        }

        if !event.success {
            failure_count += 1;
        }
    }

    SectorSimulationSummary {
        sector_id: sector_id.into(),
        event_count: events.len(),
        total_duration_ms,
        avg_duration_ms: if events.is_empty() {
            0
        } else {
            total_duration_ms / events.len() as u128
        },
        max_duration_ms,
        critical_delay_count,
        failure_count,
        wave_windows: build_wave_report(events, window_size, critical_delay_ms),
    }
}
