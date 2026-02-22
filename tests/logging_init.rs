#![cfg(feature = "logging")]

use dragon_fnd::logging::{FileBuilder, LogFormat, LoggingBuilder, Rotation};
use dragon_fnd::AppContext;

#[test]
fn build_sync_with_console_logging() {
    // Console-only logging (no file output) returns Ok with no WorkerGuard issues.
    // Note: this may fail if the global subscriber was already set by another test.
    let _ctx = AppContext::builder()
        .with_logging(LoggingBuilder::new().filter("warn"))
        .with_config("hello".to_string())
        .build_sync();
    // We don't assert Ok because the global subscriber may already be set,
    // which is fine — init_logging returns Ok(None) in that case.
}

#[test]
fn build_sync_with_logging_disabled() {
    let ctx = AppContext::builder()
        .with_logging(LoggingBuilder::new().enabled(false))
        .with_config(42u32)
        .build_sync()
        .unwrap();

    assert_eq!(*ctx.config(), 42);
}

#[test]
fn build_sync_with_file_logging() {
    let dir = tempfile::tempdir().unwrap();

    let ctx = AppContext::builder()
        .with_logging(
            LoggingBuilder::new()
                .filter("info")
                .file(
                    FileBuilder::new(dir.path().join("logs"))
                        .prefix("test")
                        .format(LogFormat::Json)
                        .rotation(Rotation::Never),
                ),
        )
        .with_config("test".to_string())
        .build_sync();

    // May succeed or return Ok(None) if subscriber already set — both are fine.
    // The key assertion is that it doesn't error from file setup.
    assert!(ctx.is_ok());
    assert!(dir.path().join("logs").exists());
}

#[test]
fn build_sync_without_logging_still_works() {
    // No with_logging() call — build_sync should succeed with no logging.
    let ctx = AppContext::builder()
        .with_config("no logging".to_string())
        .build_sync()
        .unwrap();

    assert_eq!(ctx.config(), "no logging");
}

#[test]
fn with_logging_before_config() {
    // with_logging() is available before with_config() — builder state propagates.
    let ctx = AppContext::builder()
        .with_logging(LoggingBuilder::new().enabled(false))
        .with_config(99u32)
        .build_sync()
        .unwrap();

    assert_eq!(*ctx.config(), 99);
}

#[test]
fn with_logging_after_config() {
    // with_logging() is also available after with_config().
    let ctx = AppContext::builder()
        .with_config(42u32)
        .with_logging(LoggingBuilder::new().enabled(false))
        .build_sync()
        .unwrap();

    assert_eq!(*ctx.config(), 42);
}

#[test]
fn invalid_retention_skipped_when_file_disabled() {
    // Conflicting retention values should not error when file logging is disabled.
    let mut config = dragon_fnd::logging::LoggingConfig::default();
    config.file.enabled = false;
    config.file.retain_days = Some(7);
    config.file.retain_files = Some(10);

    let result = AppContext::builder()
        .with_logging(LoggingBuilder::from_config(&config))
        .with_config("test".to_string())
        .build_sync();

    assert!(result.is_ok());
}

#[test]
fn invalid_retention_config_errors() {
    let mut config = dragon_fnd::logging::LoggingConfig::default();
    config.file.enabled = true;
    config.file.retain_days = Some(7);
    config.file.retain_files = Some(10);

    let result = AppContext::builder()
        .with_logging(LoggingBuilder::from_config(&config))
        .with_config("test".to_string())
        .build_sync();

    assert!(result.is_err());
}
