use crate::config::{Config, Session, SplitDirection, Window};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Fixed poll interval for shell readiness checks — not a UI delay.
const SHELL_POLL_MS: u64 = 50;

pub fn open_session(path: &Path, config: &Config, debug: bool) -> Result<(), Box<dyn Error>> {
    let session_name = path
        .file_name()
        .ok_or("Could not get directory name")?
        .to_string_lossy()
        .replace('.', "_");

    if debug {
        println!("Opening session: {session_name}");
    }

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
                create_session_from_config(&session_name, path, config, session_config, debug)
            {
                // Kill the partially-created session so the next run starts clean.
                let _ = Command::new("tmux")
                    .arg("kill-session")
                    .arg("-t")
                    .arg(&session_name)
                    .status();
                return Err(e);
            }
        } else {
            let mut cmd = Command::new("tmux");
            cmd.arg("new-session")
                .arg("-d")
                .arg("-s")
                .arg(&session_name)
                .arg("-c")
                .arg(path.to_string_lossy().as_ref());
            cmd.status()?;
        }
    }

    if std::env::var("TMUX").is_ok() {
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

fn get_command_output(mut cmd: Command, debug: bool) -> Result<String, Box<dyn Error>> {
    if debug {
        println!("Executing for output: {cmd:?}");
    }
    let output = cmd.output()?;
    if !output.status.success() {
        return Err(format!(
            "Tmux command failed: {:?}",
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
}

/// Split percentage for step `i` in a chain-split, clamped to [1, 99].
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
    debug: bool,
) -> Result<(), Box<dyn Error>> {
    let mut exec_tasks = Vec::new();

    for (win_idx, window_name) in session_config.windows.iter().enumerate() {
        let window_config = config.windows.iter().find(|w| &w.name == window_name);

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
            get_command_output(cmd, debug)?
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
            get_command_output(cmd, debug)?
        };

        if let Some(wc) = window_config {
            if wc.panes.is_empty() {
                continue;
            }

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
                match get_command_output(cmd, debug) {
                    Ok(new_id) => {
                        pane_ids.push(new_id.clone());
                        current_pane_id = new_id;
                    }
                    Err(e) => {
                        eprintln!("seela: split-window failed: {e}");
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
                        debug,
                        &mut exec_tasks,
                        session_name,
                        window_name,
                    )?;
                }
            }
        }
    }

    if !exec_tasks.is_empty() {
        if debug {
            println!(
                "Waiting {}ms for tmux to stabilize...",
                config.tmux.startup_delay_ms
            );
        }
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

                    // Reset terminal state and clear any partial input once, up front.
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
                                            "{} --run-command {} --run-command-label {}; clear",
                                            current_exe.display(),
                                            shell_escape(val),
                                            shell_escape(val),
                                        );
                                    }
                                }
                                "@run" => {
                                    // load-buffer + paste-buffer feeds the command
                                    // directly into the pane's stdin, so it executes
                                    // without being displayed on the terminal at all.
                                    let cmd_str = format!(
                                        "SEELA_SESSION_PATH={} SEELA_SESSION_NAME={} SEELA_WINDOW_NAME={} SEELA_PANE_ID={} {}
",
                                        shell_escape(&task.path.display().to_string()),
                                        shell_escape(&task.session_name),
                                        shell_escape(&task.window_name),
                                        shell_escape(&task.pane_id),
                                        val,
                                    );
                                    wait_for_shell_idle(&task.pane_id, 10_000);
                                    let mut load = Command::new("tmux")
                                        .arg("load-buffer")
                                        .arg("-")
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
                                        .arg("-p")
                                        .arg("-t").arg(&task.pane_id)
                                        .status();
                                    thread::sleep(Duration::from_millis(tmux_cfg.action_delay_ms));
                                    continue;
                                }
                                "@wait" => {
                                    if let Ok(secs) = val.parse::<u64>() {
                                        thread::sleep(Duration::from_secs(secs));
                                    }
                                    continue;
                                }
                                "@wait-milli" | "@wait-ms" => {
                                    if let Ok(ms) = val.parse::<u64>() {
                                        thread::sleep(Duration::from_millis(ms));
                                    }
                                    continue;
                                }
                                "@send-key" | "@sk" => {
                                    thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));
                                    let mut key_cmd = Command::new("tmux");
                                    key_cmd
                                        .arg("send-keys")
                                        .arg("-t")
                                        .arg(&task.pane_id)
                                        .arg(val);
                                    let _ = key_cmd.status();
                                    continue;
                                }
                                _ => {}
                            }
                        }

                        wait_for_shell_idle(&task.pane_id, 10_000);

                        let mut run_cmd = Command::new("tmux");
                        run_cmd
                            .arg("send-keys")
                            .arg("-t")
                            .arg(&task.pane_id)
                            .arg("-l")
                            .arg(&final_cmd);
                        let _ = run_cmd.status();

                        thread::sleep(Duration::from_millis(tmux_cfg.key_delay_ms));

                        let mut enter_cmd = Command::new("tmux");
                        enter_cmd
                            .arg("send-keys")
                            .arg("-t")
                            .arg(&task.pane_id)
                            .arg("C-m");
                        let _ = enter_cmd.status();

                        thread::sleep(Duration::from_millis(tmux_cfg.action_delay_ms));
                    }
                });
            }
        });
    }

    if let Some(focus_name) = &session_config.window_focus {
        Command::new("tmux")
            .arg("select-window")
            .arg("-t")
            .arg(format!("{session_name}:{focus_name}"))
            .status()?;
    }

    Ok(())
}

/// Wraps `s` in single quotes, escaping any single quotes within.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn setup_pane(
    pane_id: &str,
    config: &crate::config::Pane,
    path: &Path,
    debug: bool,
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
            match get_command_output(cmd, debug) {
                Ok(new_id) => {
                    sub_pane_ids.push(new_id.clone());
                    current_pane_id = new_id;
                }
                Err(e) => {
                    eprintln!("seela: split-window failed: {e}");
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
                    debug,
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
        });
    }

    Ok(())
}
