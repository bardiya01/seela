use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Enable debug output
    #[arg(long)]
    pub debug: bool,

    /// Disable fzf and tmux (for debugging)
    #[arg(long)]
    pub headless: bool,

    #[arg(long, hide = true)]
    pub run_command: Option<String>,

    #[arg(long, hide = true)]
    pub run_command_label: Option<String>,
}
