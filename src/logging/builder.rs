use std::path::PathBuf;

use super::config::{ConsoleConfig, FileConfig, LogFormat, LoggingConfig, Rotation};

/// Fluent builder for logging configuration.
///
/// This is what [`AppContextBuilder::with_logging()`](crate::context::AppContextBuilder) accepts.
/// Use [`LoggingBuilder::new()`] for programmatic construction, or
/// [`LoggingBuilder::from_config()`] to bridge from a deserialized [`LoggingConfig`].
#[derive(Debug, Clone)]
#[must_use = "builders do nothing until passed to with_logging()"]
pub struct LoggingBuilder {
    config: LoggingConfig,
}

impl LoggingBuilder {
    /// Creates a new builder with sensible defaults (console enabled at info, file disabled).
    pub fn new() -> Self {
        Self {
            config: LoggingConfig::default(),
        }
    }

    /// Creates a builder from a deserialized config, allowing further overrides.
    pub fn from_config(config: &LoggingConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    /// Override the base filter directive (e.g. `"debug"`, `"info,hyper=warn"`).
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.config.filter = filter.into();
        self
    }

    /// Add a per-module filter override.
    pub fn module(mut self, module: impl Into<String>, level: impl Into<String>) -> Self {
        self.config.modules.insert(module.into(), level.into());
        self
    }

    /// Enable or disable logging entirely.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    /// Configure console output.
    pub fn console(mut self, console: ConsoleBuilder) -> Self {
        self.config.console = console.config;
        self
    }

    /// Configure file output.
    pub fn file(mut self, file: FileBuilder) -> Self {
        self.config.file = file.config;
        self
    }

    /// Consume the builder and return the underlying config.
    pub(crate) fn into_config(self) -> LoggingConfig {
        self.config
    }
}

impl Default for LoggingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Fluent builder for console output configuration.
#[derive(Debug, Clone)]
pub struct ConsoleBuilder {
    config: ConsoleConfig,
}

impl ConsoleBuilder {
    /// Creates a new console builder with defaults (enabled, pretty format).
    pub fn new() -> Self {
        Self {
            config: ConsoleConfig::default(),
        }
    }

    /// Set the output format.
    pub fn format(mut self, format: LogFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set a per-layer filter override.
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.config.filter = Some(filter.into());
        self
    }

    /// Enable or disable console output.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }
}

impl Default for ConsoleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Fluent builder for file output configuration.
///
/// Constructing a `FileBuilder` enables file output automatically.
#[derive(Debug, Clone)]
pub struct FileBuilder {
    config: FileConfig,
}

