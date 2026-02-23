use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("required config file not found: {0}")]
    FileNotFound(PathBuf),

    #[error("failed to read config file '{path}': {source}")]
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    /// TOML syntax error in a specific file. Constructed manually with file path
    /// context, unlike `DeserializeError` which wraps the final build-time
    /// deserialization (no file path available at that stage).
    #[error("failed to parse config file '{path}': {source}")]
    ParseError {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("failed to deserialize config: {0}")]
    DeserializeError(toml::de::Error),

    #[error("root-level config entry must be a table, got {0}")]
    RootNotTable(String),

    #[error("circular reference detected: {}", .0.join(" -> "))]
    CircularReference(Vec<String>),

    #[error("referenced path not found: {0}")]
    ReferenceNotFound(String),

    #[error("invalid reference path: {0}")]
    InvalidReferencePath(String),

    #[error("cannot reference non-scalar value: {0}")]
    NonScalarReference(String),

    #[error("unclosed reference (missing '}}')")]
    UnclosedReference,

    #[error("env source separator must not be empty")]
    InvalidSeparator,
}
