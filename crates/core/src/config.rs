use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const CONFIG_FILE_NAME: &str = "config.toml";
const ENV_DATA_DIR: &str = "VIDENOA_DATA_DIR";
pub const FALLBACK_LOCALE: &str = "en";
/// Version written to `config.toml` as the top-level `version` key. A file
/// without it is version 1. Saving never lowers the version found on disk,
/// and keys this build does not know are kept (see [`AppConfig::save_to_path`]),
/// so running an older build does not erase a newer build's settings.
///
/// 2. A relative `paths.workflows_dir` is relative to the data directory
///    instead of the working directory (see [`AppConfig::load_from_path`]).
pub const CONFIG_VERSION: i64 = 2;
const CONFIG_VERSION_KEY: &str = "version";
/// The version 1 default of `paths.workflows_dir`, relative to the working
/// directory. It meant `<cwd>/data/workflows`, the workflows folder of the
/// default data directory.
const LEGACY_WORKFLOWS_DIR: &str = "data/workflows";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub paths: PathsConfig,
    pub server: ServerConfig,
    pub locale: String,
    pub performance: PerformanceConfig,
    pub auth: AuthConfig,
    pub iroh: IrohConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct IrohConfig {
    pub enabled: bool,
    /// Self-hosted relays; empty uses the public N0 relays and discovery.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relay_urls: Vec<String>,
    /// Also use N0's public relays next to `relay_urls`; ignored without them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub use_public_relays: bool,
}

impl IrohConfig {
    /// The relay network these settings select.
    ///
    /// # Errors
    /// Returns an error when a relay URL is invalid.
    pub fn network(&self) -> anyhow::Result<videnoa_transport::Network> {
        Ok(videnoa_transport::Network::with_relays(&self.relay_urls)?
            .including_public_relays(self.use_public_relays))
    }
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
    /// Extra `Host` names accepted while no password is set (DNS-rebinding defence).
    /// IP addresses, `localhost` and this machine's own name are always accepted.
    pub allowed_hosts: Vec<String>,
}
impl ServerConfig {
    pub fn validate(&self) -> Result<()> {
        for name in &self.allowed_hosts {
            anyhow::ensure!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')),
                "server.allowed_hosts entries must be host names without scheme or port, such as \"nas.local\""
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[derive(Default)]
pub struct PerformanceConfig {
    pub profiling_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthConfig {
    pub session_absolute_seconds: u64,
    pub session_idle_seconds: u64,
    pub secure_cookie: bool,
}
impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            session_absolute_seconds: 2_592_000,
            session_idle_seconds: 604_800,
            secure_cookie: false,
        }
    }
}
impl AuthConfig {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.session_idle_seconds > 0
                && self.session_idle_seconds <= self.session_absolute_seconds,
            "Session durations must be positive and idle must not exceed absolute lifetime"
        );
        let seconds = i64::try_from(self.session_absolute_seconds)?;
        let duration =
            chrono::Duration::try_seconds(seconds).context("Session duration out of range")?;
        anyhow::ensure!(
            self.session_absolute_seconds <= 9_007_199_254_740_991
                && chrono::Utc::now().checked_add_signed(duration).is_some(),
            "Session expiry out of range"
        );
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            paths: PathsConfig::default(),
            server: ServerConfig::default(),
            locale: FALLBACK_LOCALE.to_string(),
            performance: PerformanceConfig::default(),
            auth: AuthConfig::default(),
            iroh: IrohConfig::default(),
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
            workflows_dir: PathBuf::from("workflows"),
        }
    }
}

/// Where relative paths point.
///
/// - Mutable state (`workflows_dir`) lives under the data directory.
/// - Shipped assets (`models_dir`, `presets_dir`, `trt_cache_dir`) are
///   relative to the asset base: the first of the working directory, the
///   executable's directory and its parent that contains `presets_dir`;
///   the working directory when none does. Docker (WORKDIR /app) and a
///   run from the repository resolve against the working directory, a
///   release bundle started from elsewhere against its own folder.
impl PathsConfig {
    pub fn resolved_workflows_dir(&self, data_dir: &Path) -> PathBuf {
        resolve_relative_to(data_dir, &self.workflows_dir)
    }

    pub fn resolved_models_dir(&self) -> PathBuf {
        self.resolve_asset(&self.models_dir)
    }

    pub fn resolved_presets_dir(&self) -> PathBuf {
        self.resolve_asset(&self.presets_dir)
    }

