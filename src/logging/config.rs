use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

/// Top-level logging configuration, deserialized from the `[logging]` section.
///
/// All fields have serde defaults. An empty `[logging]` table produces a
/// working configuration (console enabled at info level, file disabled).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Master switch. When false, no subscriber is initialized.
    pub enabled: bool,
    /// Base `EnvFilter` directive (e.g. `"info"`, `"debug,hyper=warn"`).
    pub filter: String,
    /// Per-module level overrides. Keys are module paths, values are level strings.
    pub modules: BTreeMap<String, String>,
    /// Console output configuration.
    pub console: ConsoleConfig,
    /// File output configuration.
    pub file: FileConfig,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            filter: "info".to_string(),
            modules: BTreeMap::new(),
            console: ConsoleConfig::default(),
            file: FileConfig::default(),
        }
    }
}

/// Console output configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ConsoleConfig {
    pub enabled: bool,
    pub format: LogFormat,
    /// Optional per-layer filter override. When set, this layer uses its own
    /// `EnvFilter` instead of the base filter.
    pub filter: Option<String>,
}

impl Default for ConsoleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: LogFormat::Pretty,
            filter: None,
        }
    }
}

/// File output configuration.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub enabled: bool,
    pub dir: PathBuf,
    pub prefix: String,
    pub format: LogFormat,
    /// Optional per-layer filter override.
    pub filter: Option<String>,
    pub rotation: Rotation,
    /// Maximum size in bytes before rotating the active log file.
    /// When set, `rotation` must be `Never` (time + size composition is not yet supported).
    pub max_bytes: Option<u64>,
    /// Whether to gzip-compress rotated log files. Requires rotation to be enabled
    /// (either via `max_bytes` or a time-based `rotation` other than `Never`).
    pub compress: bool,
    /// Delete log files older than this many days. Mutually exclusive with `retain_files`.
    pub retain_days: Option<u32>,
    /// Keep only this many most recent log files. Mutually exclusive with `retain_days`.
    pub retain_files: Option<u32>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: PathBuf::from("./logs"),
            prefix: "app".to_string(),
            format: LogFormat::Json,
            filter: None,
            rotation: Rotation::Daily,
            max_bytes: None,
            compress: false,
            retain_days: None,
            retain_files: None,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Pretty,
    Json,
    Compact,
}

/// Log file rotation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rotation {
    Daily,
    Hourly,
    Never,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_config_defaults() {
        let config: LoggingConfig = toml::from_str("").unwrap();
        assert!(config.enabled);
        assert_eq!(config.filter, "info");
        assert!(config.modules.is_empty());
        assert!(config.console.enabled);
        assert_eq!(config.console.format, LogFormat::Pretty);
        assert!(!config.file.enabled);
        assert_eq!(config.file.dir, PathBuf::from("./logs"));
        assert_eq!(config.file.prefix, "app");
        assert_eq!(config.file.format, LogFormat::Json);
        assert_eq!(config.file.rotation, Rotation::Daily);
        assert!(config.file.max_bytes.is_none());
        assert!(!config.file.compress);
        assert!(config.file.retain_days.is_none());
        assert!(config.file.retain_files.is_none());
    }

    #[test]
    fn console_filter_override_parses() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [console]
            filter = "warn"
            "#,
        )
        .unwrap();
        assert_eq!(config.console.filter.as_deref(), Some("warn"));
    }

    #[test]
    fn file_filter_override_parses() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            filter = "debug"
            "#,
        )
        .unwrap();
        assert_eq!(config.file.filter.as_deref(), Some("debug"));
    }

    #[test]
    fn retain_days_only() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            retain_days = 7
            "#,
        )
        .unwrap();
        assert_eq!(config.file.retain_days, Some(7));
        assert!(config.file.retain_files.is_none());
    }

    #[test]
    fn retain_files_only() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            retain_files = 10
            "#,
        )
        .unwrap();
        assert!(config.file.retain_days.is_none());
        assert_eq!(config.file.retain_files, Some(10));
    }

    #[test]
    fn max_bytes_parses() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            max_bytes = 10485760
            "#,
        )
        .unwrap();
        assert_eq!(config.file.max_bytes, Some(10_485_760));
    }

    #[test]
    fn compress_parses() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            compress = true
            "#,
        )
        .unwrap();
        assert!(config.file.compress);
    }

    #[test]
    fn full_toml_round_trip() {
        let config: LoggingConfig = toml::from_str(
            r#"
            enabled = true
            filter = "debug,hyper=warn"
            modules = { sqlx = "warn", tower = "info" }

            [console]
            enabled = true
            format = "compact"
            filter = "warn"

            [file]
            enabled = true
            dir = "/var/log/myapp"
            prefix = "myapp"
            format = "json"
            filter = "debug"
            rotation = "hourly"
            max_bytes = 52428800
            compress = true
            retain_days = 30
            "#,
        )
        .unwrap();

        assert!(config.enabled);
        assert_eq!(config.filter, "debug,hyper=warn");
        assert_eq!(config.modules.len(), 2);
        assert_eq!(config.modules["sqlx"], "warn");
        assert_eq!(config.modules["tower"], "info");
        assert!(config.console.enabled);
        assert_eq!(config.console.format, LogFormat::Compact);
        assert_eq!(config.console.filter.as_deref(), Some("warn"));
        assert!(config.file.enabled);
        assert_eq!(config.file.dir, PathBuf::from("/var/log/myapp"));
        assert_eq!(config.file.prefix, "myapp");
        assert_eq!(config.file.format, LogFormat::Json);
        assert_eq!(config.file.filter.as_deref(), Some("debug"));
        assert_eq!(config.file.rotation, Rotation::Hourly);
        assert_eq!(config.file.max_bytes, Some(52_428_800));
        assert!(config.file.compress);
        assert_eq!(config.file.retain_days, Some(30));
    }

    #[test]
    fn rotation_variants_parse() {
        let daily: LoggingConfig = toml::from_str("[file]\nrotation = \"daily\"").unwrap();
        assert_eq!(daily.file.rotation, Rotation::Daily);

        let hourly: LoggingConfig = toml::from_str("[file]\nrotation = \"hourly\"").unwrap();
        assert_eq!(hourly.file.rotation, Rotation::Hourly);

        let never: LoggingConfig = toml::from_str("[file]\nrotation = \"never\"").unwrap();
        assert_eq!(never.file.rotation, Rotation::Never);
    }

    #[test]
    fn format_variants_parse() {
        let pretty: LoggingConfig = toml::from_str("[console]\nformat = \"pretty\"").unwrap();
        assert_eq!(pretty.console.format, LogFormat::Pretty);

        let json: LoggingConfig = toml::from_str("[console]\nformat = \"json\"").unwrap();
        assert_eq!(json.console.format, LogFormat::Json);

        let compact: LoggingConfig = toml::from_str("[console]\nformat = \"compact\"").unwrap();
        assert_eq!(compact.console.format, LogFormat::Compact);
    }
}
