use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::env::load_env_vars;
use super::resolve::resolve_references;
use super::ConfigError;

/// A configuration source in the loading pipeline.
#[derive(Debug)]
enum ConfigSource {
    File { path: PathBuf, required: bool },
    Env { prefix: String, separator: String },
}

/// Builder for loading configuration from multiple TOML files.
///
/// Files are merged in registration order, with later files overriding
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
#[derive(Debug, Default)]
#[must_use = "builders do nothing until .build() is called"]
pub struct Config {
    sources: Vec<ConfigSource>,
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
    pub fn with_file(mut self, path: impl AsRef<Path>, required: bool) -> Self {
        self.sources.push(ConfigSource::File {
            path: path.as_ref().to_path_buf(),
            required,
        });
        self
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
    pub fn with_env(mut self, prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        self.sources.push(ConfigSource::Env {
            prefix: prefix.into(),
            separator: separator.into(),
        });
        self
    }

    /// Builds the configuration by loading, merging, resolving, and deserializing.
    ///
    /// This performs deserialization once at build time rather than on each access,
    /// making subsequent config reads zero-cost.
    pub fn build<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        let mut merged = toml::Table::new();

        for source in self.sources {
            match source {
                ConfigSource::File { path, required } => {
                    if let Some(table) = load_config_file(&path, required)? {
                        deep_merge(&mut merged, table);
                    }
                }
                ConfigSource::Env { prefix, separator } => {
                    load_env_vars(&mut merged, &prefix, &separator);
                }
            }
        }

        // Resolve ${...} references after all sources are merged
        resolve_references(&mut merged)?;

        // Deserialize into the target type
        let value = toml::Value::Table(merged);
        value.try_into().map_err(ConfigError::DeserializeError)
    }
}

/// Loads and parses a TOML config file.
///
/// Returns `Ok(None)` if the file doesn't exist and `required` is false.
fn load_config_file(path: &Path, required: bool) -> Result<Option<toml::Table>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let table = toml::from_str(&contents).map_err(|e| ConfigError::ParseError {
                path: path.to_path_buf(),
                source: e,
            })?;
            Ok(Some(table))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if required {
                Err(ConfigError::FileNotFound(path.to_path_buf()))
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(ConfigError::ReadError {
            path: path.to_path_buf(),
            source: e,
        }),
    }
}

fn deep_merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base_table)), toml::Value::Table(overlay_table)) => {
                deep_merge(base_table, overlay_table);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}
