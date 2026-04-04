use serde::Deserialize;
use std::fs;
use std::path::Path;

pub mod audit;
pub mod contracts;
pub mod cycles;
pub mod error;
pub mod handoff;
pub mod memory;
pub mod policy;
pub mod routing;
pub mod runtime;
pub mod sector_registry;
pub mod tool_executor;
pub mod tool_registry;
pub mod tool_validator;

#[derive(Debug, Deserialize)]
pub struct EveConfig {
    pub eve: EveSection,
    pub logging: LoggingSection,
    pub runtime: RuntimeSection,
    pub tools: ToolsSection,
    pub model: ModelSection,
    pub memory: MemorySection,
}

#[derive(Debug, Deserialize)]
pub struct EveSection {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub mode: String,
    pub sector: String,
}

#[derive(Debug, Deserialize)]
pub struct LoggingSection {
    pub level: String,
    pub format: String,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeSection {
    pub deterministic: bool,
    pub max_cycles: u32,
    pub critical_delay_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ToolsSection {
    pub enabled: bool,
    pub policy_path: String,
}

#[derive(Debug, Deserialize)]
pub struct ModelSection {
    pub enabled: bool,
    pub json_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct MemorySection {
    pub sector_id: String,
    pub max_nodes_per_sector: usize,
    pub max_edges_per_sector: usize,
}

#[derive(Debug, Deserialize)]
pub struct ToolPolicyConfig {
    pub policy: PolicySection,
    pub throttle: ThrottleSection,
    pub tools: Vec<ToolEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PolicySection {
    pub default_action: String,
    pub require_json_contract: bool,
    pub log_all_calls: bool,
}

#[derive(Debug, Deserialize)]
pub struct ThrottleSection {
    pub enabled: bool,
    pub window_size: usize,
    pub candidate_limit: usize,
    pub critical_delay_threshold: u64,
    pub failure_threshold: u64,
    pub exempt_dry_run: bool,
}

#[derive(Debug, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub enabled: bool,
    pub side_effects: bool,
}

pub fn load_config(path: impl AsRef<Path>) -> Result<EveConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let cfg: EveConfig = toml::from_str(&raw)?;
    Ok(cfg)
}

pub fn load_tool_policy(
    path: impl AsRef<Path>,
) -> Result<ToolPolicyConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let cfg: ToolPolicyConfig = toml::from_str(&raw)?;
    Ok(cfg)
}

pub fn validate_config(cfg: &EveConfig) -> Result<(), String> {
    if cfg.eve.name.trim().is_empty() {
        return Err("eve.name is empty".into());
    }

    if cfg.eve.version.trim().is_empty() {
        return Err("eve.version is empty".into());
    }

    if cfg.eve.platform.trim().is_empty() {
        return Err("eve.platform is empty".into());
    }

    if cfg.eve.mode.trim().is_empty() {
        return Err("eve.mode is empty".into());
    }

    if cfg.eve.sector.trim().is_empty() {
        return Err("eve.sector is empty".into());
    }

    if cfg.logging.level.trim().is_empty() {
        return Err("logging.level is empty".into());
    }

    if cfg.logging.format.trim().is_empty() {
        return Err("logging.format is empty".into());
    }

    if cfg.runtime.max_cycles == 0 {
        return Err("runtime.max_cycles must be > 0".into());
    }

    if cfg.runtime.critical_delay_ms == 0 {
        return Err("runtime.critical_delay_ms must be > 0".into());
    }

    if cfg.tools.enabled && cfg.tools.policy_path.trim().is_empty() {
        return Err("tools.policy_path is empty while tools are enabled".into());
    }

    if cfg.memory.sector_id.trim().is_empty() {
        return Err("memory.sector_id is empty".into());
    }

    if cfg.memory.sector_id != cfg.eve.sector {
        return Err("memory.sector_id must match eve.sector".into());
    }

    if cfg.memory.max_nodes_per_sector == 0 {
        return Err("memory.max_nodes_per_sector must be > 0".into());
    }

    if cfg.memory.max_edges_per_sector == 0 {
        return Err("memory.max_edges_per_sector must be > 0".into());
    }

    Ok(())
}

pub fn validate_tool_policy(cfg: &ToolPolicyConfig) -> Result<(), String> {
    let action = cfg.policy.default_action.trim();

    if action.is_empty() {
        return Err("policy.default_action is empty".into());
    }

    if action != "allow" && action != "deny" {
        return Err("policy.default_action must be 'allow' or 'deny'".into());
    }

    if cfg.tools.is_empty() {
        return Err("tools list is empty".into());
    }

    if cfg.throttle.enabled {
        if cfg.throttle.window_size == 0 {
            return Err("throttle.window_size must be > 0".into());
        }

        if cfg.throttle.candidate_limit == 0 {
            return Err("throttle.candidate_limit must be > 0".into());
        }

        if cfg.throttle.critical_delay_threshold == 0 {
            return Err("throttle.critical_delay_threshold must be > 0".into());
        }

        if cfg.throttle.failure_threshold == 0 {
            return Err("throttle.failure_threshold must be > 0".into());
        }
    }

    for tool in &cfg.tools {
        if tool.name.trim().is_empty() {
            return Err("tool.name is empty".into());
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct KernelState {
    pub config: EveConfig,
    pub tool_policy: ToolPolicyConfig,
    pub audit_log: audit::AuditLog,
    pub memory: memory::KernelMemory,
}

pub fn boot_kernel() -> Result<KernelState, Box<dyn std::error::Error>> {
    let config = load_config("config/eve.toml")?;
    validate_config(&config).map_err(|e| format!("config validation failed: {e}"))?;

    let tool_policy = load_tool_policy(&config.tools.policy_path)?;
    validate_tool_policy(&tool_policy)
        .map_err(|e| format!("tool policy validation failed: {e}"))?;

    Ok(KernelState {
        memory: memory::KernelMemory::new(
            config.memory.sector_id.clone(),
            config.memory.max_nodes_per_sector,
            config.memory.max_edges_per_sector,
        ),
        config,
        tool_policy,
        audit_log: audit::AuditLog::new(),
    })
}

pub fn validate_tool_registry_alignment(state: &KernelState) -> Result<(), String> {
    let builtins = tool_registry::builtin_tools();

    for policy_tool in &state.tool_policy.tools {
        let found = builtins.iter().find(|tool| tool.name == policy_tool.name);

        let builtin = match found {
            Some(tool) => tool,
            None => {
                return Err(format!(
                    "tool '{}' exists in policy but not in builtin registry",
                    policy_tool.name
                ));
            },
        };

        if builtin.side_effects != policy_tool.side_effects {
            return Err(format!(
                "tool '{}' side_effects mismatch: policy={}, builtin={}",
                policy_tool.name, policy_tool.side_effects, builtin.side_effects
            ));
        }
    }

    Ok(())
}
