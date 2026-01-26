//! File-based configuration source.

use std::path::{Path, PathBuf};

use super::source::{ConfigEntry, ConfigSource};
use super::ConfigError;

/// A configuration source that loads from a TOML file.
///
/// Files can be marked as required or optional. Required files that don't exist
/// cause an error; optional files that don't exist are silently skipped.
#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
    required: bool,
}

impl FileSource {
    /// Creates a new file source.
    ///
    /// If `required` is true, the build will fail if the file doesn't exist.
    pub fn new(path: impl AsRef<Path>, required: bool) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            required,
        }
    }
}

impl ConfigSource for FileSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        match load_config_file(&self.path, self.required)? {
            Some(table) => Ok(vec![ConfigEntry::root(table)]),
            None => Ok(vec![]),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_source_loads_valid_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "key = \"value\"").unwrap();

        let source = FileSource::new(file.path(), true);
        let entries = source.entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.is_empty());
        let table = entries[0].value.as_table().unwrap();
        assert_eq!(
            table.get("key"),
            Some(&toml::Value::String("value".into()))
        );
    }

    #[test]
    fn test_file_source_required_missing() {
        let source = FileSource::new("/nonexistent/path/config.toml", true);
        let result = source.entries();

        assert!(matches!(result, Err(ConfigError::FileNotFound(_))));
    }

    #[test]
    fn test_file_source_optional_missing() {
        let source = FileSource::new("/nonexistent/path/config.toml", false);
        let entries = source.entries().unwrap();

        assert!(entries.is_empty());
    }
}
