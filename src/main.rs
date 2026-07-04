use clap::Parser;
use std::error::Error;
use tracing::{Level, error, trace};

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
        let _guard = init(Level::WARN);
        return run_confirm(&cmd);
    }

    if let Some(opts) = args.run_option {
        let _guard = init(Level::WARN);
        return crate::run::run_option_prompt(&opts);
    }

    let (cfg, config_dir) = load_config(args.config.clone()).map_err(|e| {
        eprintln!("seela: {e}");
        e
    })?;

    let _guard = init(cfg.log.level);
    trace!("config loaded: {cfg:#?}");

    if let Err(e) = run(&cfg, &config_dir, args) {
        error!("{e}");
        return Err(e);
    }

    Ok(())
}
