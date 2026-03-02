use std::path::PathBuf;

use super::config::{JournalMode, SqliteConfig};

/// Fluent builder for SQLite pool configuration.
///
/// This is what [`AppContextBuilder::with_sqlite()`](crate::context::AppContextBuilder) accepts.
/// Use [`SqliteBuilder::new()`] for programmatic construction, or
/// [`SqliteBuilder::from_config()`] to bridge from a deserialized [`SqliteConfig`].
#[derive(Debug, Clone)]
#[must_use = "builders do nothing until passed to AppContextBuilder::with_sqlite()"]
pub struct SqliteBuilder {
    config: SqliteConfig,
}

impl SqliteBuilder {
    /// Creates a new builder with the given database path and sensible defaults.
    ///
    /// Use `":memory:"` for an in-memory database. For file-based databases,
    /// the parent directory is created automatically at init time.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            config: SqliteConfig {
                path: path.into(),
                ..SqliteConfig::default()
            },
        }
    }

    /// Creates a builder from a deserialized [`SqliteConfig`].
    ///
    /// Bridges TOML configuration into the builder API.
    pub fn from_config(config: SqliteConfig) -> Self {
        Self { config }
    }

    /// Enables or disables running migrations at pool init time.
    pub fn migrate(mut self, enable: bool) -> Self {
        self.config.migrate = enable;
        self
    }

    /// Sets the directory containing SQL migration files.
    pub fn migrations_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.migrations_dir = dir.into();
        self
    }

    /// Sets the maximum number of connections in the pool (default: 5, must be >= 1).
    pub fn max_connections(mut self, n: u32) -> Self {
        self.config.max_connections = n;
        self
    }

    /// Sets the minimum number of idle connections maintained by the pool (default: 1).
    pub fn min_connections(mut self, n: u32) -> Self {
        self.config.min_connections = n;
        self
    }

    /// Sets how long to wait for a connection from the pool before failing (default: 10s).
    pub fn acquire_timeout_secs(mut self, secs: u64) -> Self {
        self.config.acquire_timeout_secs = secs;
        self
    }

    /// Sets how long an idle connection is kept before being closed (default: 300s).
    pub fn idle_timeout_secs(mut self, secs: u64) -> Self {
        self.config.idle_timeout_secs = secs;
        self
    }

    /// Sets the SQLite journal mode (default: [`JournalMode::Wal`]).
    pub fn journal_mode(mut self, mode: JournalMode) -> Self {
        self.config.journal_mode = mode;
        self
    }

    /// Enables or disables foreign key enforcement (default: true).
    pub fn foreign_keys(mut self, enable: bool) -> Self {
        self.config.foreign_keys = enable;
        self
    }

    /// Sets how long SQLite waits when the database is locked (default: 5s).
    pub fn busy_timeout_secs(mut self, secs: u64) -> Self {
        self.config.busy_timeout_secs = secs;
        self
    }

    pub(crate) fn into_config(self) -> SqliteConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_path_with_defaults() {
        let config = SqliteBuilder::new("test.db").into_config();
        assert_eq!(config.path, "test.db");
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.journal_mode, JournalMode::Wal);
        assert!(config.foreign_keys);
        assert_eq!(config.busy_timeout_secs, 5);
    }

    #[test]
    fn from_config_preserves_values() {
        let original = SqliteConfig {
            path: "custom.db".into(),
            max_connections: 20,
            ..SqliteConfig::default()
        };
        let config = SqliteBuilder::from_config(original.clone()).into_config();
        assert_eq!(config, original);
    }

    #[test]
    fn fluent_chain() {
        let config = SqliteBuilder::new("app.db")
            .migrate(true)
            .migrations_dir("/opt/migrations")
            .max_connections(10)
            .min_connections(2)
            .acquire_timeout_secs(30)
            .idle_timeout_secs(600)
            .journal_mode(JournalMode::Delete)
            .foreign_keys(false)
            .busy_timeout_secs(15)
            .into_config();

        assert_eq!(config.path, "app.db");
        assert!(config.migrate);
        assert_eq!(config.migrations_dir, PathBuf::from("/opt/migrations"));
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.acquire_timeout_secs, 30);
        assert_eq!(config.idle_timeout_secs, 600);
        assert_eq!(config.journal_mode, JournalMode::Delete);
        assert!(!config.foreign_keys);
        assert_eq!(config.busy_timeout_secs, 15);
    }

    #[test]
    fn new_accepts_string_types() {
        // &str
        let _ = SqliteBuilder::new("test.db");
        // String
        let _ = SqliteBuilder::new(String::from("test.db"));
    }

    #[test]
    fn memory_database() {
        let config = SqliteBuilder::new(":memory:").into_config();
        assert_eq!(config.path, ":memory:");
    }
}
