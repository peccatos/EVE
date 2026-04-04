use std::fs;

use kernel::audit::{AuditEventInput, AuditLog, AUDIT_EVENT_SCHEMA_VERSION};
use kernel::contracts::file_exists::FileExistsRequest;
use kernel::contracts::list_dir::ListDirRequest;
use kernel::contracts::read_file::ReadFileRequest;
use kernel::contracts::tool_request::ToolRequest;
use kernel::contracts::tool_result::ToolResult;
use kernel::cycles::{
    assess_sector_capacity, build_cut_candidate_report, build_rebalance_plan, build_wave_report,
    record_synthetic_cycle, simulate_sector_rebalance, SectorCapacityProfile,
    SyntheticCycleObservation,
};
use kernel::error::KernelError;
use kernel::handoff::{prepare_handoff, HandoffReason};
use kernel::memory::KernelMemory;
use kernel::policy::{evaluate_request_policy, evaluate_tool_policy, PolicyReason};
use kernel::routing::{apply_rebalance_policy, RouteReason};
use kernel::runtime::execute_tool_request;
use kernel::sector_registry::{SectorDescriptor, SectorRegistry};
use kernel::tool_executor::{decode_tool_payload, ToolPayload};
use kernel::{
    validate_config, validate_tool_policy, validate_tool_registry_alignment, EveConfig,
    EveSection, KernelState, LoggingSection, MemorySection, ModelSection, PolicySection,
    RuntimeSection, ThrottleSection, ToolEntry, ToolPolicyConfig, ToolsSection,
};
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::tempdir;

fn test_kernel_state() -> KernelState {
    KernelState {
        config: EveConfig {
            eve: EveSection {
                name: "EVE OSS".into(),
                version: "0.1.0".into(),
                platform: "linux".into(),
                mode: "runtime".into(),
                sector: "core".into(),
            },
            logging: LoggingSection {
                level: "info".into(),
                format: "pretty".into(),
            },
            runtime: RuntimeSection {
                deterministic: true,
                max_cycles: 64,
                critical_delay_ms: 100,
            },
            tools: ToolsSection {
                enabled: true,
                policy_path: "config/tool_policy.toml".into(),
            },
            model: ModelSection {
                enabled: false,
                json_only: true,
            },
            memory: MemorySection {
                sector_id: "core".into(),
                max_nodes_per_sector: 8,
                max_edges_per_sector: 8,
            },
        },
        tool_policy: ToolPolicyConfig {
            policy: PolicySection {
                default_action: "deny".into(),
                require_json_contract: true,
                log_all_calls: true,
            },
            throttle: ThrottleSection {
                enabled: true,
                window_size: 20,
                candidate_limit: 3,
                critical_delay_threshold: 4,
                failure_threshold: 2,
                exempt_dry_run: true,
            },
            tools: vec![
                ToolEntry {
                    name: "read_file".into(),
                    enabled: true,
                    side_effects: false,
                },
                ToolEntry {
                    name: "list_dir".into(),
                    enabled: true,
                    side_effects: false,
                },
                ToolEntry {
                    name: "file_exists".into(),
                    enabled: true,
                    side_effects: false,
                },
            ],
        },
        audit_log: AuditLog::new(),
        memory: KernelMemory::new("core".into(), 8, 8),
    }
}

fn request(tool_name: &str, payload_json: String) -> ToolRequest {
    ToolRequest {
        request_id: "req-1".into(),
        tool_name: tool_name.into(),
        payload_json,
        dry_run: false,
        timeout_ms: Some(100),
    }
}

#[test]
fn shipped_config_and_policy_validate() {
    let config: EveConfig =
        toml::from_str(include_str!("../config/eve.toml")).expect("config parses");
    validate_config(&config).expect("config validates");

    let policy: ToolPolicyConfig =
        toml::from_str(include_str!("../config/tool_policy.toml")).expect("policy parses");
    validate_tool_policy(&policy).expect("policy validates");

    let state = KernelState {
        config,
        tool_policy: policy,
        audit_log: AuditLog::new(),
        memory: KernelMemory::new("core".into(), 1024, 4096),
    };

    validate_tool_registry_alignment(&state).expect("registry aligns");
}

