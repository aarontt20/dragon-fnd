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

    #[error("failed to parse config file '{path}': {source}")]
    ParseError {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("failed to deserialize config: {0}")]
    DeserializeError(#[from] toml::de::Error),

    #[error("circular reference detected in configuration")]
    CircularReference,

    #[error("referenced path not found: {0}")]
    ReferenceNotFound(String),

    #[error("invalid reference path: {0}")]
    InvalidReferencePath(String),

    #[error("cannot reference non-scalar value: {0}")]
    NonScalarReference(String),

    #[error("unclosed reference (missing '}}')")]
    UnclosedReference,
}
