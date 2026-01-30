use std::path::Path;

use serde::de::DeserializeOwned;

use super::env::EnvSource;
use super::file::FileSource;
use super::resolve::resolve_references;
use super::source::{merge_at_path, ConfigSource};
use super::ConfigError;

#[derive(Default)]
#[must_use = "builders do nothing until .build() is called"]
pub struct Config {
    sources: Vec<Box<dyn ConfigSource>>,
}

impl Config {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn with_file(self, path: impl AsRef<Path>, required: bool) -> Self {
        self.with_source(FileSource::new(path, required))
    }

    pub fn with_env(self, prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        self.with_source(EnvSource::new(prefix, separator))
    }

    pub fn with_source(mut self, source: impl ConfigSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

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

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("sources", &self.sources)
            .finish()
    }
}
