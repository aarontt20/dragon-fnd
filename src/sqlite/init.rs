use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use super::config::{JournalMode, SqliteConfig};
use super::error::SqliteError;

pub(crate) async fn init_pool(config: &SqliteConfig) -> Result<SqlitePool, SqliteError> {
    // 1. Validate path
    if config.path.is_empty() {
        return Err(SqliteError::EmptyPath);
    }

    let is_memory = config.path == ":memory:";

    // 2. Warn if WAL + :memory:
    if config.journal_mode == JournalMode::Wal && is_memory {
        #[cfg(feature = "logging")]
        tracing::warn!(
            "journal_mode is set to WAL but database is in-memory — \
             SQLite will silently fall back to memory journal mode"
        );
        #[cfg(not(feature = "logging"))]
        eprintln!(
            "warning: journal_mode is set to WAL but database is in-memory — \
             SQLite will silently fall back to memory journal mode"
        );
    }

    // 3. Create parent directory for file-based databases
    if !is_memory {
        let path = std::path::Path::new(&config.path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|source| {
                    SqliteError::DirectoryCreationFailed {
                        dir: parent.to_path_buf(),
                        source,
                    }
                })?;
            }
        }
    }

    // 4. Build connection options
    let journal_mode = match config.journal_mode {
        JournalMode::Wal => SqliteJournalMode::Wal,
        JournalMode::Delete => SqliteJournalMode::Delete,
        JournalMode::Memory => SqliteJournalMode::Memory,
    };

    let connect_options = SqliteConnectOptions::from_str(&format!("sqlite:{}", config.path))
        .map_err(|source| SqliteError::PoolCreationFailed { source })?
        .create_if_missing(true)
        .journal_mode(journal_mode)
        .foreign_keys(config.foreign_keys)
        .busy_timeout(Duration::from_secs(config.busy_timeout_secs));

    // 5. Create pool
    let pool = SqlitePoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .connect_with(connect_options)
        .await
        .map_err(|source| SqliteError::PoolCreationFailed { source })?;

    // 6. Test connectivity
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(|source| SqliteError::ConnectivityTestFailed { source })?;

    // 7. Run migrations if enabled
    if config.migrate {
        let migrations_path = &config.migrations_dir;
        if !migrations_path.exists() {
            return Err(SqliteError::MigrationsDirNotFound(
                migrations_path.to_path_buf(),
            ));
        }

        let migrator = sqlx::migrate::Migrator::new(migrations_path.as_path())
            .await
            .map_err(|source| SqliteError::MigrationFailed { source })?;

        migrator
            .run(&pool)
            .await
            .map_err(|source| SqliteError::MigrationFailed { source })?;
    }

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn memory_config() -> SqliteConfig {
        SqliteConfig {
            path: ":memory:".into(),
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        }
    }

    #[tokio::test]
    async fn init_memory_database() {
        let config = memory_config();
        let pool = init_pool(&config).await.unwrap();
        let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn empty_path_rejected() {
        let config = SqliteConfig::default(); // path is ""
        let err = init_pool(&config).await.unwrap_err();
        assert!(matches!(err, SqliteError::EmptyPath));
    }

    #[tokio::test]
    async fn foreign_keys_enabled() {
        let config = SqliteConfig {
            path: ":memory:".into(),
            foreign_keys: true,
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        };
        let pool = init_pool(&config).await.unwrap();
        let row: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, 1);
    }

    #[tokio::test]
    async fn foreign_keys_disabled() {
        let config = SqliteConfig {
            path: ":memory:".into(),
            foreign_keys: false,
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        };
        let pool = init_pool(&config).await.unwrap();
        let row: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn busy_timeout_is_set() {
        let config = SqliteConfig {
            path: ":memory:".into(),
            busy_timeout_secs: 10,
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        };
        let pool = init_pool(&config).await.unwrap();
        let row: (i64,) = sqlx::query_as("PRAGMA busy_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, 10000); // milliseconds
    }

    #[tokio::test]
    async fn migrations_dir_missing() {
        let config = SqliteConfig {
            path: ":memory:".into(),
            migrate: true,
            migrations_dir: PathBuf::from("/nonexistent/migrations"),
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        };
        let err = init_pool(&config).await.unwrap_err();
        assert!(matches!(err, SqliteError::MigrationsDirNotFound(_)));
    }

    #[tokio::test]
    async fn file_based_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("subdir").join("test.db");

        let config = SqliteConfig {
            path: db_path.to_str().unwrap().into(),
            journal_mode: JournalMode::Delete,
            ..SqliteConfig::default()
        };
        let pool = init_pool(&config).await.unwrap();
        let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, 1);
        assert!(tmp.path().join("subdir").exists());
    }

    #[tokio::test]
    async fn migrations_run_successfully() {
        let tmp = tempfile::tempdir().unwrap();
        let migrations_dir = tmp.path().join("migrations");
        std::fs::create_dir(&migrations_dir).unwrap();
        std::fs::write(
            migrations_dir.join("001_create_users.sql"),
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
        )
        .unwrap();

        let config = SqliteConfig {
            path: ":memory:".into(),
            migrate: true,
            migrations_dir,
            journal_mode: JournalMode::Memory,
            ..SqliteConfig::default()
        };
        let pool = init_pool(&config).await.unwrap();

        // Verify the table was created
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, 0);
    }
}
