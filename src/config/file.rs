use std::path::{Path, PathBuf};

use super::source::{ConfigEntry, ConfigSource};
use super::ConfigError;

#[derive(Debug, Clone)]
pub struct FileSource {
    path: PathBuf,
    required: bool,
}

impl FileSource {
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

    #[test]
    fn file_source_loads_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[server]\nport = 8080\n").unwrap();

        let source = FileSource::new(&path, true);
        let entries = source.entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.is_empty());
        assert_eq!(entries[0].value["server"]["port"].as_integer(), Some(8080));
    }

    #[test]
    fn file_source_required_missing() {
        let source = FileSource::new("/nonexistent/path/config.toml", true);
        let err = source.entries().unwrap_err();

        assert!(matches!(err, ConfigError::FileNotFound(_)));
    }

    #[test]
    fn file_source_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not [valid toml").unwrap();

        let source = FileSource::new(&path, true);
        let err = source.entries().unwrap_err();

        assert!(matches!(err, ConfigError::ParseError { .. }));
    }

    #[test]
    fn file_source_optional_missing() {
        let source = FileSource::new("/nonexistent/path/config.toml", false);
        let entries = source.entries().unwrap();

        assert!(entries.is_empty());
    }
}