#[test]
fn request_validation_rejects_invalid_fields() {
    let mut invalid = request("read_file", "{\"path\":\"x\"}".into());
    invalid.request_id = " ".into();
    assert!(invalid.validate().is_err());

    let mut invalid = request("read_file", "{\"path\":\"x\"}".into());
    invalid.tool_name = String::new();
    assert!(invalid.validate().is_err());

    let invalid = request("read_file", String::new());
    assert!(invalid.validate().is_err());

    let mut invalid = request("read_file", "{\"path\":\"x\"}".into());
    invalid.timeout_ms = Some(0);
    assert!(invalid.validate().is_err());
}

#[test]
fn runtime_rejects_unknown_tool() {
    let state = test_kernel_state();
    let err = execute_tool_request(&state, &request("missing_tool", "{\"path\":\"x\"}".into()))
        .expect_err("unknown tool should fail");

    assert!(matches!(err, KernelError::UnknownTool(name) if name == "missing_tool"));
}

#[test]
fn runtime_rejects_tool_disabled_by_policy() {
    let mut state = test_kernel_state();
    state.tool_policy.tools[0].enabled = false;

    let err = execute_tool_request(&state, &request("read_file", "{\"path\":\"x\"}".into()))
        .expect_err("disabled tool should fail");

    assert!(
        matches!(err, KernelError::PolicyDenied { tool_name, reason }
            if tool_name == "read_file" && reason == PolicyReason::ExplicitDeny)
    );
}

#[test]
fn runtime_rejects_tool_when_throttle_policy_trips() {
    let mut state = test_kernel_state();
    state.memory = KernelMemory::new("core".into(), 4096, 4096);

    for cycle_index in 0..20_u64 {
        record_synthetic_cycle(
            &state,
            &SyntheticCycleObservation {
                cycle_index,
                tool_name: "read_file".into(),
                duration_ms: 220,
                success: false,
            },
        )
        .expect("record synthetic cycle");
    }

    let err = execute_tool_request(&state, &request("read_file", "{\"path\":\"x\"}".into()))
        .expect_err("throttled tool should fail");

    assert!(matches!(err, KernelError::ToolThrottled { tool_name } if tool_name == "read_file"));

    let audit = state.audit_log.events();
    let last = audit.last().expect("last audit");
    assert_eq!(last.policy_decision, "deny");
    assert_eq!(last.policy_reason, "throttled_by_policy");
    assert!(!last.success);
}

#[test]
fn policy_returns_expected_reasons() {
    let state = test_kernel_state();

    let allowed = evaluate_tool_policy(&state, "read_file");
    assert!(allowed.allowed);
    assert_eq!(allowed.reason, PolicyReason::ExplicitAllow);

    let unknown = evaluate_tool_policy(&state, "missing_tool");
    assert!(!unknown.allowed);
    assert_eq!(unknown.reason, PolicyReason::UnknownTool);

    let mut default_allow_state = test_kernel_state();
    default_allow_state.tool_policy.tools.clear();
    default_allow_state.tool_policy.policy.default_action = "allow".into();

    let fallback = evaluate_tool_policy(&default_allow_state, "read_file");
    assert!(fallback.allowed);
    assert_eq!(fallback.reason, PolicyReason::DefaultAllow);
}

#[test]
fn request_policy_throttles_hot_tool_after_critical_wave() {
    let mut state = test_kernel_state();
    state.memory = KernelMemory::new("core".into(), 4096, 4096);

    for cycle_index in 0..20_u64 {
        record_synthetic_cycle(
            &state,
            &SyntheticCycleObservation {
                cycle_index,
                tool_name: "read_file".into(),
                duration_ms: 220,
                success: cycle_index % 7 != 0,
            },
        )
        .expect("record synthetic cycle");
    }

    let decision =
        evaluate_request_policy(&state, &request("read_file", "{\"path\":\"x\"}".into()));
    assert!(!decision.allowed);
    assert_eq!(decision.reason, PolicyReason::ThrottledByPolicy);
}

#[test]
fn request_policy_exempts_dry_run_from_throttle() {
    let mut state = test_kernel_state();
    state.memory = KernelMemory::new("core".into(), 4096, 4096);

    for cycle_index in 0..20_u64 {
        record_synthetic_cycle(
            &state,
            &SyntheticCycleObservation {
                cycle_index,
                tool_name: "read_file".into(),
                duration_ms: 220,
                success: false,
            },
        )
        .expect("record synthetic cycle");
    }

    let mut dry_run_request = request("read_file", "{\"path\":\"x\"}".into());
    dry_run_request.dry_run = true;

    let decision = evaluate_request_policy(&state, &dry_run_request);
    assert!(decision.allowed);
    assert_eq!(decision.reason, PolicyReason::ExplicitAllow);
}

