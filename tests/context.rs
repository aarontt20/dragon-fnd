use dragon_fnd::{AppContext, ConfigBuilder};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TestConfig {
    name: String,
    port: u16,
}

#[test]
fn builder_chain_with_config_and_build_sync() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"test\"\nport = 3000\n").unwrap();

    let config: TestConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .build()
        .unwrap();

    let ctx = AppContext::builder()
        .with_config(config)
        .build_sync()
        .unwrap();

    assert_eq!(ctx.config().name, "test");
    assert_eq!(ctx.config().port, 3000);
}

#[test]
fn build_sync_returns_result() {
    // build_sync() returns Result<AppContext<C>, Error>.
    let ctx = AppContext::builder()
        .with_config("hello".to_string())
        .build_sync()
        .unwrap();

    assert_eq!(ctx.config(), "hello");
}

#[test]
fn app_context_debug_output() {
    let ctx = AppContext::builder()
        .with_config(42u32)
        .build_sync()
        .unwrap();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("AppContext"));
    assert!(debug.contains("42"));
}

#[test]
fn extension_store_and_retrieve() {
    struct MyPool(String);

    let ctx = AppContext::builder()
        .with_config("test".to_string())
        .with_extension(MyPool("postgres://localhost".to_string()))
        .build_sync()
        .unwrap();

    let pool = ctx.extension::<MyPool>().unwrap();
    assert_eq!(pool.0, "postgres://localhost");
}

#[test]
fn extension_missing_returns_none() {
    struct NotRegistered;

    let ctx = AppContext::builder()
        .with_config("test".to_string())
        .build_sync()
        .unwrap();

    assert!(ctx.extension::<NotRegistered>().is_none());
}

#[test]
fn extension_last_writer_wins() {
    struct Counter(u32);

    let ctx = AppContext::builder()
        .with_config("test".to_string())
        .with_extension(Counter(1))
        .with_extension(Counter(2))
        .build_sync()
        .unwrap();

    assert_eq!(ctx.extension::<Counter>().unwrap().0, 2);
}

#[test]
fn extension_before_config() {
    struct Tag(&'static str);

    let ctx = AppContext::builder()
        .with_extension(Tag("early"))
        .with_config(42u32)
        .build_sync()
        .unwrap();

    assert_eq!(ctx.extension::<Tag>().unwrap().0, "early");
}

#[test]
fn extension_debug_shows_count() {
    struct Ext1;
    struct Ext2;

    let ctx = AppContext::builder()
        .with_config(42u32)
        .with_extension(Ext1)
        .with_extension(Ext2)
        .build_sync()
        .unwrap();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("extensions: 2"));
}
