use std::path::Path;

use serde::de::DeserializeOwned;

use super::env::EnvSource;
use super::file::FileSource;
use super::resolve::resolve_references;
use super::source::{merge_at_path, ConfigSource};
use super::ConfigError;

/// Builder for loading configuration from multiple sources.
///
/// Sources are merged in registration order, with later sources overriding
/// earlier ones. Nested tables are merged recursively; other values
/// (including arrays) are replaced entirely.
///
/// ## Variable References
///
/// String values can reference other config values using `${path.to.field}` syntax:
///
/// ```toml
/// [server]
/// host = "localhost"
/// port = 8080
/// url = "http://${server.host}:${server.port}/api"
/// ```
///
/// Use `$$` to escape a literal `$` (e.g., `$${VAR}` becomes `${VAR}`).
///
/// ## Example
///
/// ```no_run
/// use dragon_fnd::Config;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     name: String,
///     port: u16,
/// }
///
/// let config: MyConfig = Config::builder()
///     .with_file("config/default.toml", true)
///     .with_file("config/local.toml", false)
///     .build()?;
/// # Ok::<(), dragon_fnd::ConfigError>(())
/// ```
#[derive(Default)]
#[must_use = "builders do nothing until .build() is called"]
pub struct Config {
    sources: Vec<Box<dyn ConfigSource>>,
}

impl Config {
    /// Creates a new configuration builder.
    pub fn builder() -> Self {
        Self::default()
    }

    /// Adds a TOML file to be loaded.
    ///
    /// If `required` is `true`, the build will fail if the file doesn't exist.
    /// Optional files that are missing are silently skipped.
    ///
    /// Sources are applied in registration order, so later sources override earlier ones.
    pub fn with_file(self, path: impl AsRef<Path>, required: bool) -> Self {
        self.with_source(FileSource::new(path, required))
    }

    /// Loads configuration from environment variables with the given prefix.
    ///
    /// Environment variables are mapped to config paths by:
    /// 1. Removing the prefix and separator
    /// 2. Splitting remaining segments on the separator
    /// 3. Converting path segments to lowercase
    ///
    /// Values are coerced from strings to the most specific type:
    /// integer, float, boolean, or string (fallback).
    ///
    /// Sources are applied in registration order. This allows flexible layering:
    ///
    /// ```no_run
    /// # use dragon_fnd::Config;
    /// # use serde::Deserialize;
    /// # #[derive(Deserialize)] struct MyConfig { }
    /// // defaults -> env overrides -> local file overrides env
    /// let config: MyConfig = Config::builder()
    ///     .with_file("config/default.toml", true)
    ///     .with_env("MYAPP", "__")
    ///     .with_file("config/local.toml", false)
    ///     .build()?;
    /// # Ok::<(), dragon_fnd::ConfigError>(())
    /// ```
    ///
    /// ## Example
    ///
    /// ```no_run
    /// use dragon_fnd::Config;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct MyConfig {
    ///     database: Database,
    /// }
    ///
    /// #[derive(Deserialize)]
    /// struct Database {
    ///     host: String,
    ///     port: u16,
    /// }
    ///
    /// // With MYAPP__DATABASE__HOST=localhost and MYAPP__DATABASE__PORT=5432
    /// let config: MyConfig = Config::builder()
    ///     .with_file("config/default.toml", true)
    ///     .with_env("MYAPP", "__")
    ///     .build()?;
    /// # Ok::<(), dragon_fnd::ConfigError>(())
    /// ```
    pub fn with_env(self, prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        self.with_source(EnvSource::new(prefix, separator))
    }

    /// Adds a custom configuration source.
    ///
    /// This enables extension with custom source types (CLI args, remote config, etc.)
    /// by implementing the [`ConfigSource`] trait.
    ///
    /// ## Example
    ///
    /// ```ignore
    /// use dragon_fnd::config::{ConfigSource, ConfigEntry, ConfigError};
    ///
    /// struct MyCustomSource { /* ... */ }
    ///
    /// impl ConfigSource for MyCustomSource {
    ///     fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
    ///         // Return configuration entries
    ///         Ok(vec![])
    ///     }
    /// }
    ///
    /// let config: MyConfig = Config::builder()
    ///     .with_file("defaults.toml", true)
    ///     .with_source(MyCustomSource::new())
    ///     .build()?;
    /// ```
    pub fn with_source(mut self, source: impl ConfigSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// Builds the configuration by loading, merging, resolving, and deserializing.
    ///
    /// This performs deserialization once at build time rather than on each access,
    /// making subsequent config reads zero-cost.
    pub fn build<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        let mut merged = toml::Table::new();

        for source in self.sources {
            let entries = source.entries()?;
            for entry in entries {
                merge_at_path(&mut merged, &entry.path, entry.value);
            }
        }

        // Resolve ${...} references after all sources are merged
        resolve_references(&mut merged)?;

        // Deserialize into the target type
        let value = toml::Value::Table(merged);
        value.try_into().map_err(ConfigError::DeserializeError)
    }
}

// Implement Debug manually since Box<dyn ConfigSource> doesn't implement Debug
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("sources", &format!("[{} sources]", self.sources.len()))
            .finish()
    }
}