#[test]
fn typed_payload_decode_uses_per_tool_contracts() {
    let read_file_request = request("read_file", "{\"path\":\"note.txt\"}".into());
    let list_dir_request = request("list_dir", "{\"path\":\"/tmp\"}".into());
    let file_exists_request = request("file_exists", "{\"path\":\"Cargo.toml\"}".into());

    assert_eq!(
        decode_tool_payload(&read_file_request).expect("decode read_file"),
        ToolPayload::ReadFile(ReadFileRequest {
            path: "note.txt".into(),
        })
    );
    assert_eq!(
        decode_tool_payload(&list_dir_request).expect("decode list_dir"),
        ToolPayload::ListDir(ListDirRequest {
            path: "/tmp".into(),
        })
    );
    assert_eq!(
        decode_tool_payload(&file_exists_request).expect("decode file_exists"),
        ToolPayload::FileExists(FileExistsRequest {
            path: "Cargo.toml".into(),
        })
    );
}

#[test]
fn dry_run_succeeds_without_touching_files() {
    let state = test_kernel_state();
    let mut dry_run_request = request("read_file", "{\"path\":\"/definitely/missing\"}".into());
    dry_run_request.dry_run = true;

    let result = execute_tool_request(&state, &dry_run_request).expect("dry run should pass");
    let json: Value = serde_json::from_str(&result.output_json).expect("dry run json");

    assert!(result.success);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["tool_name"], "read_file");

    let audit = state.audit_log.events();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].schema_version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(audit[0].sequence, 0);
    assert_eq!(audit[0].event_type, "tool_call");
    assert_eq!(audit[0].policy_decision, "allow");
    assert_eq!(audit[0].policy_reason, "explicit_allow");
    assert!(audit[0].success);

    let snapshot = state
        .memory
        .sector_snapshot("core")
        .expect("memory snapshot");
    assert!(snapshot.nodes.iter().any(|node| node.key == "kernel:core"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "request:req-1"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "tool:read_file"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "audit:req-1:0"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "status:success"));
    assert!(snapshot
        .edges
        .iter()
        .any(|edge| edge.relation == "owns_request"));
    assert!(snapshot
        .edges
        .iter()
        .any(|edge| edge.relation == "targets_tool"));
    assert!(snapshot
        .edges
        .iter()
        .any(|edge| edge.relation == "emits_audit"));
    assert!(snapshot
        .edges
        .iter()
        .any(|edge| edge.relation == "records_status"));
    assert!(snapshot.flow_metrics.iter().any(|metric| {
        metric.relation == "dispatches_tool"
            && metric.from_key == "kernel:core"
            && metric.to_key == "tool:read_file"
            && metric.call_count == 1
            && metric.success_count == 1
    }));
    assert!(snapshot.flow_metrics.iter().any(|metric| {
        metric.relation == "returns_status"
            && metric.from_key == "tool:read_file"
            && metric.to_key == "status:success"
            && metric.call_count == 1
    }));
}

#[test]
fn file_exists_reports_existing_and_missing_paths() {
    let state = test_kernel_state();
    let temp = tempdir().expect("temp dir");
    let file_path = temp.path().join("exists.txt");
    fs::write(&file_path, "hello").expect("write temp file");

    let existing = execute_tool_request(
        &state,
        &request(
            "file_exists",
            format!("{{\"path\":\"{}\"}}", file_path.display()),
        ),
    )
    .expect("existing path should succeed");
    let existing_json: Value = serde_json::from_str(&existing.output_json).expect("json");

    let missing_path = temp.path().join("missing.txt");
    let missing = execute_tool_request(
        &state,
        &request(
            "file_exists",
            format!("{{\"path\":\"{}\"}}", missing_path.display()),
        ),
    )
    .expect("missing path should still succeed");
    let missing_json: Value = serde_json::from_str(&missing.output_json).expect("json");

    assert_eq!(existing_json["exists"], true);
    assert_eq!(missing_json["exists"], false);
}

#[test]
fn list_dir_returns_stable_sorted_entries() {
    let state = test_kernel_state();
    let temp = tempdir().expect("temp dir");
    fs::write(temp.path().join("b.txt"), "b").expect("write b");
    fs::write(temp.path().join("a.txt"), "a").expect("write a");

    let result = execute_tool_request(
        &state,
        &request(
            "list_dir",
            format!("{{\"path\":\"{}\"}}", temp.path().display()),
        ),
    )
    .expect("list dir should succeed");
    let json: Value = serde_json::from_str(&result.output_json).expect("json");

    assert_eq!(json["entries"], serde_json::json!(["a.txt", "b.txt"]));
}

