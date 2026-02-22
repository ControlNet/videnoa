use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "config.toml";
const ENV_DATA_DIR: &str = "VIDENOA_DATA_DIR";
pub const FALLBACK_LOCALE: &str = "en";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub paths: PathsConfig,
    pub server: ServerConfig,
    pub locale: String,
    pub performance: PerformanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PathsConfig {
    pub models_dir: PathBuf,
    pub trt_cache_dir: PathBuf,
    pub presets_dir: PathBuf,
    pub workflows_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PerformanceConfig {
    pub profiling_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            server: ServerConfig::default(),
            locale: FALLBACK_LOCALE.to_string(),
            performance: PerformanceConfig::default(),
        }
    }
}

pub fn normalize_supported_locale(locale: &str) -> String {
    let normalized = locale.trim().to_ascii_lowercase();

    if normalized.starts_with("zh") {
        "zh-CN".to_string()
    } else if normalized.starts_with("en") {
        "en".to_string()
    } else {
        FALLBACK_LOCALE.to_string()
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("models"),
            trt_cache_dir: PathBuf::from("trt_cache"),
            presets_dir: PathBuf::from("presets"),
            workflows_dir: PathBuf::from("data/workflows"),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "0.0.0.0".to_string(),
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            profiling_enabled: false,
        }
    }
}

impl AppConfig {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;

        if raw.trim().is_empty() {
            return Ok(Self::default());
        }

        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config TOML: {}", path.display()))
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .context("config path does not have a parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;

        let encoded = toml::to_string_pretty(self).context("failed to serialize config TOML")?;
        fs::write(path, encoded)
            .with_context(|| format!("failed to write config file: {}", path.display()))?;

        Ok(())
    }
}

/// Resolve the data directory with 3-tier priority:
/// 1. CLI override if provided
/// 2. VIDENOA_DATA_DIR environment variable
/// 3. Default: ./data
pub fn data_dir(cli_override: Option<&Path>) -> PathBuf {
    if let Some(path) = cli_override {
        return path.to_path_buf();
    }

    if let Some(env_dir) = env::var_os(ENV_DATA_DIR) {
        return PathBuf::from(env_dir);
    }

    PathBuf::from("data")
}

/// Returns the path to config.toml within the given data directory.
pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CONFIG_FILE_NAME)
}

/// Initialize the data directory structure on first run:
/// - Creates data_dir if missing
/// - Writes default config.toml only if file doesn't exist
pub fn initialize_data_dir(data_dir: &Path) -> Result<()> {
    // Create data directory
    if !data_dir.exists() {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("failed to create data directory: {}", data_dir.display()))?;
    }

    // Write default config if doesn't exist
    let cfg_path = config_path(data_dir);
    if !cfg_path.exists() {
        let default_cfg = AppConfig::default();
        default_cfg.save_to_path(&cfg_path)?;
    }

    Ok(())
}

/// Resolve a path relative to a base directory.
/// Returns the path as-is if absolute, otherwise joins it to base.
pub fn resolve_relative_to(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let cfg = AppConfig::default();

        assert_eq!(cfg.paths.models_dir, PathBuf::from("models"));
        assert_eq!(cfg.paths.trt_cache_dir, PathBuf::from("trt_cache"));
        assert_eq!(cfg.paths.presets_dir, PathBuf::from("presets"));
        assert_eq!(cfg.paths.workflows_dir, PathBuf::from("data/workflows"));

        assert_eq!(cfg.server.port, 3000);
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.locale, "en");
        assert!(!cfg.performance.profiling_enabled);
    }

    #[test]
    fn toml_roundtrip_preserves_values() {
        let original = AppConfig::default();
        let encoded = toml::to_string_pretty(&original).expect("serialize config");
        let decoded: AppConfig = toml::from_str(&encoded).expect("deserialize config");
        assert_eq!(decoded, original);
    }

    #[test]
    fn load_from_nonexistent_file_returns_defaults() {
        let path = unique_temp_config_path();
        let loaded = AppConfig::load_from_path(&path).expect("load config from nonexistent path");
        assert_eq!(loaded, AppConfig::default());
    }

    #[test]
    fn data_dir_uses_cli_override() {
        let cli_path = Path::new("/custom");
        let result = data_dir(Some(cli_path));
        assert_eq!(result, PathBuf::from("/custom"));
    }

    #[test]
    fn data_dir_uses_env_var_when_no_cli() {
        env::set_var(ENV_DATA_DIR, "/env/path");
        let result = data_dir(None);
        env::remove_var(ENV_DATA_DIR);
        assert_eq!(result, PathBuf::from("/env/path"));
    }

    #[test]
    fn data_dir_defaults_to_data_dir() {
        let old_videnoa = env::var(ENV_DATA_DIR).ok();
        let old_xdg = env::var("XDG_CONFIG_HOME").ok();
        let old_home = env::var("HOME").ok();

        env::remove_var(ENV_DATA_DIR);
        env::set_var("XDG_CONFIG_HOME", "/xdg");
        env::set_var("HOME", "/home/example");

        let result = data_dir(None);

        if let Some(val) = old_videnoa {
            env::set_var(ENV_DATA_DIR, val);
        } else {
            env::remove_var(ENV_DATA_DIR);
        }
        if let Some(val) = old_xdg {
            env::set_var("XDG_CONFIG_HOME", val);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(val) = old_home {
            env::set_var("HOME", val);
        } else {
            env::remove_var("HOME");
        }

        assert_eq!(result, PathBuf::from("data"));
    }

    #[test]
    fn config_path_is_data_dir_join_config_toml() {
        let result = config_path(Path::new("/data"));
        assert_eq!(result, PathBuf::from("/data/config.toml"));
    }

    #[test]
    fn initialize_creates_data_dir_and_config() {
        let temp = unique_temp_dir();
        initialize_data_dir(&temp).expect("initialize data dir");

        assert!(temp.exists());
        assert!(temp.join("config.toml").exists());

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn initialize_preserves_existing_config() {
        let temp = unique_temp_dir();
        fs::create_dir_all(&temp).expect("create temp dir");

        let cfg_path = temp.join("config.toml");
        let custom_content = "[server]\nport = 9999\n";
        fs::write(&cfg_path, custom_content).expect("write custom config");

        initialize_data_dir(&temp).expect("initialize data dir");

        let content = fs::read_to_string(&cfg_path).expect("read config");
        assert_eq!(content, custom_content);

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn resolve_relative_to_absolute_path_unchanged() {
        let result = resolve_relative_to(Path::new("/base"), Path::new("/abs/path"));
        assert_eq!(result, PathBuf::from("/abs/path"));
    }

    #[test]
    fn resolve_relative_to_joins_relative_path() {
        let result = resolve_relative_to(Path::new("/base"), Path::new("sub"));
        assert_eq!(result, PathBuf::from("/base/sub"));
    }

    #[test]
    fn normalize_supported_locale_maps_known_prefixes() {
        assert_eq!(normalize_supported_locale("zh-CN"), "zh-CN");
        assert_eq!(normalize_supported_locale("zh-TW"), "zh-CN");
        assert_eq!(normalize_supported_locale("en-US"), "en");
        assert_eq!(normalize_supported_locale("fr-FR"), "en");
    }

    fn unique_temp_config_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time moved backwards")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "videnoa-config-test-{}-{timestamp}.toml",
            std::process::id()
        ))
    }

    fn unique_temp_dir() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time moved backwards")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "videnoa-config-test-{}-{timestamp}",
            std::process::id()
        ))
    }
}
