use crate::config::{Config, Pane, Session, SplitDirection, Window};
use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};
use tracing::{debug, error, trace, warn};

const SHELL_POLL_MS: u64 = 50;

pub fn open_session(path: &Path, config: &Config, config_dir: &Path) -> Result<(), Box<dyn Error>> {
    let session_name = path
        .file_name()
        .ok_or("Could not get directory name")?
        .to_string_lossy()
        .replace('.', "_");

    debug!("opening session: {session_name}");

    let status = Command::new("tmux")
        .arg("has-session")
        .arg("-t")
        .arg(&session_name)
        .stderr(std::process::Stdio::null())
        .status();

    let session_exists = match status {
        Ok(s) => s.success(),
        Err(_) => false,
    };

    if !session_exists {
        if let Some(session_config) = config.get_session_for_path(path) {
            if let Err(e) =
                create_session_from_config(&session_name, path, config, session_config, config_dir)
            {
                error!("session creation failed: {e}");
                let _ = Command::new("tmux")
                    .arg("kill-session")
                    .arg("-t")
                    .arg(&session_name)
                    .status();
                return Err(e);
            }
        } else {
            debug!(
                "no session config found for {}, creating plain session",
                path.display()
            );
            Command::new("tmux")
                .arg("new-session")
                .arg("-d")
                .arg("-s")
                .arg(&session_name)
                .arg("-c")
                .arg(path.to_string_lossy().as_ref())
                .status()?;
        }
    } else {
        debug!("session {session_name} already exists, attaching");
    }

    if env::var("TMUX").is_ok() {
        Command::new("tmux")
            .arg("switch-client")
            .arg("-t")
            .arg(&session_name)
            .status()?;
    } else {
        Command::new("tmux")
            .arg("attach-session")
            .arg("-t")
            .arg(&session_name)
            .status()?;
    }

    Ok(())
}

