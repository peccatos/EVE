use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct EveConfig {
    pub eve: EveSection,
    pub logging: LoggingSection,
    pub runtime: RuntimeSection,
    pub tools: ToolsSection,
    pub model: ModelSection,
}

#[derive(Debug, Deserialize)]
pub struct EveSection {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub mode: String,
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
pub struct ToolPolicyConfig {
    pub policy: PolicySection,
    pub tools: Vec<ToolEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PolicySection {
    pub default_action: String,
    pub require_json_contract: bool,
    pub log_all_calls: bool,
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

    if cfg.logging.level.trim().is_empty() {
        return Err("logging.level is empty".into());
    }

    if cfg.logging.format.trim().is_empty() {
        return Err("logging.format is empty".into());
    }

    if cfg.runtime.max_cycles == 0 {
        return Err("runtime.max_cycles must be > 0".into());
    }

    if cfg.tools.enabled && cfg.tools.policy_path.trim().is_empty() {
        return Err("tools.policy_path is empty while tools are enabled".into());
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

    for tool in &cfg.tools {
        if tool.name.trim().is_empty() {
            return Err("tool.name is empty".into());
        }
    }

    Ok(())
}
