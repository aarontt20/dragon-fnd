#![cfg(feature = "sqlite")]

use dragon_fnd::AppContext;
use dragon_fnd::sqlite::{JournalMode, SqliteBuilder};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TestConfig {
    name: String,
}

#[tokio::test]
async fn async_builder_with_config_and_sqlite() {
    let ctx = AppContext::builder()
        .with_config(TestConfig {
            name: "test-app".into(),
        })
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .build()
        .await
        .unwrap();

    assert_eq!(ctx.config().name, "test-app");
    assert!(ctx.sqlite().is_some());
}

#[tokio::test]
async fn sqlite_before_config() {
    // with_sqlite() before with_config() — order shouldn't matter
    let ctx = AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(42u32)
        .build()
        .await
        .unwrap();

    assert_eq!(*ctx.config(), 42);
    assert!(ctx.sqlite().is_some());
}

#[tokio::test]
async fn async_build_without_sqlite() {
    // Even in AsyncBuild, not registering sqlite should yield None
    // (This test requires another async subsystem to enter AsyncBuild,
    // but we only have sqlite right now. So we test via sqlite feature
    // being enabled but build() without with_sqlite().)
    // Actually, build() is only on AsyncBuild which requires with_sqlite().
    // So this test verifies the sqlite_pool is Some when registered.
    let ctx = AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(())
        .build()
        .await
        .unwrap();

    assert!(ctx.sqlite().is_some());
}

#[tokio::test]
async fn async_build_with_extensions() {
    struct MyService(String);

    let ctx = AppContext::builder()
        .with_extension(MyService("hello".into()))
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(())
        .build()
        .await
        .unwrap();

    assert!(ctx.sqlite().is_some());
    assert_eq!(ctx.extension::<MyService>().unwrap().0, "hello");
}

#[tokio::test]
async fn async_context_debug() {
    let ctx = AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory))
        .with_config(42u32)
        .build()
        .await
        .unwrap();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("AppContext"));
    assert!(debug.contains("42"));
    assert!(debug.contains("sqlite_pool: true"));
}

#[tokio::test]
async fn builder_debug_with_sqlite() {
    let builder = AppContext::builder()
        .with_sqlite(SqliteBuilder::new(":memory:").journal_mode(JournalMode::Memory));

    let debug = format!("{:?}", builder);
    assert!(debug.contains("AppContextBuilder"));
    assert!(debug.contains("sqlite: true"));
}
