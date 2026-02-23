use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoggingError {
    #[error("invalid log filter directive: {0}")]
    InvalidFilter(String),

    #[error("invalid retention config: {0}")]
    InvalidRetention(String),

    #[error("invalid rotation config: {0}")]
    InvalidRotation(String),

    #[error("failed to create log directory '{}'", dir.display())]
    FileSetupFailed {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("global tracing subscriber already set — logging configuration was not applied")]
    SubscriberAlreadySet,
}
