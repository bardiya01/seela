use crate::cli::Args;
use crate::config::{Config, expand_path};
use crate::fzf::select_project;
use crate::tmux::open_session;
use rayon::prelude::*;
use std::error::Error;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub fn run_confirm(cmd: &str) -> Result<(), Box<dyn Error>> {
    print!("Run \"{cmd}\"? [Y/n] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim().to_lowercase();
    if input.is_empty() || input == "y" || input == "yes" {
        let status = Command::new("sh").arg("-c").arg(cmd).status()?;
        if !status.success() {
            tracing::warn!("@confirm command exited with status: {}", status);
        }
    } else {
        println!("Skipped.");
    }

    Ok(())
}

fn is_excluded(path: &Path, exclude_paths: &[PathBuf], search_dirs: &[PathBuf]) -> bool {
    let excluded_by = exclude_paths
        .iter()
        .filter(|ex| path.starts_with(ex.as_path()))
        .max_by_key(|ex| ex.as_os_str().len());

    let Some(exclude_rule) = excluded_by else {
        return false;
    };

    !search_dirs
        .iter()
        .any(|s| path.starts_with(s) && s.as_os_str().len() > exclude_rule.as_os_str().len())
}

fn expand_paths(paths: &[String]) -> Vec<PathBuf> {
    paths.iter().map(|s| expand_path(s)).collect()
}

pub fn find_projects(config: &Config) -> Vec<PathBuf> {
    let search_dirs = expand_paths(&config.folders.search_dirs);
    let exclude_paths = expand_paths(config.folders.exclude_paths.as_deref().unwrap_or(&[]));
    let force_include = expand_paths(config.folders.force_include.as_deref().unwrap_or(&[]));

    // Warn about configured paths that don't exist.
    for dir in &search_dirs {
        if !dir.exists() {
            tracing::warn!("search_dir does not exist: {}", dir.display());
        }
    }
    for p in config.folders.exclude_paths.as_deref().unwrap_or(&[]) {
        let expanded = expand_path(p);
        if !expanded.exists() {
            tracing::warn!("exclude_path does not exist: {}", expanded.display());
        }
    }
    for p in config.folders.force_include.as_deref().unwrap_or(&[]) {
        let expanded = expand_path(p);
        if !expanded.exists() {
            tracing::warn!("force_include path does not exist: {}", expanded.display());
        }
    }

    let mut projects: Vec<PathBuf> = force_include.into_iter().filter(|p| p.exists()).collect();

    let discovered: Vec<PathBuf> = search_dirs
        .par_iter()
        .filter(|root| root.exists())
        .flat_map(|root| {
            let mut it = WalkDir::new(root).into_iter();
            let mut results = Vec::new();

            loop {
                let entry = match it.next() {
                    None => break,
                    Some(Ok(entry)) => entry,
                    Some(Err(e)) => {
                        tracing::warn!("error walking directory: {e}");
                        continue;
                    }
                };

                let path = entry.path();
                if !entry.file_type().is_dir() {
                    continue;
                }

                if is_excluded(path, &exclude_paths, &search_dirs) {
                    let has_search_dir_below = search_dirs.iter().any(|s| s.starts_with(path));
                    if !has_search_dir_below {
                        it.skip_current_dir();
                    }
                    continue;
                }

                if path.join(".git").exists() {
                    tracing::trace!("found project: {}", path.display());
                    results.push(path.to_path_buf());
                    it.skip_current_dir();
                    continue;
                }
            }
            results
        })
        .collect();

    for p in discovered {
        if !projects.contains(&p) {
            projects.push(p);
        }
    }

    tracing::debug!("found {} projects", projects.len());
    projects
}

pub fn run(config: &Config, config_dir: &Path, cli: Args) -> Result<(), Box<dyn Error>> {
    // Check tmux is available before doing anything.
    let tmux_ok = std::process::Command::new("which")
        .arg("tmux")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if !tmux_ok {
        return Err("tmux not found in PATH — please install tmux".into());
    }

    if let Some(path) = cli.dir
        && path.exists()
    {
        open_session(&path, config, config_dir)?;
    } else {
        let projects = find_projects(config);
        let project_strings = projects
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<String>>();

        if project_strings.is_empty() {
            tracing::warn!("no projects found in configured search_dirs");
        }

        if cli.headless {
            tracing::debug!("headless mode, skipping fzf and tmux");
            tracing::debug!("found {} projects", project_strings.len());
            return Ok(());
        }

        if let Some(selected) = select_project(&project_strings, &config.fzf)? {
            open_session(Path::new(&selected), config, config_dir)?;
        }
    }

    Ok(())
}
