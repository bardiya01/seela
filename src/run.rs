use crate::config::{Config, expand_path};
use rayon::prelude::*;
use std::error::Error;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub fn run_confirm(label: &str, cmd: &str) -> Result<(), Box<dyn Error>> {
    print!("Run \"{label}\"? [Y/n] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let input = input.trim().to_lowercase();
    if input.is_empty() || input == "y" || input == "yes" {
        let status = Command::new("sh").arg("-c").arg(cmd).status()?;
        if !status.success() {
            eprintln!("Command exited with status: {}", status);
        }
    } else {
        println!("Skipped.");
    }

    Ok(())
}

pub fn find_projects(config: &Config) -> Vec<PathBuf> {
    let search_dirs: Vec<PathBuf> = config
        .folders
        .search_dirs
        .iter()
        .map(|s| expand_path(s))
        .collect();
    let exclude_paths: Vec<PathBuf> = config
        .folders
        .exclude_paths
        .as_ref()
        .unwrap_or(&vec![])
        .iter()
        .map(|s| expand_path(s))
        .collect();
    let force_include: Vec<PathBuf> = config
        .folders
        .force_include
        .as_ref()
        .unwrap_or(&vec![])
        .iter()
        .map(|s| expand_path(s))
        .collect();

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
                    Some(Err(_)) => continue,
                };

                let path = entry.path();
                if !entry.file_type().is_dir() {
                    continue;
                }

                let mut longest_rule_len = 0;
                let mut is_excluded = false;

                for ex in &exclude_paths {
                    if path.starts_with(ex) && ex.as_os_str().len() > longest_rule_len {
                        longest_rule_len = ex.as_os_str().len();
                        is_excluded = true;
                    }
                }

                for s in &search_dirs {
                    if path.starts_with(s) && s.as_os_str().len() >= longest_rule_len {
                        longest_rule_len = s.as_os_str().len();
                        is_excluded = false;
                    }
                }

                if is_excluded {
                    let is_parent_of_search = search_dirs.iter().any(|s| s.starts_with(path));
                    if !is_parent_of_search {
                        it.skip_current_dir();
                    }
                    continue;
                }

                if path.join(".git").exists() {
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

    projects
}

pub fn run(config: &Config, debug: bool, headless: bool) -> Result<(), Box<dyn Error>> {
    if debug {
        println!("Loaded Config: {config:#?}");
    }

    let projects = find_projects(config);
    let project_strings = projects
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<String>>();

    if headless {
        println!("Headless mode enabled. Skipping fzf and tmux.");
        if debug {
            println!("Found {} projects", project_strings.len());
        }
        return Ok(());
    }

    if let Some(selected) = crate::fzf::select_project(&project_strings, &config.fzf)? {
        crate::tmux::open_session(Path::new(&selected), config, debug)?;
    }

    Ok(())
}
