use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::resolve::resolve_references;
use super::ConfigError;

#[derive(Debug)]
struct FileEntry {
    path: PathBuf,
    required: bool,
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
    files: Vec<FileEntry>,
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
    pub fn with_file(mut self, path: impl AsRef<Path>, required: bool) -> Self {
        self.files.push(FileEntry {
            path: path.as_ref().to_path_buf(),
            required,
        });
        self
    }

    /// Builds the configuration by loading, merging, resolving, and deserializing.
    ///
    /// This performs deserialization once at build time rather than on each access,
    /// making subsequent config reads zero-cost.
    pub fn build<T: DeserializeOwned>(self) -> Result<T, ConfigError> {
        let mut merged = toml::Table::new();

        for entry in self.files {
            match std::fs::read_to_string(&entry.path) {
                Ok(contents) => {
                    let table: toml::Table =
                        toml::from_str(&contents).map_err(|e| ConfigError::ParseError {
                            path: entry.path.clone(),
                            source: e,
                        })?;
                    deep_merge(&mut merged, table);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if entry.required {
                        return Err(ConfigError::FileNotFound(entry.path));
                    }
                    // Optional file not found - skip silently
                }
                Err(e) => {
                    return Err(ConfigError::ReadError {
                        path: entry.path,
                        source: e,
                    });
                }
            }
        }

        // Resolve ${...} references after all files are merged
        resolve_references(&mut merged)?;

        // Deserialize into the target type
        let value = toml::Value::Table(merged);
        value.try_into().map_err(ConfigError::DeserializeError)
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