    pub fn resolved_trt_cache_dir(&self) -> PathBuf {
        self.resolve_asset(&self.trt_cache_dir)
    }

    fn resolve_asset(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        asset_base_dir(&self.presets_dir).join(path)
    }

    /// Where a `model_path` written in a workflow points. A relative path is
    /// looked up in `models_dir`, also without a leading `models/` (bundled
    /// presets write `models/<file>`), then against the asset base. If no
    /// such file exists it is returned unchanged, so it resolves against the
    /// working directory as it always did and the node reports it.
    pub fn resolve_model_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        let models_dir = self.resolved_models_dir();
        let mut candidates = vec![models_dir.join(path)];
        if let Ok(rest) = path.strip_prefix("models") {
            candidates.push(models_dir.join(rest));
        }
        candidates.push(asset_base_dir(&self.presets_dir).join(path));
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| path.to_path_buf())
    }

    /// Applies [`Self::resolve_model_path`] to every `model_path` param of a
    /// workflow document (`{"nodes": [{"params": {...}}, ...]}`).
    pub fn resolve_model_paths_in(&self, workflow: &mut serde_json::Value) {
        let Some(nodes) = workflow
            .get_mut("nodes")
            .and_then(serde_json::Value::as_array_mut)
        else {
            return;
        };
        for node in nodes {
            let Some(model_path) = node.pointer_mut("/params/model_path") else {
                continue;
            };
            if let Some(raw) = model_path.as_str() {
                let resolved = self.resolve_model_path(Path::new(raw));
                *model_path = serde_json::Value::String(resolved.to_string_lossy().into_owned());
            }
        }
    }
}

/// See [`PathsConfig`]: the base directory of relative shipped-asset paths.
pub fn asset_base_dir(presets_dir: &Path) -> PathBuf {
    let cwd = env::current_dir().ok();
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let candidates: Vec<PathBuf> = cwd
        .iter()
        .cloned()
        .chain(exe_dir.iter().cloned())
        .chain(
            exe_dir
                .iter()
                .filter_map(|dir| dir.parent().map(Path::to_path_buf)),
        )
        .collect();
    first_dir_containing(&candidates, presets_dir)
        .or(cwd)
        .unwrap_or_default()
}

fn first_dir_containing(candidates: &[PathBuf], relative: &Path) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.join(relative).is_dir())
        .cloned()
}