#[test]
fn read_file_returns_contents() {
    let state = test_kernel_state();
    let temp = tempdir().expect("temp dir");
    let file_path = temp.path().join("note.txt");
    fs::write(&file_path, "kernel").expect("write file");

    let result = execute_tool_request(
        &state,
        &request(
            "read_file",
            format!("{{\"path\":\"{}\"}}", file_path.display()),
        ),
    )
    .expect("read file should succeed");
    let json: Value = serde_json::from_str(&result.output_json).expect("json");

    assert_eq!(json["contents"], "kernel");
}

#[test]
fn malformed_json_payload_returns_error() {
    let state = test_kernel_state();
    let err = execute_tool_request(&state, &request("read_file", "{bad json}".into()))
        .expect_err("invalid json should fail");

    assert!(
        matches!(err, KernelError::MalformedPayload { tool_name, .. } if tool_name == "read_file")
    );

    let audit = state.audit_log.events();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].policy_decision, "allow");
    assert_eq!(audit[0].policy_reason, "explicit_allow");
    assert!(!audit[0].success);

    let snapshot = state
        .memory
        .sector_snapshot("core")
        .expect("memory snapshot");
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "tool:read_file"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "audit:req-1:0"));
    assert!(snapshot
        .nodes
        .iter()
        .any(|node| node.key == "status:failure"));
    assert!(snapshot.flow_metrics.iter().any(|metric| {
        metric.relation == "returns_status"
            && metric.from_key == "tool:read_file"
            && metric.to_key == "status:failure"
            && metric.failure_count == 1
    }));
}

#[test]
fn denied_request_writes_audit_event() {
    let mut state = test_kernel_state();
    state.tool_policy.tools[1].enabled = false;

    let err = execute_tool_request(&state, &request("list_dir", "{\"path\":\".\"}".into()))
        .expect_err("policy deny should fail");

    assert!(
        matches!(err, KernelError::PolicyDenied { tool_name, reason }
            if tool_name == "list_dir" && reason == PolicyReason::ExplicitDeny)
    );

    let audit = state.audit_log.events();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].schema_version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(audit[0].sequence, 0);
    assert_eq!(audit[0].event_type, "tool_call");
    assert_eq!(audit[0].policy_decision, "deny");
    assert_eq!(audit[0].policy_reason, "explicit_deny");
    assert!(!audit[0].success);
}

#[test]
fn audit_events_increment_sequence_deterministically() {
    let state = test_kernel_state();
    let temp = tempdir().expect("temp dir");
    let file_path = temp.path().join("note.txt");
    fs::write(&file_path, "kernel").expect("write file");

    execute_tool_request(
        &state,
        &request(
            "file_exists",
            format!("{{\"path\":\"{}\"}}", file_path.display()),
        ),
    )
    .expect("first call succeeds");

    execute_tool_request(
        &state,
        &request(
            "read_file",
            format!("{{\"path\":\"{}\"}}", file_path.display()),
        ),
    )
    .expect("second call succeeds");

    let audit = state.audit_log.events();
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].sequence, 0);
    assert_eq!(audit[1].sequence, 1);
}

#[test]
fn audit_log_persists_and_loads_jsonl_round_trip() {
    let audit_log = AuditLog::new();
    audit_log.record_tool_call(AuditEventInput {
        request_id: "req-1".into(),
        tool_name: "file_exists".into(),
        dry_run: false,
        policy_decision: "allow".into(),
        policy_reason: "explicit_allow".into(),
        success: true,
        duration_ms: 7,
        error_message: None,
    });
    audit_log.record_tool_call(AuditEventInput {
        request_id: "req-2".into(),
        tool_name: "read_file".into(),
        dry_run: true,
        policy_decision: "deny".into(),
        policy_reason: "explicit_deny".into(),
        success: false,
        duration_ms: 3,
        error_message: Some("policy denied".into()),
    });

    let temp = tempdir().expect("temp dir");
    let path = temp.path().join("audit").join("tool_calls.jsonl");

    audit_log.persist_jsonl(&path).expect("persist audit");
    let loaded = AuditLog::load_jsonl(&path).expect("load audit");

    assert_eq!(loaded.events(), audit_log.events());
}

