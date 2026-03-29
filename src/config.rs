use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use tracing::Level;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub folders: Folders,
    #[serde(default)]
    pub fzf: FzfConfig,
    #[serde(default)]
    pub tmux: TmuxConfig,
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub custom_sessions: Vec<Session>,
    pub default_session: Option<Session>,
    #[serde(default)]
    pub project_types: Vec<ProjectType>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LogConfig {
    #[serde(
        default = "defaults::log_level",
        deserialize_with = "deserialize_level"
    )]
    pub level: Level,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: defaults::log_level(),
        }
    }
}

fn deserialize_level<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Level, D::Error> {
    let s = String::deserialize(d)?;
    s.parse::<Level>().map_err(|_| {
        serde::de::Error::custom(format!(
            "invalid log level '{}', expected one of: trace, debug, info, warn, error",
            s
        ))
    })
}

#[derive(Debug, Deserialize, Clone)]
pub struct TmuxConfig {
    #[serde(default = "defaults::startup_delay")]
    pub startup_delay_ms: u64,
    #[serde(default = "defaults::key_delay")]
    pub key_delay_ms: u64,
    #[serde(default = "defaults::action_delay")]
    pub action_delay_ms: u64,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            startup_delay_ms: defaults::startup_delay(),
            key_delay_ms: defaults::key_delay(),
            action_delay_ms: defaults::action_delay(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectType {
    pub name: String,
    pub files: Vec<String>,
}

impl ProjectType {
    pub fn matches(&self, path: &Path) -> bool {
        self.files.iter().any(|f| path.join(f).exists())
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Session {
    #[allow(dead_code)]
    pub name: Option<String>,
    pub paths: Option<Vec<String>>,
    pub types: Option<Vec<String>>,
    pub windows: Vec<String>,
    pub window_focus: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Window {
    pub name: String,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub hooks_parallel: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Pane {
    pub split: Option<SplitDirection>,
    pub exec: Option<Vec<String>>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    pub ratio: Option<f32>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FzfConfig {
    #[serde(default = "defaults::preview")]
    pub preview: bool,
    #[serde(default = "defaults::preview_command")]
    pub preview_command: String,
    pub fzf_opts: Option<String>,
}

impl Default for FzfConfig {
    fn default() -> Self {
        Self {
            preview: defaults::preview(),
            preview_command: defaults::preview_command(),
            fzf_opts: None,
        }
    }
}

mod defaults {
    use tracing::Level;

    pub fn preview() -> bool {
        true
    }
    pub fn preview_command() -> String {
        "tree -C -L 2 {}".to_string()
    }
    pub fn startup_delay() -> u64 {
        600
    }
    pub fn key_delay() -> u64 {
        70
    }
    pub fn action_delay() -> u64 {
        200
    }
    pub fn log_level() -> Level {
        Level::WARN
    }
}

pub fn expand_path(path: &str) -> PathBuf {
    let expanded = shellexpand::tilde(path);
    PathBuf::from(expanded.to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("folders.search_dirs must not be empty")]
    EmptySearchDirs,
}

impl Config {
    pub fn load(path: PathBuf) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;

        if config.folders.search_dirs.is_empty() {
            return Err(ConfigError::EmptySearchDirs);
        }

        Ok(config)
    }

    pub fn get_session_for_path(&self, path: &Path) -> Option<&Session> {
        for session in &self.custom_sessions {
            if let Some(paths) = &session.paths {
                for p in paths {
                    let expanded = expand_path(p);
                    if path == expanded {
                        return Some(session);
                    }
                }
            }
        }

        for session in &self.custom_sessions {
            if let Some(types) = &session.types {
                for t_name in types {
                    if let Some(pt) = self.project_types.iter().find(|pt| &pt.name == t_name)
                        && pt.matches(path)
                    {
                        return Some(session);
                    }
                }
            }
        }

        let mut best_match: Option<&Session> = None;
        let mut longest_prefix = 0;

        for session in &self.custom_sessions {
            if let Some(paths) = &session.paths {
                for p in paths {
                    let expanded = expand_path(p);
                    if path.starts_with(&expanded) {
                        let len = expanded.as_os_str().len();
                        if len > longest_prefix {
                            longest_prefix = len;
                            best_match = Some(session);
                        }
                    }
                }
            }
        }

        best_match.or(self.default_session.as_ref())
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Folders {
    pub search_dirs: Vec<String>,
    pub force_include: Option<Vec<String>>,
    pub exclude_paths: Option<Vec<String>>,
}

pub fn get_config_path(cli_path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = cli_path.filter(|p| p.exists()) {
        return Some(path);
    }

    if let Ok(seela_home) = env::var("SEELA_CONFIG_HOME") {
        let path = PathBuf::from(seela_home).join("config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    if let Ok(xdg_home) = env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(xdg_home).join("seela/config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    if let Some(home) = dirs::home_dir() {
        let path = home.join(".config/seela/config.toml");
        if path.exists() {
            return Some(path);
        }
    }

    None
}