/// Copies the workflows saved under the version 1 default location,
/// `<cwd>/data/workflows`, into `workflows_dir` when that resolves somewhere
/// else under a custom data directory and does not exist yet. The old files
/// stay where they are. Returns the number of files copied.
pub fn migrate_legacy_workflows_dir(
    paths: &PathsConfig,
    data_dir: &Path,
    cwd: &Path,
) -> Result<usize> {
    if paths.workflows_dir.is_absolute() {
        return Ok(0);
    }
    let legacy = cwd.join(LEGACY_WORKFLOWS_DIR);
    let target = paths.resolved_workflows_dir(data_dir);
    if !legacy.is_dir() || target.exists() {
        return Ok(0);
    }
    fs::create_dir_all(&target)
        .with_context(|| format!("failed to create workflows directory: {}", target.display()))?;
    let mut copied = 0;
    for entry in fs::read_dir(&legacy)
        .with_context(|| format!("failed to read legacy workflows: {}", legacy.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), target.join(entry.file_name())).with_context(|| {
                format!("failed to copy legacy workflow {}", entry.path().display())
            })?;
            copied += 1;
        }
    }
    Ok(copied)
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "0.0.0.0".to_string(),
            allowed_hosts: Vec::new(),
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

        let table: toml::Table = raw
            .parse()
            .with_context(|| format!("failed to parse config TOML: {}", path.display()))?;
        let version = config_version(&table);
        if version > CONFIG_VERSION {
            tracing::warn!(
                version,
                supported = CONFIG_VERSION,
                path = %path.display(),
                "config.toml was written by a newer build; settings this build does not know are kept unchanged"
            );
        }
        let mut config: Self = toml::Value::Table(table)
            .try_into()
            .with_context(|| format!("failed to parse config TOML: {}", path.display()))?;
        if version < 2 {
            config.paths.workflows_dir = migrate_v1_workflows_dir(&config.paths.workflows_dir);
        }
        Ok(config)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .context("config path does not have a parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;

        let encoded = self.encode_over(path)?;
        use std::io::Write;
        let temporary = parent.join(format!(".config-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(encoded.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.with_context(|| format!("failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Serializes this config for writing over the file at `path`. Keys in
    /// that file that this build does not know are carried over, and its
    /// version is kept when it is newer than [`CONFIG_VERSION`].
    fn encode_over(&self, path: &Path) -> Result<String> {
        let mut table = encode_table(self)?;
        let existing = fs::read_to_string(path)
            .ok()
            .and_then(|raw| raw.parse::<toml::Table>().ok());
        let mut version = CONFIG_VERSION;
        if let Some(existing) = existing {
            version = version.max(config_version(&existing));
            // Keys this build knows are those it writes back after reading the
            // file; everything else in the file belongs to another build.
            match toml::Value::Table(existing.clone())
                .try_into::<AppConfig>()
                .map_err(anyhow::Error::from)
                .and_then(|known| encode_table(&known))
            {
                Ok(known) => keep_unknown_keys(&mut table, &existing, &known),
                Err(error) => tracing::warn!(
                    %error,
                    path = %path.display(),
                    "Existing config is unreadable; unknown keys in it are not kept"
                ),
            }
        }
        table.insert(CONFIG_VERSION_KEY.to_owned(), toml::Value::Integer(version));
        toml::to_string_pretty(&table).context("failed to serialize config TOML")
    }
}

/// Version 1 resolved a relative `workflows_dir` against the working
/// directory. Its default becomes the data-relative default; any other
/// relative value is pinned to the working directory it meant.
fn migrate_v1_workflows_dir(workflows_dir: &Path) -> PathBuf {
    if workflows_dir == Path::new(LEGACY_WORKFLOWS_DIR) {
        return PathsConfig::default().workflows_dir;
    }
    match env::current_dir() {
        Ok(cwd) if workflows_dir.is_relative() => cwd.join(workflows_dir),
        _ => workflows_dir.to_path_buf(),
    }
}

fn encode_table(config: &AppConfig) -> Result<toml::Table> {
    match toml::Value::try_from(config).context("failed to serialize config TOML")? {
        toml::Value::Table(table) => Ok(table),
        other => anyhow::bail!(
            "config serialized to a {} instead of a table",
            other.type_str()
        ),
    }
}

/// The `version` key of a config file; 1 when it has none.
fn config_version(table: &toml::Table) -> i64 {
    table
        .get(CONFIG_VERSION_KEY)
        .and_then(toml::Value::as_integer)
        .unwrap_or(1)
}

/// Copies into `target` every key of `existing` that is absent from `known`
/// (what this build writes for that file), recursing into tables both know.
/// A known key missing from `target` was cleared on purpose and stays absent.
fn keep_unknown_keys(target: &mut toml::Table, existing: &toml::Table, known: &toml::Table) {
    for (key, value) in existing {
        match (known.get(key), value) {
            (None, _) => {
                target.entry(key.clone()).or_insert_with(|| value.clone());
            }
            (Some(toml::Value::Table(known_section)), toml::Value::Table(existing_section)) => {
                if let Some(toml::Value::Table(target_section)) = target.get_mut(key) {
                    keep_unknown_keys(target_section, existing_section, known_section);
                }
            }
            (Some(_), _) => {}
        }
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
        assert_eq!(cfg.paths.workflows_dir, PathBuf::from("workflows"));

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
    fn iroh_relay_urls_are_optional_in_the_config_file() {
        // Existing files without relays keep the public N0 infrastructure.
        let legacy: AppConfig = toml::from_str("[iroh]\nenabled = true\n").expect("legacy iroh");
        assert!(legacy.iroh.enabled);
        assert!(legacy.iroh.relay_urls.is_empty());
        let encoded = toml::to_string_pretty(&legacy).expect("serialize config");
        assert!(!encoded.contains("relay_urls"), "{encoded}");
        assert!(!legacy.iroh.use_public_relays);
        assert!(!encoded.contains("use_public_relays"), "{encoded}");

        let relays: AppConfig = toml::from_str(
            "[iroh]\nenabled = true\nrelay_urls = [\"https://relay.example.test\"]\nuse_public_relays = true\n",
        )
        .expect("self-hosted relays");
        assert_eq!(relays.iroh.relay_urls, ["https://relay.example.test"]);
        assert!(relays.iroh.use_public_relays);
        let encoded = toml::to_string_pretty(&relays).expect("serialize config");
        assert_eq!(
            toml::from_str::<AppConfig>(&encoded).expect("roundtrip"),
            relays
        );
    }

    #[test]
    fn server_allowed_hosts_are_optional_and_must_be_bare_names() {
        let legacy: AppConfig =
            toml::from_str("[server]\nport = 3000\nhost = \"0.0.0.0\"\n").expect("legacy server");
        assert!(legacy.server.allowed_hosts.is_empty());
        legacy.server.validate().expect("empty list is valid");

        let mut server = ServerConfig {
            allowed_hosts: vec!["nas.local".into(), "Media_Box-2".into()],
            ..ServerConfig::default()
        };
        server.validate().expect("bare names are valid");
        for invalid in [
            "",
            "nas.local:3000",
            "http://nas.local",
            "nas local",
            "nas/",
        ] {
            server.allowed_hosts = vec![invalid.into()];
            assert!(server.validate().is_err(), "{invalid}");
        }
    }

    #[test]
    fn saving_keeps_keys_this_build_does_not_know() {
        // Given: a config written by a newer build, with a key in a known
        // section and a whole section this build does not know.
        let temp = unique_temp_dir();
        fs::create_dir_all(&temp).expect("create temp dir");
        let path = temp.join("config.toml");
        fs::write(
            &path,
            "version = 99\nlocale = \"zh-CN\"\n\n[server]\nport = 4000\nallowed_hosts = [\"nas.lan\"]\n\n[future]\nmode = \"fast\"\n",
        )
        .expect("write newer config");

        // When: this build loads it, changes a known value and saves it.
        let mut config = AppConfig::load_from_path(&path).expect("load newer config");
        assert_eq!(config.server.port, 4000);
        config.locale = "en".to_string();
        config.save_to_path(&path).expect("save config");

        // Then: the change is written and the unknown keys are kept.
        let saved: toml::Table = fs::read_to_string(&path)
            .expect("read saved config")
            .parse()
            .expect("saved config is TOML");
        assert_eq!(saved["locale"].as_str(), Some("en"));
        assert_eq!(saved["server"]["port"].as_integer(), Some(4000));
        assert_eq!(
            saved["server"]["allowed_hosts"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(saved["future"]["mode"].as_str(), Some("fast"));
        // The newer version stays, so that build does not migrate it again.
        assert_eq!(saved["version"].as_integer(), Some(99));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn saved_config_records_its_version_and_cleared_known_keys_stay_cleared() {
        let temp = unique_temp_dir();
        fs::create_dir_all(&temp).expect("create temp dir");
        let path = temp.join("config.toml");
        fs::write(
            &path,
            "[iroh]\nenabled = true\nrelay_urls = [\"https://relay.example.test\"]\n",
        )
        .expect("write config");

        let mut config = AppConfig::load_from_path(&path).expect("load config");
        config.iroh.relay_urls.clear();
        config.save_to_path(&path).expect("save config");

        let saved: toml::Table = fs::read_to_string(&path)
            .expect("read saved config")
            .parse()
            .expect("saved config is TOML");
        assert_eq!(saved["version"].as_integer(), Some(CONFIG_VERSION));
        assert!(saved["iroh"].get("relay_urls").is_none(), "{saved}");

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn relative_state_paths_resolve_against_the_data_dir() {
        let paths = PathsConfig::default();
        assert_eq!(
            paths.resolved_workflows_dir(Path::new("/srv/videnoa")),
            PathBuf::from("/srv/videnoa/workflows")
        );
        let custom = PathsConfig {
            workflows_dir: PathBuf::from("/elsewhere/workflows"),
            ..PathsConfig::default()
        };
        assert_eq!(
            custom.resolved_workflows_dir(Path::new("/srv/videnoa")),
            PathBuf::from("/elsewhere/workflows")
        );
    }

    #[test]
    fn a_version_1_config_keeps_pointing_at_its_workflows() {
        let temp = unique_temp_dir();
        fs::create_dir_all(&temp).expect("create temp dir");
        let path = temp.join("config.toml");

        // The old default becomes the data-relative default.
        fs::write(&path, "[paths]\nworkflows_dir = \"data/workflows\"\n").expect("write");
        let config = AppConfig::load_from_path(&path).expect("load v1 default");
        assert_eq!(config.paths.workflows_dir, PathBuf::from("workflows"));

        // Another relative value keeps meaning the working directory.
        fs::write(&path, "[paths]\nworkflows_dir = \"my-workflows\"\n").expect("write");
        let config = AppConfig::load_from_path(&path).expect("load v1 custom");
        assert_eq!(
            config.paths.workflows_dir,
            env::current_dir().unwrap().join("my-workflows")
        );

        // A version 2 file is taken as written.
        fs::write(
            &path,
            "version = 2\n[paths]\nworkflows_dir = \"my-workflows\"\n",
        )
        .expect("write");
        let config = AppConfig::load_from_path(&path).expect("load v2");
        assert_eq!(config.paths.workflows_dir, PathBuf::from("my-workflows"));

        fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn legacy_workflows_are_copied_into_a_custom_data_dir_once() {
        // Given: workflows saved by an older build under <cwd>/data/workflows
        // while the data directory was elsewhere.
        let root = unique_temp_dir();
        let cwd = root.join("cwd");
        let data_dir = root.join("state");
        fs::create_dir_all(cwd.join("data/workflows")).expect("create legacy dir");
        fs::write(cwd.join("data/workflows/mine.json"), "{}").expect("write workflow");
        let paths = PathsConfig::default();

        // When: the new build starts.
        let copied = migrate_legacy_workflows_dir(&paths, &data_dir, &cwd).expect("migrate");

        // Then: the workflow is in the data directory and the original stays.
        assert_eq!(copied, 1);
        assert!(data_dir.join("workflows/mine.json").is_file());
        assert!(cwd.join("data/workflows/mine.json").is_file());
        // Once the new directory exists nothing is copied again.
        fs::write(cwd.join("data/workflows/later.json"), "{}").expect("write workflow");
        assert_eq!(
            migrate_legacy_workflows_dir(&paths, &data_dir, &cwd).expect("rerun"),
            0
        );
        // With the default data directory both locations are the same.
        assert_eq!(
            migrate_legacy_workflows_dir(&paths, &cwd.join("data"), &cwd).expect("same dir"),
            0
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn asset_paths_resolve_against_the_first_dir_with_presets() {
        let root = unique_temp_dir();
        let cwd = root.join("home");
        let bundle = root.join("bundle");
        fs::create_dir_all(&cwd).expect("create cwd");
        fs::create_dir_all(bundle.join("presets")).expect("create bundle presets");
        let candidates = [cwd.clone(), bundle.clone()];

        assert_eq!(
            first_dir_containing(&candidates, Path::new("presets")),
            Some(bundle.clone())
        );
        fs::create_dir_all(cwd.join("presets")).expect("create cwd presets");
        assert_eq!(
            first_dir_containing(&candidates, Path::new("presets")),
            Some(cwd.clone())
        );
        assert_eq!(first_dir_containing(&candidates, Path::new("none")), None);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn workflow_model_paths_resolve_against_models_dir() {
        let root = unique_temp_dir();
        let models = root.join("my-models");
        fs::create_dir_all(&models).expect("create models dir");
        fs::write(models.join("x2.onnx"), b"model").expect("write model");
        let paths = PathsConfig {
            models_dir: models.clone(),
            ..PathsConfig::default()
        };

        // Bundled presets write `models/<file>`; bare names work too.
        assert_eq!(
            paths.resolve_model_path(Path::new("models/x2.onnx")),
            models.join("x2.onnx")
        );
        assert_eq!(
            paths.resolve_model_path(Path::new("x2.onnx")),
            models.join("x2.onnx")
        );
        // Unknown files and absolute paths are left alone.
        assert_eq!(
            paths.resolve_model_path(Path::new("models/missing.onnx")),
            PathBuf::from("models/missing.onnx")
        );
        assert_eq!(
            paths.resolve_model_path(Path::new("/abs/x2.onnx")),
            PathBuf::from("/abs/x2.onnx")
        );

        fs::remove_dir_all(&root).ok();
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

    /// Tests run in parallel and share the process environment.
    static DATA_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn data_dir_uses_env_var_when_no_cli() {
        let _env = DATA_DIR_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        env::set_var(ENV_DATA_DIR, "/env/path");
        let result = data_dir(None);
        env::remove_var(ENV_DATA_DIR);
        assert_eq!(result, PathBuf::from("/env/path"));
    }

    #[test]
    fn data_dir_defaults_to_data_dir() {
        let _env = DATA_DIR_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let old_videnoa = env::var_os(ENV_DATA_DIR);
        env::remove_var(ENV_DATA_DIR);

        let result = data_dir(None);

        if let Some(val) = old_videnoa {
            env::set_var(ENV_DATA_DIR, val);
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
