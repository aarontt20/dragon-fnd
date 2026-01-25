use crate::config::ConfigError;
use thiserror::Error;

/// Top-level error type for the dragon-fnd library.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("application context requires a configuration")]
    MissingConfig,
}
