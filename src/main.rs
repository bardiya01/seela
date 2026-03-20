use clap::Parser;
use seela::cli;
use seela::config;
use seela::run;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args = cli::Args::parse();

    if let Some(cmd) = args.run_command {
        // Use the label for display if provided, otherwise fall back to the raw command.
        let label = args.run_command_label.as_deref().unwrap_or(&cmd);
        return run::run_confirm(label, &cmd);
    }

    let config_path = config::get_config_path(args.config);

    if let Some(path) = config_path {
        match config::Config::load(path) {
            Ok(cfg) => run::run(&cfg, args.debug, args.headless)?,
            Err(e) => {
                eprintln!("Error loading config: {e}");
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("Error: No config file found in the search paths.");
        std::process::exit(1);
    }

    Ok(())
}
