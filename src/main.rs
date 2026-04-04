use kernel::{boot_kernel, validate_tool_registry_alignment};

fn main() {
    let kernel = match boot_kernel() {
        Ok(kernel) => kernel,
        Err(err) => {
            eprintln!("kernel boot failed: {err}");
            std::process::exit(1);
        },
    };

    if let Err(err) = validate_tool_registry_alignment(&kernel) {
        eprintln!("tool registry validation failed: {err}");
        std::process::exit(1);
    }

    println!("boot: {}", kernel.config.eve.name);
    println!("version: {}", kernel.config.eve.version);
    println!("platform: {}", kernel.config.eve.platform);
    println!("mode: {}", kernel.config.eve.mode);
    println!("sector: {}", kernel.config.eve.sector);
    println!("tools_enabled: {}", kernel.config.tools.enabled);
    println!(
        "tool_policy_default: {}",
        kernel.tool_policy.policy.default_action
    );
    println!("registered_tools: {}", kernel.tool_policy.tools.len());
    println!(
        "memory_limits: nodes={}, edges={}",
        kernel.config.memory.max_nodes_per_sector, kernel.config.memory.max_edges_per_sector
    );
    println!(
        "critical_delay_ms: {}",
        kernel.config.runtime.critical_delay_ms
    );
}
