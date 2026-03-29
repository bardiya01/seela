use clap::Parser;
use std::error::Error;

mod cli;
mod config;
mod fzf;
mod logging;
mod run;
mod tmux;

fn main() -> Result<(), Box<dyn Error>> {
    let args = cli::Args::parse();

    if let Some(cmd) = args.run_command {
        return run::run_confirm(&cmd);
    }

    let config_path = config::get_config_path(args.config.clone());

    let Some(path) = config_path else {
        eprintln!("seela: no config file found");
        std::process::exit(1);
    };

    let config_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();

    let cfg = match config::Config::load(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("seela: {e}");
            std::process::exit(1);
        }
    };

    let _guard = logging::init(cfg.log.level);

    tracing::debug!("config loaded: {cfg:#?}");

    if let Err(e) = run::run(&cfg, &config_dir, args) {
        tracing::error!("{e}");
        std::process::exit(1);
    }

    Ok(())
}
