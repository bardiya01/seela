use std::{fs, path::PathBuf};
use tracing::{Level, debug};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};
use tracing_subscriber::filter::LevelFilter;

const MAX_LOG_BYTES: u64 = 1024 * 1024; // 1 MB

fn log_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share")
        });
    base.join("seela")
}

fn truncate_if_needed(path: &std::path::Path) {
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAX_LOG_BYTES
    {
        let _ = std::fs::write(path, "");
    }
}

pub fn init(level: Level) -> WorkerGuard {
    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);

    let log_path = dir.join("seela.log");
    truncate_if_needed(&log_path);

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("could not open log file");

    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_filter(env_filter);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(false)
        .with_level(true)
        .without_time()
        .with_filter(LevelFilter::WARN);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .init();

    debug!("log initialized at level: {}", level);

    guard
}