fn get_command_output(mut cmd: Command) -> Result<String, Box<dyn Error>> {
    trace!("executing: {cmd:?}");
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(format!(
            "tmux command failed: {:?}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn pane_current_command(pane_id: &str) -> Option<String> {
    let out = Command::new("tmux")
        .arg("display-message")
        .arg("-t")
        .arg(pane_id)
        .arg("-p")
        .arg("#{pane_current_command}")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn is_shell(cmd: &str) -> bool {
    matches!(
        cmd,
        "bash" | "zsh" | "fish" | "sh" | "dash" | "ksh" | "tcsh" | "csh" | "nu"
    )
}

fn wait_for_shell_idle(pane_id: &str, timeout_ms: u64) -> bool {
    let mut elapsed = 0u64;
    loop {
        if pane_current_command(pane_id).is_some_and(|c| is_shell(&c)) {
            return true;
        }
        if elapsed >= timeout_ms {
            warn!("timed out waiting for shell idle on pane {pane_id}");
            return false;
        }
        thread::sleep(Duration::from_millis(SHELL_POLL_MS));
        elapsed += SHELL_POLL_MS;
    }
}

struct ExecTask {
    pane_id: String,
    commands: Vec<String>,
    path: PathBuf,
    session_name: String,
    window_name: String,
    config_dir: PathBuf,
}

fn split_percentage(ratios: &[f32], i: usize, remaining_ratio: f32) -> u32 {
    let next_sum: f32 = ratios[i + 1..].iter().sum();
    let pct = (next_sum / remaining_ratio) * 100.0;
    (pct.round() as u32).clamp(1, 99)
}

/// `-h` (side-by-side) by default; `-v` (stacked) if first pane sets `split = "horizontal"`.
fn window_split_arg(wc: &Window) -> &'static str {
    match wc.panes.first().and_then(|p| p.split) {
        Some(SplitDirection::Horizontal) => "-v",
        _ => "-h",
    }
}

fn create_session_from_config(
    session_name: &str,
    path: &Path,
    config: &Config,
    session_config: &Session,
    config_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut exec_tasks = Vec::new();
    let mut hook_handles: Vec<thread::JoinHandle<()>> = Vec::new();

    for (win_idx, window_name) in session_config.windows.iter().enumerate() {
        let window_config = config.windows.iter().find(|w| &w.name == window_name);

        if window_config.is_none() {
            trace!(
                "window '{}' referenced in session but not defined — creating empty window",
                window_name
            );
        }

        let root_pane_id = if win_idx == 0 {
            let mut cmd = Command::new("tmux");
            cmd.arg("new-session")
                .arg("-d")
                .arg("-s")
                .arg(session_name)
                .arg("-n")
                .arg(window_name)
                .arg("-c")
                .arg(path.to_string_lossy().as_ref())
                .arg("-P")
                .arg("-F")
                .arg("#{pane_id}");
            get_command_output(cmd)?
        } else {
            let mut cmd = Command::new("tmux");
            cmd.arg("new-window")
                .arg("-a")
                .arg("-t")
                .arg(session_name)
                .arg("-n")
                .arg(window_name)
                .arg("-c")
                .arg(path.to_string_lossy().as_ref())
                .arg("-P")
                .arg("-F")
                .arg("#{pane_id}");
            get_command_output(cmd)?
        };

        debug!("created window '{window_name}' with root pane {root_pane_id}");

        if let Some(wc) = window_config {
            // A window with an empty panes list gets one plain shell pane.
            if wc.panes.is_empty() {
                debug!("window '{window_name}' has no panes defined, leaving as single shell");
                // root_pane_id already exists as a shell, nothing to do.
            } else {
                let split_arg = window_split_arg(wc);
                let mut pane_ids = vec![root_pane_id.clone()];
                let ratios: Vec<f32> = wc.panes.iter().map(|p| p.ratio.unwrap_or(1.0)).collect();
                let mut remaining_ratio: f32 = ratios.iter().sum();

                let mut current_pane_id = root_pane_id.clone();
                for i in 0..wc.panes.len().saturating_sub(1) {
                    let percentage = split_percentage(&ratios, i, remaining_ratio);
                    let mut cmd = Command::new("tmux");
                    cmd.arg("split-window")
                        .arg("-d")
                        .arg(split_arg)
                        .arg("-l")
                        .arg(format!("{}%", percentage))
                        .arg("-t")
                        .arg(&current_pane_id)
                        .arg("-c")
                        .arg(path.to_string_lossy().as_ref())
                        .arg("-P")
                        .arg("-F")
                        .arg("#{pane_id}");
                    match get_command_output(cmd) {
                        Ok(new_id) => {
                            trace!("split pane {new_id} from {current_pane_id}");
                            pane_ids.push(new_id.clone());
                            current_pane_id = new_id;
                        }
                        Err(e) => {
                            error!("split-window failed: {e}");
                            break;
                        }
                    }
                    remaining_ratio -= ratios[i];
                }

                for (i, pane_config) in wc.panes.iter().enumerate() {
                    if i < pane_ids.len() {
                        setup_pane(
                            &pane_ids[i],
                            pane_config,
                            path,
                            config_dir,
                            &mut exec_tasks,
                            session_name,
                            window_name,
                        )?;
                    }
                }
            }

            if !wc.hooks.is_empty() {
                let hooks = wc.hooks.clone();
                let parallel = wc.hooks_parallel;
                let session_name_s = session_name.to_string();
                let window_name_s = window_name.to_string();
                let path_s = path.to_path_buf();
                let config_dir_s = config_dir.to_path_buf();
                debug!(
                    "spawning {} hook(s) for window '{window_name}' (parallel={parallel})",
                    hooks.len()
                );
                hook_handles.push(thread::spawn(move || {
                    run_window_hooks(
                        &hooks,
                        parallel,
                        &session_name_s,
                        &window_name_s,
                        &path_s,
                        &config_dir_s,
                    );
                }));
            }
        }
    }

    for handle in hook_handles {
        let _ = handle.join();
    }

    if !exec_tasks.is_empty() {
        debug!(
            "waiting {}ms for tmux to stabilize before sending execs",
            config.tmux.startup_delay_ms
        );
        thread::sleep(Duration::from_millis(config.tmux.startup_delay_ms));

        let tmux_cfg = config.tmux.clone();

        thread::scope(|s| {
            for (task_idx, task) in exec_tasks.into_iter().enumerate() {
                let tmux_cfg = tmux_cfg.clone();
                s.spawn(move || {
                    thread::sleep(Duration::from_millis(task_idx as u64 * 25));

                    let idle = wait_for_shell_idle(&task.pane_id, 10_000);
                    if !idle {
                        let mut cmd = Command::new("tmux");
                        cmd.arg("send-keys").arg("-t").arg(&task.pane_id).arg("C-c");
                        let _ = cmd.status();
                        thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));
                    }

                    let mut reset_cmd = Command::new("tmux");
                    reset_cmd.arg("send-keys").arg("-t").arg(&task.pane_id).arg("-R");
                    let _ = reset_cmd.status();

                    let mut clear_cmd = Command::new("tmux");
                    clear_cmd.arg("send-keys").arg("-t").arg(&task.pane_id).arg("C-u");
                    let _ = clear_cmd.status();

                    thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));

                    for exec_cmd in task.commands.iter() {
                        let mut final_cmd = exec_cmd.clone();
                        let trimmed = exec_cmd.trim();

                        if trimmed.is_empty() {
                            continue;
                        }

                        if let Some((keyword, val)) = trimmed.split_once(' ') {
                            match keyword {
                                "@confirm" => {
                                    if let Ok(current_exe) = std::env::current_exe() {
                                        final_cmd = format!(
                                            "{} --run-command {}",
                                            current_exe.display(),
                                            shell_escape(val),
                                        );
                                    }
                                }
                                "@run" => {
                                    let expanded_val = {
                                        let mut parts = val.splitn(2, ' ');
                                        let script = expand_exec_path(
                                            parts.next().unwrap_or(""),
                                            &task.config_dir,
                                        );
                                        let rest = parts.next().unwrap_or("");
                                        if rest.is_empty() { script } else { format!("{script} {rest}") }
                                    };
                                    let cmd_str = format!(
                                        "SEELA_SESSION_PATH={} SEELA_SESSION_NAME={} SEELA_WINDOW_NAME={} SEELA_PANE_ID={} {}\n",
                                        shell_escape(&task.path.display().to_string()),
                                        shell_escape(&task.session_name),
                                        shell_escape(&task.window_name),
                                        shell_escape(&task.pane_id),
                                        expanded_val,
                                    );
                                    trace!("@run: {}", expanded_val);
                                    wait_for_shell_idle(&task.pane_id, 10_000);
                                    let mut load = Command::new("tmux")
                                        .arg("load-buffer").arg("-")
                                        .stdin(std::process::Stdio::piped())
                                        .spawn()
                                        .expect("tmux load-buffer failed");
                                    if let Some(mut stdin) = load.stdin.take() {
                                        use std::io::Write;
                                        let _ = stdin.write_all(cmd_str.as_bytes());
                                    }
                                    let _ = load.wait();
                                    let _ = Command::new("tmux")
                                        .arg("paste-buffer")
                                        .arg("-t").arg(&task.pane_id)
                                        .status();
                                    thread::sleep(Duration::from_millis(tmux_cfg.action_delay_ms));
                                    continue;
                                }
                                "@wait" => {
                                    if let Ok(secs) = val.parse::<u64>() {
                                        trace!("@wait {secs}s");
                                        thread::sleep(Duration::from_secs(secs));
                                    }
                                    continue;
                                }
                                "@wait-milli" | "@wait-ms" => {
                                    if let Ok(ms) = val.parse::<u64>() {
                                        trace!("@wait-ms {ms}ms");
                                        thread::sleep(Duration::from_millis(ms));
                                    }
                                    continue;
                                }
                                "@send-key" | "@sk" => {
                                    trace!("@sk {val}");
                                    thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));
                                    let mut key_cmd = Command::new("tmux");
                                    key_cmd
                                        .arg("send-keys")
                                        .arg("-t").arg(&task.pane_id)
                                        .arg(val);
                                    let _ = key_cmd.status();
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        trace!("exec: {trimmed}");
                        wait_for_shell_idle(&task.pane_id, 10_000);

                        let mut run_cmd = Command::new("tmux");
                        run_cmd
                            .arg("send-keys")
                            .arg("-t").arg(&task.pane_id)
                            .arg("-l").arg(&final_cmd);
                        let _ = run_cmd.status();

                        thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));

                        let mut enter_cmd = Command::new("tmux");
                        enter_cmd
                            .arg("send-keys")
                            .arg("-t").arg(&task.pane_id)
                            .arg("C-m");
                        let _ = enter_cmd.status();

                        thread::sleep(Duration::from_millis(tmux_cfg.action_delay_ms));
                    }
                });
            }
        });
    }

    if let Some(focus_name) = &session_config.window_focus {
        debug!("focusing window '{focus_name}'");
        Command::new("tmux")
            .arg("select-window")
            .arg("-t")
            .arg(format!("{session_name}:{focus_name}"))
            .status()?;
    }

    Ok(())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Expands `~` and resolves relative paths against the config file directory.
