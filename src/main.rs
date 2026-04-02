use kernel::{load_config, load_tool_policy, validate_config, validate_tool_policy};

fn main() {
    let cfg = match load_config("config/eve.toml") {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("config load failed: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = validate_config(&cfg) {
        eprintln!("config validation failed: {err}");
        std::process::exit(1);
    }

    let tool_policy = match load_tool_policy(&cfg.tools.policy_path) {
        Ok(policy) => policy,
        Err(err) => {
            eprintln!("tool policy load failed: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = validate_tool_policy(&tool_policy) {
        eprintln!("tool policy validation failed: {err}");
        std::process::exit(1);
    }

    println!("boot: {}", cfg.eve.name);
    println!("version: {}", cfg.eve.version);
    println!("platform: {}", cfg.eve.platform);
    println!("mode: {}", cfg.eve.mode);
    println!("tools_enabled: {}", cfg.tools.enabled);
    println!("tool_policy_default: {}", tool_policy.policy.default_action);
    println!("registered_tools: {}", tool_policy.tools.len());
}
