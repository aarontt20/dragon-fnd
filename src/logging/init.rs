use std::fs;
use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::Layer;

use super::config::{FileConfig, LogFormat, LoggingConfig, RotationStrategy};
use super::error::LoggingError;
use super::retain;
use super::writer::SizeRotatingWriter;

/// Initialize the tracing subscriber based on the provided logging configuration.
///
/// Returns `Ok(Some(guard))` when file logging is enabled (the guard must be held
/// for the application lifetime to ensure log flushing). Returns `Ok(None)` when
/// logging is disabled or console-only. Returns `Err(SubscriberAlreadySet)` if the
/// global subscriber was already set.
pub(crate) fn init_logging(config: &LoggingConfig) -> Result<Option<WorkerGuard>, LoggingError> {
    if !config.enabled {
        return Ok(None);
    }

    // Validate file config (only when file logging is active)
    if config.file.enabled {
        // Size-based validation
        if let RotationStrategy::SizeBased { max_bytes } = config.file.rotation {
            if max_bytes < 4096 {
                return Err(LoggingError::InvalidRotation(
                    "max_bytes must be at least 4096".to_string(),
                ));
            }
        }

        // Compress requires some form of rotation
        if config.file.compress && config.file.rotation == RotationStrategy::Never {
            return Err(LoggingError::InvalidRotation(
                "compress requires rotation to be enabled (set a rotation strategy other than never)"
                    .to_string(),
            ));
        }

        // Retention rules
        if config.file.retain_days.is_some() && config.file.retain_files.is_some() {
            return Err(LoggingError::InvalidRetention(
                "retain_days and retain_files are mutually exclusive".to_string(),
            ));
        }
        if config.file.retain_days == Some(0) {
            return Err(LoggingError::InvalidRetention(
                "retain_days must be at least 1".to_string(),
            ));
        }
        if config.file.retain_files == Some(0) {
            return Err(LoggingError::InvalidRetention(
                "retain_files must be at least 1".to_string(),
            ));
        }
        if config.file.rotation == RotationStrategy::Never
            && (config.file.retain_days.is_some() || config.file.retain_files.is_some())
        {
            return Err(LoggingError::InvalidRetention(
                "retention policies require rotation to be enabled".to_string(),
            ));
        }
    }

    // Build base filter from config
    let base_filter = build_env_filter(&config.filter, &config.modules)?;

    // Collect layers into a Vec — this avoids the type mismatch from chaining
    // multiple .with() calls (each changes the subscriber type).
    let mut layers: Vec<Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync>> = Vec::new();
    let mut guard: Option<WorkerGuard> = None;
    let mut cleanup_errors: Vec<(PathBuf, std::io::Error)> = Vec::new();

    // Build console layer
    if config.console.enabled {
        let filter = match config.console.filter.as_deref() {
            Some(f) => EnvFilter::try_new(f).map_err(|e| LoggingError::InvalidFilter(e.to_string()))?,
            None => base_filter.clone(),
        };
        layers.push(build_console_layer(config.console.format, filter));
    }

    // Build file layer
    if config.file.enabled {
        let (layer, file_guard, errors) = build_file_layer(&config.file, &base_filter)?;
        layers.push(layer);
        guard = Some(file_guard);
        cleanup_errors = errors;
    }

    // Compose subscriber
    let subscriber = tracing_subscriber::registry().with(layers);

    // Try to set the global subscriber. If it's already set, surface the error —
    // silently ignoring means the user's logging configuration was not applied.
    match subscriber.try_init() {
        Ok(()) => {}
        Err(_) => {
            return Err(LoggingError::SubscriberAlreadySet);
        }
    }

    // Now that the subscriber is live, surface any retention cleanup errors
    for (path, err) in cleanup_errors {
        tracing::warn!(
            path = %path.display(),
            error = %err,
            "failed to remove old log file during retention cleanup"
        );
    }

    Ok(guard)
}

/// Build an `EnvFilter` from a base directive string and optional per-module overrides.
fn build_env_filter(
    base: &str,
    modules: &std::collections::BTreeMap<String, String>,
) -> Result<EnvFilter, LoggingError> {
    let mut directives = base.to_string();
    for (module, level) in modules {
        directives.push(',');
        directives.push_str(module);
        directives.push('=');
        directives.push_str(level);
    }
    EnvFilter::try_new(&directives).map_err(|e| LoggingError::InvalidFilter(e.to_string()))
}

