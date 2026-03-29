use crate::{config::FzfConfig, run::check_binary};
use std::{
    error::Error,
    io::Write,
    process::{Command, Stdio},
};
use tracing::warn;

pub fn select_project(
    projects: &[String],
    config: &FzfConfig,
) -> Result<Option<String>, Box<dyn Error>> {
    if !check_binary("fzf") {
        return Err("fzf not found in PATH — please install fzf".into());
    }

    let mut cmd = Command::new("fzf");

    if config.preview {
        let binary = config
            .preview_command
            .split_whitespace()
            .next()
            .unwrap_or("");
        if check_binary(binary) {
            cmd.arg("--preview").arg(&config.preview_command);
        } else if !binary.is_empty() {
            warn!("preview command '{}' not found, falling back to ls", binary);
            cmd.arg("--preview").arg("ls {}");
        }
    }

    if let Some(opts) = &config.fzf_opts {
        for opt in opts.split_whitespace() {
            cmd.arg(opt);
        }
    }

    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;

    let mut stdin = child.stdin.take().ok_or("Failed to open fzf stdin")?;
    for project in projects {
        writeln!(stdin, "{}", project)?;
    }
    drop(stdin);

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Ok(None);
    }

    let selected = String::from_utf8(output.stdout)?.trim().to_string();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}
