use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to a config file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Open a directory as tmux session, config will still apply
    #[arg(value_name = "DIR")]
    pub dir: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub headless: bool,

    #[arg(long, hide = true)]
    pub run_command: Option<String>,
}
