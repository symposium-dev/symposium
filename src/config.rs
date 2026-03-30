use serde::Deserialize;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use tracing::Level;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    #[serde(default = "default_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
        }
    }
}

fn default_level() -> String {
    "info".to_string()
}

thread_local! {
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
}

/// Returns the symposium home directory (~/.symposium), creating it if needed.
pub fn home_dir() -> PathBuf {
    let dir = dirs::home_dir()
        .expect("could not determine home directory")
        .join(".symposium");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the logs directory (~/.symposium/logs), creating it if needed.
pub fn logs_dir() -> PathBuf {
    let dir = home_dir().join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn plugins_dir() -> PathBuf {
    let dir = home_dir().join("plugins");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Returns the path to the config file (~/.symposium/config.toml).
pub fn config_path() -> PathBuf {
    home_dir().join("config.toml")
}

fn load_config() -> Config {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            eprintln!("warning: failed to parse {}: {e}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

fn with_config<T>(f: impl FnOnce(&Config) -> T) -> T {
    CONFIG.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(load_config());
        }
        f(opt.as_ref().unwrap())
    })
}

/// Returns the configured log level.
pub fn log_level() -> Level {
    with_config(|c| match c.logging.level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        other => {
            eprintln!("warning: unknown log level '{other}', defaulting to info");
            Level::INFO
        }
    })
}

/// Initialize logging and config. Call once at startup.
pub fn init() {
    use std::fs::OpenOptions;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;

    let logs = logs_dir();
    let now = chrono::Local::now();
    let filename = now.format("symposium-%Y%m%d-%H%M%S.log").to_string();
    let log_path = logs.join(&filename);

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");

    let level = log_level();
    let filter = EnvFilter::new(level.as_str());

    fmt()
        .with_env_filter(filter)
        .with_writer(file)
        .with_ansi(false)
        .init();
}