#[test]
fn tool_result_validation_enforces_success_and_failure_invariants() {
    let success = ToolResult {
        request_id: "req-1".into(),
        tool_name: "file_exists".into(),
        success: true,
        output_json: "{\"exists\":true}".into(),
        error_message: None,
        duration_ms: 0,
    };
    success.validate().expect("success result validates");

    let failure = ToolResult {
        request_id: "req-2".into(),
        tool_name: "file_exists".into(),
        success: false,
        output_json: String::new(),
        error_message: Some("boom".into()),
        duration_ms: 0,
    };
    failure.validate().expect("failure result validates");

    let invalid_failure = ToolResult {
        request_id: "req-3".into(),
        tool_name: "file_exists".into(),
        success: false,
        output_json: String::new(),
        error_message: None,
        duration_ms: 0,
    };
    assert!(invalid_failure.validate().is_err());
}

#[test]
fn kernel_memory_is_bound_to_its_sector() {
    let state = test_kernel_state();

    state
        .memory
        .upsert_node("core", "kernel:main")
        .expect("node write in own sector succeeds");

    let err = state
        .memory
        .upsert_node("other", "kernel:foreign")
        .expect_err("cross-sector write should fail");

    assert!(
        matches!(err, KernelError::MemorySectorAccessDenied { kernel_sector, requested_sector }
            if kernel_sector == "core" && requested_sector == "other")
    );
}

#[test]
fn kernel_memory_deduplicates_nodes_and_tracks_edges() {
    let state = test_kernel_state();

    let first_node = state
        .memory
        .upsert_node("core", "kernel:alpha")
        .expect("first node");
    let duplicate_node = state
        .memory
        .upsert_node("core", "kernel:alpha")
        .expect("duplicate node");
    let edge_id = state
        .memory
        .add_edge("core", "kernel:alpha", "tool:read_file", "uses")
        .expect("edge");
    let duplicate_edge_id = state
        .memory
        .add_edge("core", "kernel:alpha", "tool:read_file", "uses")
        .expect("duplicate edge");

    let snapshot = state
        .memory
        .sector_snapshot("core")
        .expect("snapshot for core");

    assert_eq!(first_node, duplicate_node);
    assert_eq!(edge_id, 0);
    assert_eq!(duplicate_edge_id, edge_id);
    assert_eq!(snapshot.nodes.len(), 2);
    assert_eq!(snapshot.edges.len(), 1);
    assert_eq!(snapshot.nodes[0].key, "kernel:alpha");
    assert_eq!(snapshot.edges[0].relation, "uses");
    assert!(snapshot.flow_metrics.is_empty());
}

#[test]
fn kernel_memory_enforces_sector_limits() {
    let memory = KernelMemory::new("core".into(), 2, 1);

    memory
        .upsert_node("core", "node:a")
        .expect("first node within limit");
    memory
        .upsert_node("core", "node:b")
        .expect("second node within limit");

    let node_err = memory
        .upsert_node("core", "node:c")
        .expect_err("third node exceeds limit");
    assert!(
        matches!(node_err, KernelError::MemoryNodeLimitReached { sector_id, limit }
            if sector_id == "core" && limit == 2)
    );

    let edge_memory = KernelMemory::new("core".into(), 4, 1);
    edge_memory
        .add_edge("core", "node:a", "node:b", "links")
        .expect("first edge within limit");
    let edge_err = edge_memory
        .add_edge("core", "node:b", "node:c", "links")
        .expect_err("second edge exceeds limit");
    assert!(
        matches!(edge_err, KernelError::MemoryEdgeLimitReached { sector_id, limit }
            if sector_id == "core" && limit == 1)
    );
}

#[test]
fn kernel_memory_tracks_critical_delay_flow_metrics() {
    let state = test_kernel_state();
    let req = request("read_file", "{\"path\":\"/tmp/demo\"}".into());
    let audit = state.audit_log.record_tool_call(AuditEventInput {
        request_id: req.request_id.clone(),
        tool_name: req.tool_name.clone(),
        dry_run: false,
        policy_decision: "allow".into(),
        policy_reason: "explicit_allow".into(),
        success: false,
        duration_ms: 250,
        error_message: Some("timeout".into()),
    });

    state
        .memory
        .record_tool_activity(&req, &audit, state.config.runtime.critical_delay_ms)
        .expect("record tool activity");

    let snapshot = state.memory.sector_snapshot("core").expect("snapshot");
    let dispatch_metric = snapshot
        .flow_metrics
        .iter()
        .find(|metric| {
            metric.relation == "dispatches_tool"
                && metric.from_key == "kernel:core"
                && metric.to_key == "tool:read_file"
        })
        .expect("dispatch metric");

    assert_eq!(dispatch_metric.call_count, 1);
    assert_eq!(dispatch_metric.failure_count, 1);
    assert_eq!(dispatch_metric.max_duration_ms, 250);
    assert_eq!(dispatch_metric.critical_delay_count, 1);
    assert_eq!(dispatch_metric.last_sequence, 0);
}