/// Plain command names (no `/`) are left untouched so PATH lookup still works.
fn expand_exec_path(s: &str, config_dir: &Path) -> String {
    let expanded = shellexpand::tilde(s);
    let p = Path::new(expanded.as_ref());
    if p.is_absolute() {
        p.to_string_lossy().into_owned()
    } else if s.contains('/') {
        config_dir.join(p).to_string_lossy().into_owned()
    } else {
        s.to_string()
    }
}

fn run_hook(script: &str, session_name: &str, window_name: &str, path: &Path, config_dir: &Path) {
    let expanded = {
        let mut parts = script.splitn(2, ' ');
        let cmd = expand_exec_path(parts.next().unwrap_or(""), config_dir);
        let rest = parts.next().unwrap_or("");
        if rest.is_empty() {
            cmd
        } else {
            format!("{cmd} {rest}")
        }
    };
    let cmd_str = format!(
        "SEELA_SESSION_PATH={} SEELA_SESSION_NAME={} SEELA_WINDOW_NAME={} {}",
        shell_escape(&path.display().to_string()),
        shell_escape(session_name),
        shell_escape(window_name),
        expanded,
    );
    trace!("running hook: {script}");
    match Command::new("sh").arg("-c").arg(&cmd_str).output() {
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stderr = stderr.trim();
            if !stderr.is_empty() {
                error!("hook '{}' failed ({}): {}", script, out.status, stderr);
            } else {
                error!("hook '{}' failed ({})", script, out.status);
            }
        }
        Err(e) => error!("hook '{}' could not run: {}", script, e),
        _ => debug!("hook '{}' completed successfully", script),
    }
}

