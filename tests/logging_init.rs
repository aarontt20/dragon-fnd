#![cfg(feature = "logging")]

use dragon_fnd::logging::{FileBuilder, LogFormat, LoggingBuilder, Rotation};
use dragon_fnd::AppContext;

#[test]
fn build_sync_with_console_logging() {
    let result = AppContext::builder()
        .with_logging(LoggingBuilder::new().filter("warn"))
        .with_config("hello".to_string())
        .build_sync();
    // Config is valid; subscriber may or may not be set by another test
    match result {
        Ok(_) => {}
        Err(dragon_fnd::Error::Logging(dragon_fnd::logging::LoggingError::SubscriberAlreadySet)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
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

    let result = AppContext::builder()
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

    // Config is valid; subscriber may or may not be set by another test
    match result {
        Ok(_) => {
            assert!(dir.path().join("logs").exists());
        }
        Err(dragon_fnd::Error::Logging(dragon_fnd::logging::LoggingError::SubscriberAlreadySet)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
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
        .with_logging(LoggingBuilder::from_config(config))
        .with_config("test".to_string())
        .build_sync();

    // Config is valid (file disabled skips validation); subscriber may already be set
    match result {
        Ok(_) => {}
        Err(dragon_fnd::Error::Logging(dragon_fnd::logging::LoggingError::SubscriberAlreadySet)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn invalid_retention_config_errors() {
    let mut config = dragon_fnd::logging::LoggingConfig::default();
    config.file.enabled = true;
    config.file.retain_days = Some(7);
    config.file.retain_files = Some(10);

    let result = AppContext::builder()
        .with_logging(LoggingBuilder::from_config(config))
        .with_config("test".to_string())
        .build_sync();

    assert!(result.is_err());
}

#[test]
fn build_sync_with_size_rotation_creates_dir_and_active_file() {
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join("logs");

    let result = AppContext::builder()
        .with_logging(
            LoggingBuilder::new().filter("info").file(
                FileBuilder::new(&log_dir)
                    .prefix("app")
                    .rotation(Rotation::Never)
                    .max_bytes(10_485_760),
            ),
        )
        .with_config("test".to_string())
        .build_sync();

    // Config is valid; subscriber may or may not be set by another test.
    // File assertions only apply when subscriber was successfully set.
    match result {
        Ok(_) => {
            assert!(log_dir.exists(), "log directory should be created");
            assert!(log_dir.join("app").exists(), "active log file should exist");
        }
        Err(dragon_fnd::Error::Logging(dragon_fnd::logging::LoggingError::SubscriberAlreadySet)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn build_sync_with_size_rotation_and_compress_and_retain() {
    let dir = tempfile::tempdir().unwrap();
    let log_dir = dir.path().join("logs");

    let result = AppContext::builder()
        .with_logging(
            LoggingBuilder::new().filter("info").file(
                FileBuilder::new(&log_dir)
                    .prefix("app")
                    .rotation(Rotation::Never)
                    .max_bytes(10_485_760)
                    .compress(true)
                    .retain_files(5),
            ),
        )
        .with_config("test".to_string())
        .build_sync();

    // Config is valid; subscriber may or may not be set by another test
    match result {
        Ok(_) => {}
        Err(dragon_fnd::Error::Logging(dragon_fnd::logging::LoggingError::SubscriberAlreadySet)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn invalid_max_bytes_with_time_rotation_errors_through_app_context() {
    let dir = tempfile::tempdir().unwrap();

    let result = AppContext::builder()
        .with_logging(
            LoggingBuilder::new().filter("info").file(
                FileBuilder::new(dir.path())
                    .prefix("app")
                    .rotation(Rotation::Daily)
                    .max_bytes(10_485_760),
            ),
        )
        .with_config("test".to_string())
        .build_sync();

    assert!(result.is_err());
}