/// Build a console fmt layer with the given format and filter.
fn build_console_layer(
    format: LogFormat,
    filter: EnvFilter,
) -> Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync> {
    let fmt = tracing_subscriber::fmt::layer().with_target(true);

    match format {
        LogFormat::Pretty => fmt.pretty().with_filter(filter).boxed(),
        LogFormat::Json => fmt.json().with_filter(filter).boxed(),
        LogFormat::Compact => fmt.compact().with_filter(filter).boxed(),
    }
}

/// Build a file fmt layer, creating the log directory and running retention cleanup.
///
/// Returns (layer, guard, cleanup_errors).
#[allow(clippy::type_complexity)]
fn build_file_layer(
    config: &FileConfig,
    base_filter: &EnvFilter,
) -> Result<
    (
        Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync>,
        WorkerGuard,
        Vec<(PathBuf, std::io::Error)>,
    ),
    LoggingError,
> {
    // Create log directory
    fs::create_dir_all(&config.dir).map_err(|e| LoggingError::FileSetupFailed {
        dir: config.dir.clone(),
        source: e,
    })?;

    // Run startup retention cleanup (only for time-based rotation;
    // size-based rotation handles retention inline after each rotation)
    let cleanup_errors = match &config.rotation {
        RotationStrategy::Daily | RotationStrategy::Hourly => {
            if config.retain_days.is_some() || config.retain_files.is_some() {
                retain::cleanup_old_logs(
                    &config.dir,
                    &config.prefix,
                    config.retain_days,
                    config.retain_files,
                )
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    // Create writer based on rotation strategy
    let (non_blocking, guard) = match &config.rotation {
        RotationStrategy::SizeBased { max_bytes } => {
            let writer = SizeRotatingWriter::new(
                config.dir.clone(),
                config.prefix.clone(),
                *max_bytes,
                config.compress,
                config.retain_days,
                config.retain_files,
            )?;
            tracing_appender::non_blocking(writer)
        }
        RotationStrategy::Daily => {
            let appender = tracing_appender::rolling::daily(&config.dir, &config.prefix);
            tracing_appender::non_blocking(appender)
        }
        RotationStrategy::Hourly => {
            let appender = tracing_appender::rolling::hourly(&config.dir, &config.prefix);
            tracing_appender::non_blocking(appender)
        }
        RotationStrategy::Never => {
            let appender = tracing_appender::rolling::never(&config.dir, &config.prefix);
            tracing_appender::non_blocking(appender)
        }
    };

    // Build per-layer filter
    let filter = match config.filter.as_deref() {
        Some(f) => EnvFilter::try_new(f).map_err(|e| LoggingError::InvalidFilter(e.to_string()))?,
        None => base_filter.clone(),
    };

    let fmt = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    // Note: this format-match mirrors build_console_layer but cannot be extracted —
    // each `fmt` variable has a different concrete type (different writer/ansi config).
    let layer: Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync> = match config.format {
        LogFormat::Pretty => fmt.pretty().with_filter(filter).boxed(),
        LogFormat::Json => fmt.json().with_filter(filter).boxed(),
        LogFormat::Compact => fmt.compact().with_filter(filter).boxed(),
    };

    Ok((layer, guard, cleanup_errors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Assert that init_logging accepted the config as valid.
    /// SubscriberAlreadySet is OK — it means validation passed but the global subscriber was taken.
    fn assert_config_accepted(result: Result<Option<WorkerGuard>, LoggingError>) {
        match result {
            Ok(_) => {}
            Err(LoggingError::SubscriberAlreadySet) => {}
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn build_env_filter_valid_base() {
        let filter = build_env_filter("info", &BTreeMap::new());
        assert!(filter.is_ok());
    }

    #[test]
    fn build_env_filter_with_modules() {
        let mut modules = BTreeMap::new();
        modules.insert("sqlx".to_string(), "warn".to_string());
        modules.insert("hyper".to_string(), "info".to_string());
        let filter = build_env_filter("debug", &modules);
        assert!(filter.is_ok());
    }

    #[test]
    fn build_env_filter_invalid_directive() {
        // EnvFilter is lenient with level names but rejects malformed directives
        let filter = build_env_filter("info,=invalid", &BTreeMap::new());
        assert!(filter.is_err());
        assert!(matches!(filter, Err(LoggingError::InvalidFilter(_))));
    }

    #[test]
    fn build_env_filter_invalid_module() {
        let mut modules = BTreeMap::new();
        modules.insert("sqlx".to_string(), "not_a_level!!!".to_string());
        let filter = build_env_filter("info", &modules);
        assert!(matches!(filter, Err(LoggingError::InvalidFilter(_))));
    }

    #[test]
    fn disabled_logging_returns_none() {
        let config = LoggingConfig {
            enabled: false,
            ..LoggingConfig::default()
        };
        let result = init_logging(&config).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn retention_validation_both_set() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.retain_days = Some(7);
        config.file.retain_files = Some(10);
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRetention(_))));
    }

    #[test]
    fn retention_validation_zero_days() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.retain_days = Some(0);
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRetention(_))));
    }

    #[test]
    fn retention_validation_zero_files() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.retain_files = Some(0);
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRetention(_))));
    }

    #[test]
    fn retention_validation_never_rotation_with_retention() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.rotation = RotationStrategy::Never;
        config.file.retain_days = Some(7);
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRetention(_))));
    }

    #[test]
    fn build_file_layer_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("nested/logs");
        let config = FileConfig {
            enabled: true,
            dir: log_dir.clone(),
            prefix: "test".to_string(),
            format: LogFormat::Json,
            filter: None,
            rotation: RotationStrategy::Daily,
            compress: false,
            retain_days: None,
            retain_files: None,
        };
        let base_filter = build_env_filter("info", &BTreeMap::new()).unwrap();
        let result = build_file_layer(&config, &base_filter);
        assert!(result.is_ok());
        assert!(log_dir.exists());
    }

    #[test]
    fn rotation_validation_max_bytes_too_small() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.rotation = RotationStrategy::SizeBased { max_bytes: 100 };
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRotation(_))));
    }

    #[test]
    fn rotation_validation_compress_without_rotation() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.compress = true;
        config.file.rotation = RotationStrategy::Never;
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRotation(_))));
    }

    #[test]
    fn rotation_validation_size_based_with_retain_files_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.dir = dir.path().to_path_buf();
        config.file.rotation = RotationStrategy::SizeBased {
            max_bytes: 10_485_760,
        };
        config.file.retain_files = Some(5);
        let result = init_logging(&config);
        // Should succeed — SizeBased provides rotation
        assert_config_accepted(result);
    }

    #[test]
    fn rotation_validation_size_based_with_retain_days_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.dir = dir.path().to_path_buf();
        config.file.rotation = RotationStrategy::SizeBased {
            max_bytes: 10_485_760,
        };
        config.file.retain_days = Some(7);
        let result = init_logging(&config);
        // Should succeed — SizeBased provides rotation, retain_days works
        assert_config_accepted(result);
    }

    #[test]
    fn rotation_validation_compress_with_size_based_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.dir = dir.path().to_path_buf();
        config.file.rotation = RotationStrategy::SizeBased {
            max_bytes: 10_485_760,
        };
        config.file.compress = true;
        let result = init_logging(&config);
        // Should succeed — compress + SizeBased is valid
        assert_config_accepted(result);
    }

    #[test]
    fn rotation_validation_max_bytes_boundary_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.dir = dir.path().to_path_buf();
        config.file.rotation = RotationStrategy::SizeBased { max_bytes: 4096 };
        let result = init_logging(&config);
        assert_config_accepted(result);
    }

    #[test]
    fn rotation_validation_max_bytes_boundary_fail() {
        let mut config = LoggingConfig::default();
        config.file.enabled = true;
        config.file.rotation = RotationStrategy::SizeBased { max_bytes: 4095 };
        let result = init_logging(&config);
        assert!(matches!(result, Err(LoggingError::InvalidRotation(_))));
    }
}