fn run_window_hooks(
    hooks: &[String],
    parallel: bool,
    session_name: &str,
    window_name: &str,
    path: &Path,
    config_dir: &Path,
) {
    if parallel {
        thread::scope(|s| {
            for hook in hooks {
                let hook = hook.clone();
                let session_name = session_name.to_string();
                let window_name = window_name.to_string();
                let path = path.to_path_buf();
                let config_dir = config_dir.to_path_buf();
                s.spawn(move || run_hook(&hook, &session_name, &window_name, &path, &config_dir));
            }
        });
    } else {
        for hook in hooks {
            run_hook(hook, session_name, window_name, path, config_dir);
        }
    }
}

fn setup_pane(
    pane_id: &str,
    config: &Pane,
    path: &Path,
    config_dir: &Path,
    exec_tasks: &mut Vec<ExecTask>,
    session_name: &str,
    window_name: &str,
) -> Result<(), Box<dyn Error>> {
    if !config.panes.is_empty() {
        let mut sub_pane_ids = vec![pane_id.to_string()];

        let split_arg = match config.split {
            Some(SplitDirection::Vertical) => "-h",
            _ => "-v",
        };

        let ratios: Vec<f32> = config
            .panes
            .iter()
            .map(|p| p.ratio.unwrap_or(1.0))
            .collect();
        let mut remaining_ratio: f32 = ratios.iter().sum();

        let mut current_pane_id = pane_id.to_string();
        for i in 0..config.panes.len().saturating_sub(1) {
            let percentage = split_percentage(&ratios, i, remaining_ratio);
            let mut cmd = Command::new("tmux");
            cmd.arg("split-window")
                .arg("-d")
                .arg(split_arg)
                .arg("-l")
                .arg(format!("{}%", percentage))
                .arg("-t")
                .arg(&current_pane_id)
                .arg("-c")
                .arg(path.to_string_lossy().as_ref())
                .arg("-P")
                .arg("-F")
                .arg("#{pane_id}");
            match get_command_output(cmd) {
                Ok(new_id) => {
                    trace!("split pane {new_id} from {current_pane_id}");
                    sub_pane_ids.push(new_id.clone());
                    current_pane_id = new_id;
                }
                Err(e) => {
                    error!("split-window failed: {e}");
                    break;
                }
            }
            remaining_ratio -= ratios[i];
        }

        for (i, sub_pane_config) in config.panes.iter().enumerate() {
            if i < sub_pane_ids.len() {
                setup_pane(
                    &sub_pane_ids[i],
                    sub_pane_config,
                    path,
                    config_dir,
                    exec_tasks,
                    session_name,
                    window_name,
                )?;
            }
        }
    } else if let Some(execs) = &config.exec
        && !execs.is_empty()
    {
        exec_tasks.push(ExecTask {
            pane_id: pane_id.to_string(),
            commands: execs.clone(),
            path: path.to_path_buf(),
            session_name: session_name.to_string(),
            window_name: window_name.to_string(),
            config_dir: config_dir.to_path_buf(),
        });
    }

    Ok(())
}
