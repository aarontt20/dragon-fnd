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
