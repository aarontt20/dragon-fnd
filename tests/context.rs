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

    let ctx = AppContext::builder().with_config(config).build_sync();

    assert_eq!(ctx.config().name, "test");
    assert_eq!(ctx.config().port, 3000);
}

#[test]
fn build_sync_is_infallible() {
    // build_sync() returns AppContext<C> directly, not Result.
    // This test verifies the return type by assigning without ? or unwrap().
    let ctx: AppContext<String> = AppContext::builder()
        .with_config("hello".to_string())
        .build_sync();

    assert_eq!(ctx.config(), "hello");
}

#[test]
fn app_context_debug_output() {
    let ctx = AppContext::builder()
        .with_config(42u32)
        .build_sync();

    let debug = format!("{:?}", ctx);
    assert!(debug.contains("AppContext"));
    assert!(debug.contains("42"));
}