#[test]
fn bottleneck_report_returns_empty_for_zero_limit() {
    let state = test_kernel_state();
    let report = state
        .memory
        .bottleneck_report("core", 0)
        .expect("bottleneck report");

    assert!(report.is_empty());
}

#[test]
fn bottleneck_report_sorts_by_criticality_and_duration() {
    let mut state = test_kernel_state();
    state.memory = KernelMemory::new("core".into(), 32, 32);

    let events = [
        ("req-1", "read_file", false, 250_u128),
        ("req-2", "read_file", true, 110_u128),
        ("req-3", "list_dir", true, 90_u128),
    ];

    for (request_id, tool_name, success, duration_ms) in events {
        let request = ToolRequest {
            request_id: request_id.into(),
            tool_name: tool_name.into(),
            payload_json: "{\"path\":\"/tmp/demo\"}".into(),
            dry_run: false,
            timeout_ms: Some(100),
        };
        let audit = state.audit_log.record_tool_call(AuditEventInput {
            request_id: request.request_id.clone(),
            tool_name: request.tool_name.clone(),
            dry_run: false,
            policy_decision: "allow".into(),
            policy_reason: "explicit_allow".into(),
            success,
            duration_ms,
            error_message: (!success).then(|| "failure".into()),
        });

        state
            .memory
            .record_tool_activity(&request, &audit, state.config.runtime.critical_delay_ms)
            .expect("record tool activity");
    }

    let report = state
        .memory
        .bottleneck_report("core", 4)
        .expect("bottleneck report");

    assert_eq!(report.len(), 4);

    assert_eq!(report[0].rank, 1);
    assert_eq!(report[0].relation, "dispatches_tool");
    assert_eq!(report[0].from_key, "kernel:core");
    assert_eq!(report[0].to_key, "tool:read_file");
    assert_eq!(report[0].call_count, 2);
    assert_eq!(report[0].critical_delay_count, 2);
    assert_eq!(report[0].total_duration_ms, 360);
    assert_eq!(report[0].avg_duration_ms, 180);
    assert_eq!(report[0].failure_count, 1);

    assert_eq!(report[1].rank, 2);
    assert_eq!(report[1].relation, "returns_status");
    assert_eq!(report[1].from_key, "tool:read_file");
    assert_eq!(report[1].to_key, "status:failure");
    assert_eq!(report[1].critical_delay_count, 1);
    assert_eq!(report[1].total_duration_ms, 250);

    assert_eq!(report[2].rank, 3);
    assert_eq!(report[2].relation, "returns_status");
    assert_eq!(report[2].from_key, "tool:read_file");
    assert_eq!(report[2].to_key, "status:success");
    assert_eq!(report[2].critical_delay_count, 1);
    assert_eq!(report[2].total_duration_ms, 110);

    assert_eq!(report[3].rank, 4);
    assert_eq!(report[3].relation, "dispatches_tool");
    assert_eq!(report[3].from_key, "kernel:core");
    assert_eq!(report[3].to_key, "tool:list_dir");
    assert_eq!(report[3].critical_delay_count, 0);
    assert_eq!(report[3].total_duration_ms, 90);
}

