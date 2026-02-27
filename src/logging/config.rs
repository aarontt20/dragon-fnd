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
    pub(crate) enabled: bool,
    /// Base `EnvFilter` directive (e.g. `"info"`, `"debug,hyper=warn"`).
    pub(crate) filter: String,
    /// Per-module level overrides. Keys are module paths, values are level strings.
    pub(crate) modules: BTreeMap<String, String>,
    /// Console output configuration.
    pub(crate) console: ConsoleConfig,
    /// File output configuration.
    pub(crate) file: FileConfig,
}

impl LoggingConfig {
    /// Whether logging is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }
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
    pub(crate) enabled: bool,
    pub(crate) format: LogFormat,
    /// Optional per-layer filter override. When set, this layer uses its own
    /// `EnvFilter` instead of the base filter.
    pub(crate) filter: Option<String>,
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
#[derive(Debug, Clone, PartialEq)]
pub struct FileConfig {
    pub(crate) enabled: bool,
    pub(crate) dir: PathBuf,
    pub(crate) prefix: String,
    pub(crate) format: LogFormat,
    /// Optional per-layer filter override.
    pub(crate) filter: Option<String>,
    pub(crate) rotation: RotationStrategy,
    /// Whether to gzip-compress rotated log files. Requires rotation to be enabled
    /// (a rotation strategy other than `Never`).
    pub(crate) compress: bool,
    /// Delete log files older than this many days. Mutually exclusive with `retain_files`.
    pub(crate) retain_days: Option<u32>,
    /// Keep only this many most recent log files. Mutually exclusive with `retain_days`.
    pub(crate) retain_files: Option<u32>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: PathBuf::from("./logs"),
            prefix: "app".to_string(),
            format: LogFormat::Json,
            filter: None,
            rotation: RotationStrategy::Daily,
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
#[derive(Debug, Clone, PartialEq)]
pub enum RotationStrategy {
    Daily,
    Hourly,
    SizeBased { max_bytes: u64 },
    Never,
}

impl<'de> Deserialize<'de> for FileConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(default)]
        struct Raw {
            enabled: bool,
            dir: PathBuf,
            prefix: String,
            format: LogFormat,
            filter: Option<String>,
            rotation: String,
            max_bytes: Option<u64>,
            compress: bool,
            retain_days: Option<u32>,
            retain_files: Option<u32>,
        }

        impl Default for Raw {
            fn default() -> Self {
                let defaults = FileConfig::default();
                Self {
                    enabled: defaults.enabled,
                    dir: defaults.dir,
                    prefix: defaults.prefix,
                    format: defaults.format,
                    filter: defaults.filter,
                    rotation: "daily".to_string(),
                    max_bytes: None,
                    compress: defaults.compress,
                    retain_days: defaults.retain_days,
                    retain_files: defaults.retain_files,
                }
            }
        }

        let raw = Raw::deserialize(deserializer)?;

        let rotation = match raw.rotation.as_str() {
            "daily" => RotationStrategy::Daily,
            "hourly" => RotationStrategy::Hourly,
            "never" => RotationStrategy::Never,
            "size" => {
                let max_bytes = raw.max_bytes.ok_or_else(|| {
                    serde::de::Error::custom(
                        "rotation = \"size\" requires max_bytes to be set",
                    )
                })?;
                RotationStrategy::SizeBased { max_bytes }
            }
            other => {
                return Err(serde::de::Error::custom(format!(
                    "unknown rotation strategy: '{other}' (expected daily, hourly, never, or size)"
                )));
            }
        };

        Ok(FileConfig {
            enabled: raw.enabled,
            dir: raw.dir,
            prefix: raw.prefix,
            format: raw.format,
            filter: raw.filter,
            rotation,
            compress: raw.compress,
            retain_days: raw.retain_days,
            retain_files: raw.retain_files,
        })
    }
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
        assert_eq!(config.file.rotation, RotationStrategy::Daily);
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
    fn size_rotation_parses() {
        let config: LoggingConfig = toml::from_str(
            r#"
            [file]
            rotation = "size"
            max_bytes = 10485760
            "#,
        )
        .unwrap();
        assert_eq!(
            config.file.rotation,
            RotationStrategy::SizeBased {
                max_bytes: 10_485_760
            }
        );
    }

    #[test]
    fn size_rotation_without_max_bytes_errors() {
        let result: Result<LoggingConfig, _> = toml::from_str(
            r#"
            [file]
            rotation = "size"
            "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn unknown_rotation_strategy_errors() {
        let result: Result<LoggingConfig, _> = toml::from_str(
            r#"
            [file]
            rotation = "weekly"
            "#,
        );
        assert!(result.is_err());
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
        assert_eq!(config.file.rotation, RotationStrategy::Hourly);
        assert!(config.file.compress);
        assert_eq!(config.file.retain_days, Some(30));
    }

    #[test]
    fn rotation_variants_parse() {
        let daily: LoggingConfig = toml::from_str("[file]\nrotation = \"daily\"").unwrap();
        assert_eq!(daily.file.rotation, RotationStrategy::Daily);

        let hourly: LoggingConfig = toml::from_str("[file]\nrotation = \"hourly\"").unwrap();
        assert_eq!(hourly.file.rotation, RotationStrategy::Hourly);

        let never: LoggingConfig = toml::from_str("[file]\nrotation = \"never\"").unwrap();
        assert_eq!(never.file.rotation, RotationStrategy::Never);

        let size: LoggingConfig =
            toml::from_str("[file]\nrotation = \"size\"\nmax_bytes = 1048576").unwrap();
        assert_eq!(
            size.file.rotation,
            RotationStrategy::SizeBased {
                max_bytes: 1_048_576
            }
        );
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