impl FileBuilder {
    /// Creates a new file builder for the given directory. File output is enabled by default.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            config: FileConfig {
                enabled: true,
                dir: dir.into(),
                ..FileConfig::default()
            },
        }
    }

    /// Set the log file prefix.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config.prefix = prefix.into();
        self
    }

    /// Set the output format.
    pub fn format(mut self, format: LogFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set a per-layer filter override.
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.config.filter = Some(filter.into());
        self
    }

    /// Set the rotation strategy.
    pub fn rotation(mut self, rotation: Rotation) -> Self {
        self.config.rotation = rotation;
        self
    }

    /// Set days-based retention (delete files older than N days).
    pub fn retain_days(mut self, days: u32) -> Self {
        self.config.retain_days = Some(days);
        self.config.retain_files = None;
        self
    }

    /// Set file-count retention (keep only N most recent files).
    pub fn retain_files(mut self, count: u32) -> Self {
        self.config.retain_files = Some(count);
        self.config.retain_days = None;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn new_matches_config_defaults() {
        let builder = LoggingBuilder::new();
        let default_config = LoggingConfig::default();
        assert_eq!(builder.config.enabled, default_config.enabled);
        assert_eq!(builder.config.filter, default_config.filter);
        assert_eq!(builder.config.modules, default_config.modules);
        assert_eq!(builder.config.console.enabled, default_config.console.enabled);
        assert_eq!(builder.config.console.format, default_config.console.format);
        assert_eq!(builder.config.file.enabled, default_config.file.enabled);
    }

    #[test]
    fn from_config_round_trips() {
        let config: LoggingConfig = toml::from_str(
            r#"
            filter = "debug"
            modules = { sqlx = "warn" }
            [console]
            format = "json"
            [file]
            enabled = true
            dir = "/var/log"
            prefix = "myapp"
            rotation = "hourly"
            retain_days = 14
            "#,
        )
        .unwrap();

        let builder = LoggingBuilder::from_config(&config);
        let result = builder.into_config();
        assert_eq!(result.filter, "debug");
        assert_eq!(result.modules["sqlx"], "warn");
        assert_eq!(result.console.format, LogFormat::Json);
        assert!(result.file.enabled);
        assert_eq!(result.file.dir, PathBuf::from("/var/log"));
        assert_eq!(result.file.prefix, "myapp");
        assert_eq!(result.file.rotation, Rotation::Hourly);
        assert_eq!(result.file.retain_days, Some(14));
    }

    #[test]
    fn fluent_overrides() {
        let builder = LoggingBuilder::new()
            .filter("debug")
            .module("sqlx", "warn")
            .module("hyper", "info")
            .enabled(false);

        let config = builder.into_config();
        assert_eq!(config.filter, "debug");
        assert!(!config.enabled);
        let mut expected = BTreeMap::new();
        expected.insert("sqlx".to_string(), "warn".to_string());
        expected.insert("hyper".to_string(), "info".to_string());
        assert_eq!(config.modules, expected);
    }

    #[test]
    fn console_builder_overrides() {
        let console = ConsoleBuilder::new()
            .format(LogFormat::Compact)
            .filter("warn")
            .enabled(false);

        assert!(!console.config.enabled);
        assert_eq!(console.config.format, LogFormat::Compact);
        assert_eq!(console.config.filter.as_deref(), Some("warn"));
    }

    #[test]
    fn file_builder_enables_by_default() {
        let file = FileBuilder::new("./logs");
        assert!(file.config.enabled);
        assert_eq!(file.config.dir, PathBuf::from("./logs"));
    }

    #[test]
    fn file_builder_overrides() {
        let file = FileBuilder::new("/var/log/app")
            .prefix("myapp")
            .format(LogFormat::Compact)
            .filter("debug")
            .rotation(Rotation::Hourly)
            .retain_days(30);

        assert_eq!(file.config.prefix, "myapp");
        assert_eq!(file.config.format, LogFormat::Compact);
        assert_eq!(file.config.filter.as_deref(), Some("debug"));
        assert_eq!(file.config.rotation, Rotation::Hourly);
        assert_eq!(file.config.retain_days, Some(30));
        assert!(file.config.retain_files.is_none());
    }

    #[test]
    fn retain_days_clears_retain_files() {
        let file = FileBuilder::new("./logs")
            .retain_files(10)
            .retain_days(7);

        assert_eq!(file.config.retain_days, Some(7));
        assert!(file.config.retain_files.is_none());
    }

    #[test]
    fn retain_files_clears_retain_days() {
        let file = FileBuilder::new("./logs")
            .retain_days(7)
            .retain_files(10);

        assert!(file.config.retain_days.is_none());
        assert_eq!(file.config.retain_files, Some(10));
    }

    #[test]
    fn console_and_file_compose_into_logging_builder() {
        let builder = LoggingBuilder::new()
            .filter("debug")
            .console(ConsoleBuilder::new().format(LogFormat::Pretty))
            .file(
                FileBuilder::new("./logs")
                    .prefix("test")
                    .rotation(Rotation::Daily)
                    .retain_days(7),
            );

        let config = builder.into_config();
        assert_eq!(config.filter, "debug");
        assert_eq!(config.console.format, LogFormat::Pretty);
        assert!(config.file.enabled);
        assert_eq!(config.file.prefix, "test");
        assert_eq!(config.file.rotation, Rotation::Daily);
        assert_eq!(config.file.retain_days, Some(7));
    }
}
