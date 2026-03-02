use crate::config::ConfigError;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[cfg(feature = "logging")]
    #[error("logging error: {0}")]
    Logging(#[from] crate::logging::LoggingError),

    #[cfg(feature = "sqlite")]
    #[error("sqlite error: {0}")]
    Sqlite(#[from] crate::sqlite::SqliteError),
}
