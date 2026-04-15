use clap::Parser;
use std::error::Error;
use tracing::{Level, debug, error};

use crate::{
    config::load_config,
    logging::init,
    run::{run, run_confirm},
};

mod cli;
mod config;
mod fzf;
mod logging;
mod run;
mod tmux;

fn main() -> Result<(), Box<dyn Error>> {
    let args = cli::Args::parse();

    if let Some(cmd) = args.run_command {
        // We still initialize logging even for --run-command so warnings get captured.
        let _guard = init(Level::WARN);
        return run_confirm(&cmd);
    }

    let (cfg, config_dir) = load_config(args.config.clone()).map_err(|e| {
        eprintln!("seela: {e}");
        e
    })?;

    let _guard = init(cfg.log.level);
    debug!("config loaded: {cfg:#?}");

    if let Err(e) = run(&cfg, &config_dir, args) {
        error!("{e}");
        return Err(e);
    }

    Ok(())
}
