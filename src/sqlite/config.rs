use serde::Deserialize;
use std::path::PathBuf;

/// SQLite database configuration, deserialized from the `[sqlite]` section.
///
/// All fields have serde defaults via `#[serde(default)]`. The `path` field
/// must be set before use — an empty path produces [`SqliteError::EmptyPath`](super::SqliteError::EmptyPath)
/// at init time.
///
/// Fields are `pub(crate)` — use [`SqliteBuilder`](super::SqliteBuilder) for
/// programmatic construction or deserialize from TOML via
/// [`SqliteBuilder::from_config()`](super::SqliteBuilder::from_config).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SqliteConfig {
    pub(crate) path: String,
    pub(crate) max_connections: u32,
    pub(crate) min_connections: u32,
    pub(crate) acquire_timeout_secs: u64,
    pub(crate) idle_timeout_secs: u64,
    pub(crate) migrate: bool,
    pub(crate) migrations_dir: PathBuf,
    pub(crate) journal_mode: JournalMode,
    pub(crate) foreign_keys: bool,
    pub(crate) busy_timeout_secs: u64,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            max_connections: 5,
            min_connections: 1,
            acquire_timeout_secs: 10,
            idle_timeout_secs: 300,
            migrate: false,
            migrations_dir: PathBuf::from("./migrations"),
            journal_mode: JournalMode::Wal,
            foreign_keys: true,
            busy_timeout_secs: 5,
        }
    }
}

/// SQLite journal mode, controlling how transactions are written to disk.
///
/// Deserialized from lowercase strings: `"wal"`, `"delete"`, `"memory"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalMode {
    /// Write-Ahead Logging — best for concurrent read/write workloads.
    /// Requires a file-based database; unsupported for `:memory:` (overridden
    /// to `Memory` with a warning).
    Wal,
    /// Classic rollback journal — deletes the journal after each transaction.
    Delete,
    /// In-memory journal — no disk I/O for journal. Appropriate for
    /// `:memory:` databases or when durability is not needed.
    Memory,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let config = SqliteConfig::default();
        assert_eq!(config.path, "");
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.acquire_timeout_secs, 10);
        assert_eq!(config.idle_timeout_secs, 300);
        assert!(!config.migrate);
        assert_eq!(config.migrations_dir, PathBuf::from("./migrations"));
        assert_eq!(config.journal_mode, JournalMode::Wal);
        assert!(config.foreign_keys);
        assert_eq!(config.busy_timeout_secs, 5);
    }

    #[test]
    fn deserialize_minimal_toml() {
        let toml = r#"path = "app.db""#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.path, "app.db");
        // All other fields should be defaults
        assert_eq!(config.max_connections, 5);
        assert_eq!(config.journal_mode, JournalMode::Wal);
        assert!(config.foreign_keys);
        assert_eq!(config.busy_timeout_secs, 5);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml = r#"
            path = "data/test.db"
            max_connections = 10
            min_connections = 2
            acquire_timeout_secs = 30
            idle_timeout_secs = 600
            migrate = true
            migrations_dir = "./db/migrations"
            journal_mode = "delete"
            foreign_keys = false
            busy_timeout_secs = 15
        "#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.path, "data/test.db");
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.acquire_timeout_secs, 30);
        assert_eq!(config.idle_timeout_secs, 600);
        assert!(config.migrate);
        assert_eq!(config.migrations_dir, PathBuf::from("./db/migrations"));
        assert_eq!(config.journal_mode, JournalMode::Delete);
        assert!(!config.foreign_keys);
        assert_eq!(config.busy_timeout_secs, 15);
    }

    #[test]
    fn deserialize_empty_table() {
        let toml = "";
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config, SqliteConfig::default());
    }

    #[test]
    fn journal_mode_wal() {
        let toml = r#"journal_mode = "wal""#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.journal_mode, JournalMode::Wal);
    }

    #[test]
    fn journal_mode_delete() {
        let toml = r#"journal_mode = "delete""#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.journal_mode, JournalMode::Delete);
    }

    #[test]
    fn journal_mode_memory() {
        let toml = r#"journal_mode = "memory""#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.journal_mode, JournalMode::Memory);
    }

    #[test]
    fn journal_mode_invalid() {
        let toml = r#"journal_mode = "truncate""#;
        let result: Result<SqliteConfig, _> = toml::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn memory_path() {
        let toml = r#"path = ":memory:""#;
        let config: SqliteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.path, ":memory:");
    }
}
