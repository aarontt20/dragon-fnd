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

    #[cfg(feature = "http")]
    #[error("http error: {0}")]
    Http(#[from] crate::http::HttpError),

    #[cfg(feature = "shutdown")]
    #[error("shutdown error: {0}")]
    Shutdown(#[from] crate::shutdown::ShutdownError),

    #[cfg(feature = "sqlite")]
    #[error("sqlite error: {0}")]
    Sqlite(#[from] crate::sqlite::SqliteError),
}