#[test]
fn thousand_cycles_expose_wave_movement() {
    let mut state = test_kernel_state();
    state.memory = KernelMemory::new("core".into(), 4096, 4096);

    for cycle_index in 0..1_000_u64 {
        let phase = cycle_index % 200;
        let (tool_name, duration_ms, success) = if phase < 50 {
            ("file_exists", 35_u128, true)
        } else if phase < 100 {
            ("read_file", 120_u128, true)
        } else if phase < 150 {
            ("list_dir", 260_u128, phase % 10 != 0)
        } else {
            ("read_file", 80_u128, true)
        };

        record_synthetic_cycle(
            &state,
            &SyntheticCycleObservation {
                cycle_index,
                tool_name: tool_name.into(),
                duration_ms,
                success,
            },
        )
        .expect("record synthetic cycle");
    }

    let events = state.audit_log.events();
    assert_eq!(events.len(), 1_000);

    let wave = build_wave_report(&events, 100, state.config.runtime.critical_delay_ms);
    assert_eq!(wave.len(), 10);

    assert_eq!(wave[0].window_index, 0);
    assert_eq!(wave[0].avg_duration_ms, 77);
    assert_eq!(wave[0].critical_delay_count, 50);
    assert_eq!(wave[0].failure_count, 0);
    assert_eq!(wave[0].dominant_tool_name, "read_file");

    assert_eq!(wave[1].window_index, 1);
    assert_eq!(wave[1].avg_duration_ms, 170);
    assert_eq!(wave[1].critical_delay_count, 50);
    assert_eq!(wave[1].failure_count, 5);
    assert_eq!(wave[1].dominant_tool_name, "list_dir");

    assert_eq!(wave[2].window_index, 2);
    assert_eq!(wave[2].avg_duration_ms, 77);
    assert_eq!(wave[2].critical_delay_count, 50);
    assert_eq!(wave[2].dominant_tool_name, "read_file");

    assert_eq!(wave[3].window_index, 3);
    assert_eq!(wave[3].avg_duration_ms, 170);
    assert_eq!(wave[3].critical_delay_count, 50);
    assert_eq!(wave[3].failure_count, 5);
    assert_eq!(wave[3].dominant_tool_name, "list_dir");

    let bottlenecks = state
        .memory
        .bottleneck_report("core", 3)
        .expect("bottleneck report");
    assert_eq!(bottlenecks[0].to_key, "tool:list_dir");
    assert_eq!(bottlenecks[0].critical_delay_count, 250);
    assert_eq!(bottlenecks[0].failure_count, 25);

    let cut_candidates = build_cut_candidate_report(&bottlenecks, &wave, 3);
    assert_eq!(cut_candidates.len(), 3);
    assert_eq!(cut_candidates[0].rank, 1);
    assert_eq!(cut_candidates[0].tool_name, "list_dir");
    assert_eq!(cut_candidates[0].relation, "dispatches_tool");
    assert_eq!(cut_candidates[0].critical_delay_count, 250);
    assert_eq!(cut_candidates[0].dominant_window_count, 5);
    assert_eq!(cut_candidates[0].dominant_window_critical_delay_count, 250);
    assert_eq!(cut_candidates[0].dominant_window_peak_avg_duration_ms, 170);

    let rebalance_plan = build_rebalance_plan(&cut_candidates, "edge", 1);
    assert_eq!(rebalance_plan.len(), 1);
    assert_eq!(rebalance_plan[0].tool_name, "list_dir");
    assert_eq!(rebalance_plan[0].target_sector, "edge");

    let simulation = simulate_sector_rebalance(
        &events,
        "core",
        &rebalance_plan,
        100,
        state.config.runtime.critical_delay_ms,
    );
    assert_eq!(simulation.source_sector_before.sector_id, "core");
    assert_eq!(simulation.source_sector_before.event_count, 1_000);
    assert_eq!(simulation.source_sector_before.avg_duration_ms, 123);
    assert_eq!(simulation.source_sector_before.critical_delay_count, 500);
    assert_eq!(simulation.source_sector_before.failure_count, 25);

    assert_eq!(simulation.sectors_after.len(), 2);
    assert_eq!(simulation.sectors_after[0].sector_id, "core");
    assert_eq!(simulation.sectors_after[0].event_count, 750);
    assert_eq!(simulation.sectors_after[0].avg_duration_ms, 78);
    assert_eq!(simulation.sectors_after[0].critical_delay_count, 250);
    assert_eq!(simulation.sectors_after[0].failure_count, 0);

    assert_eq!(simulation.sectors_after[1].sector_id, "edge");
    assert_eq!(simulation.sectors_after[1].event_count, 250);
    assert_eq!(simulation.sectors_after[1].avg_duration_ms, 260);
    assert_eq!(simulation.sectors_after[1].critical_delay_count, 250);
    assert_eq!(simulation.sectors_after[1].failure_count, 25);
    assert_eq!(simulation.sectors_after[1].wave_windows.len(), 3);

    let capacity = assess_sector_capacity(
        &simulation.sectors_after,
        &[
            SectorCapacityProfile {
                sector_id: "core".into(),
                max_events: 800,
                max_avg_duration_ms: 100,
                max_critical_delay_count: 300,
                max_failure_count: 5,
            },
            SectorCapacityProfile {
                sector_id: "edge".into(),
                max_events: 300,
                max_avg_duration_ms: 200,
                max_critical_delay_count: 200,
                max_failure_count: 20,
            },
        ],
    );
    assert_eq!(capacity.sectors.len(), 2);
    assert_eq!(capacity.sectors[0].sector_id, "core");
    assert!(capacity.sectors[0].within_capacity);
    assert!(capacity.sectors[0].exceeded_dimensions.is_empty());

    assert_eq!(capacity.sectors[1].sector_id, "edge");
    assert!(!capacity.sectors[1].within_capacity);
    assert_eq!(
        capacity.sectors[1].exceeded_dimensions,
        vec![
            "max_avg_duration_ms".to_string(),
            "max_critical_delay_count".to_string(),
            "max_failure_count".to_string()
        ]
    );

    let routed_to_edge = apply_rebalance_policy(
        &request("list_dir", "{\"path\":\"/tmp\"}".into()),
        "core",
        &rebalance_plan,
        &assess_sector_capacity(
            &simulation.sectors_after,
            &[
                SectorCapacityProfile {
                    sector_id: "core".into(),
                    max_events: 800,
                    max_avg_duration_ms: 100,
                    max_critical_delay_count: 300,
                    max_failure_count: 5,
                },
                SectorCapacityProfile {
                    sector_id: "edge".into(),
                    max_events: 300,
                    max_avg_duration_ms: 300,
                    max_critical_delay_count: 300,
                    max_failure_count: 30,
                },
            ],
        ),
    );
    assert_eq!(routed_to_edge.source_sector, "core");
    assert_eq!(routed_to_edge.target_sector, "edge");
    assert_eq!(routed_to_edge.reason, RouteReason::RebalanceApplied);

    let fallback_to_core = apply_rebalance_policy(
        &request("list_dir", "{\"path\":\"/tmp\"}".into()),
        "core",
        &rebalance_plan,
        &capacity,
    );
    assert_eq!(fallback_to_core.source_sector, "core");
    assert_eq!(fallback_to_core.target_sector, "core");
    assert_eq!(fallback_to_core.reason, RouteReason::TargetOverCapacity);

    let no_assignment = apply_rebalance_policy(
        &request("read_file", "{\"path\":\"/tmp/x\"}".into()),
        "core",
        &rebalance_plan,
        &capacity,
    );
    assert_eq!(no_assignment.source_sector, "core");
    assert_eq!(no_assignment.target_sector, "core");
    assert_eq!(no_assignment.reason, RouteReason::NoAssignment);

    let default_registry = SectorRegistry::default_for_builtin_tools("core");
    let handoff = prepare_handoff(
        &request("list_dir", "{\"path\":\"/tmp\"}".into()),
        &routed_to_edge,
        &default_registry,
    );
    assert!(handoff.receipt.accepted);
    assert_eq!(
        handoff.receipt.reason,
        HandoffReason::HandoffPrepared.as_str()
    );
    let handoff_request = handoff.handoff.expect("handoff request");
    assert_eq!(handoff_request.source_sector, "core");
    assert_eq!(handoff_request.target_sector, "edge");
    assert_eq!(handoff_request.tool_name, "list_dir");

    let local_handoff = prepare_handoff(
        &request("read_file", "{\"path\":\"/tmp/x\"}".into()),
        &no_assignment,
        &default_registry,
    );
    assert!(!local_handoff.receipt.accepted);
    assert_eq!(
        local_handoff.receipt.reason,
        HandoffReason::LocalExecution.as_str()
    );
    assert!(local_handoff.handoff.is_none());

    let unsupported_registry = SectorRegistry::new(vec![
        SectorDescriptor {
            sector_id: "core".into(),
            enabled: true,
            accepts_handoffs: false,
            supported_tools: vec!["read_file".into(), "list_dir".into(), "file_exists".into()],
        },
        SectorDescriptor {
            sector_id: "edge".into(),
            enabled: true,
            accepts_handoffs: true,
            supported_tools: vec!["file_exists".into()],
        },
    ]);
    let unsupported_handoff = prepare_handoff(
        &request("list_dir", "{\"path\":\"/tmp\"}".into()),
        &routed_to_edge,
        &unsupported_registry,
    );
    assert!(!unsupported_handoff.receipt.accepted);
    assert_eq!(
        unsupported_handoff.receipt.reason,
        HandoffReason::ToolUnsupportedInTarget.as_str()
    );
    assert!(unsupported_handoff.handoff.is_none());
}
