use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during SQLite pool initialization and migration.
///
/// All variants carry enough context to identify the failure without
/// inspecting the source chain. Inner `sqlx` errors use `#[source]` (not
/// `#[from]`) since multiple variants wrap the same type with different
/// semantic meanings.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SqliteError {
    #[error("database path cannot be empty")]
    EmptyPath,

    #[error("invalid sqlite configuration: {field} — {reason}")]
    InvalidConfig {
        field: &'static str,
        reason: String,
    },

    #[error("failed to create database directory '{}'", dir.display())]
    DirectoryCreationFailed {
        dir: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create database connection pool")]
    PoolCreationFailed {
        #[source]
        source: sqlx::Error,
    },

    #[error("database connectivity test failed")]
    ConnectivityTestFailed {
        #[source]
        source: sqlx::Error,
    },

    #[error("migrations directory not found: '{}'", _0.display())]
    MigrationsDirNotFound(PathBuf),

    #[error("database migration failed")]
    MigrationFailed {
        #[source]
        source: sqlx::migrate::MigrateError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn empty_path_display() {
        let err = SqliteError::EmptyPath;
        assert_eq!(err.to_string(), "database path cannot be empty");
        assert!(err.source().is_none());
    }

    #[test]
    fn invalid_config_display() {
        let err = SqliteError::InvalidConfig {
            field: "max_connections",
            reason: "must be at least 1".into(),
        };
        assert_eq!(
            err.to_string(),
            "invalid sqlite configuration: max_connections — must be at least 1"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn migrations_dir_not_found_display() {
        let err = SqliteError::MigrationsDirNotFound(PathBuf::from("/data/migrations"));
        assert_eq!(
            err.to_string(),
            "migrations directory not found: '/data/migrations'"
        );
    }

    #[test]
    fn directory_creation_failed_display() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = SqliteError::DirectoryCreationFailed {
            dir: PathBuf::from("/data/db"),
            source: io_err,
        };
        assert_eq!(
            err.to_string(),
            "failed to create database directory '/data/db'"
        );
        assert!(err.source().is_some());
    }

    #[test]
    fn top_level_error_from_sqlite() {
        let sqlite_err = SqliteError::EmptyPath;
        let err: crate::Error = sqlite_err.into();
        assert_eq!(err.to_string(), "sqlite error: database path cannot be empty");
    }
}
