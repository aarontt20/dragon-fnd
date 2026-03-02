#![cfg(feature = "sqlite")]

use dragon_fnd::sqlite::{JournalMode, SqliteBuilder, SqliteConfig, SqlitePool};

// -- Pool creation and connectivity --

#[tokio::test]
async fn memory_pool_creation() {
    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().expect("sqlite was registered");
    let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(pool).await.unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn file_pool_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("test.db");

    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(db_path.to_str().unwrap()).journal_mode(JournalMode::Delete),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (i64,) = sqlx::query_as("SELECT 42")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, 42);
    assert!(db_path.exists());
}

// -- PRAGMA verification --

#[tokio::test]
async fn pragma_foreign_keys_enabled() {
    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(":memory:")
                .journal_mode(JournalMode::Memory)
                .foreign_keys(true),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn pragma_foreign_keys_disabled() {
    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(":memory:")
                .journal_mode(JournalMode::Memory)
                .foreign_keys(false),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);
}

#[tokio::test]
async fn pragma_busy_timeout() {
    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(":memory:")
                .journal_mode(JournalMode::Memory)
                .busy_timeout_secs(7),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (i64,) = sqlx::query_as("PRAGMA busy_timeout")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, 7000); // stored in milliseconds
}

#[tokio::test]
async fn pragma_journal_mode_wal_requires_file() {
    // WAL journal mode needs a file-based database — :memory: doesn't support it
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("wal_test.db");

    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(db_path.to_str().unwrap()).journal_mode(JournalMode::Wal),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, "wal");
}

// -- Migrations --

#[tokio::test]
async fn migrations_from_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let migrations_dir = tmp.path().join("migrations");
    std::fs::create_dir(&migrations_dir).unwrap();
    std::fs::write(
        migrations_dir.join("001_init.sql"),
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
    )
    .unwrap();

    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(
            SqliteBuilder::new(":memory:")
                .journal_mode(JournalMode::Memory)
                .migrate(true)
                .migrations_dir(&migrations_dir),
        )
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();

    // Insert and query to verify migration worked
    sqlx::query("INSERT INTO items (name) VALUES ('test')")
        .execute(pool)
        .await
        .unwrap();
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM items")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1);
}

// -- from_config bridge --

#[tokio::test]
async fn from_config_via_toml() {
    let toml = r#"
        path = ":memory:"
        journal_mode = "memory"
        max_connections = 3
    "#;
    let config: SqliteConfig = toml::from_str(toml).unwrap();

    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(SqliteBuilder::from_config(config))
        .with_config(())
        .build()
        .await
        .unwrap();

    let pool = ctx.sqlite().unwrap();
    let row: (i64,) = sqlx::query_as("SELECT 1").fetch_one(pool).await.unwrap();
    assert_eq!(row.0, 1);
}

// -- SqlitePool re-export --

#[tokio::test]
async fn sqlite_pool_reexport_is_usable() {
    let ctx = dragon_fnd::AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(())
        .build()
        .await
        .unwrap();

    // Verify the re-exported type works — users can write SqlitePool
    // without depending on sqlx directly
    let pool: &SqlitePool = ctx.sqlite().unwrap();
    let _: (i64,) = sqlx::query_as("SELECT 1").fetch_one(pool).await.unwrap();
}
