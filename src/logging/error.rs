use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoggingError {
    #[error("invalid log filter directive: {0}")]
    InvalidFilter(String),

    #[error("invalid retention config: {0}")]
    InvalidRetention(String),

    #[error("failed to create log directory '{}': {source}", dir.display())]
    FileSetupFailed {
        dir: PathBuf,
        source: std::io::Error,
    },
}
