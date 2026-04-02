use kernel::{load_config, validate_config};

fn main() {
    match load_config("config/eve.toml") {
        Ok(cfg) => {
            if let Err(err) = validate_config(&cfg) {
                eprintln!("config validation failed: {err}");
                std::process::exit(1);
            }

            println!("boot: {}", cfg.eve.name);
            println!("version: {}", cfg.eve.version);
            println!("platform: {}", cfg.eve.platform);
            println!("mode: {}", cfg.eve.mode);
        }
        Err(err) => {
            eprintln!("config load failed: {err}");
            std::process::exit(1);
        }
    }
}
