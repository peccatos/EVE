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

pub fn load_config(path: impl AsRef<Path>) -> Result<EveConfig, Box<dyn std::error::Error>> {
    let raw = fs::read_to_string(path)?;
    let cfg: EveConfig = toml::from_str(&raw)?;
    Ok(cfg)
}

//
pub fn validate_config(cfg: &EveConfig) -> Result<(), String> {
    if cfg.eve.name.trim().is_empty() {
        return Err("eve.name is empty".into());
    }

    if cfg.eve.version.trim().is_empty() {
        return Err("eve.version is empty".into());
    }

    if cfg.runtime.max_cycles == 0 {
        return Err("runtime.max_cycles must be > 0".into());
    }

    if cfg.tools.enabled && cfg.tools.policy_path.trim().is_empty() {
        return Err("tools.policy_path is empty while tools are enabled".into());
    }

    Ok(())
}
//


