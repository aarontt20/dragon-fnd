use std::fs;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::Layer;

use super::config::{FileConfig, LogFormat, LoggingConfig, Rotation};
use super::error::LoggingError;

/// Initialize the tracing subscriber based on the provided logging configuration.
///
/// Returns `Ok(Some(guard))` when file logging is enabled (the guard must be held
/// for the application lifetime to ensure log flushing). Returns `Ok(None)` when
/// logging is disabled, console-only, or the global subscriber was already set.
pub(crate) fn init_logging(config: &LoggingConfig) -> Result<Option<WorkerGuard>, LoggingError> {
    if !config.enabled {
        return Ok(None);
    }

    // Validate retention config (only when file logging is active)
    if config.file.enabled {
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
        if config.file.rotation == Rotation::Never
            && (config.file.retain_days.is_some() || config.file.retain_files.is_some())
        {
            return Err(LoggingError::InvalidRetention(
                "retention policies have no effect with Rotation::Never".to_string(),
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

    // Try to set the global subscriber. If it's already set, that's fine.
    match subscriber.try_init() {
        Ok(()) => {}
        Err(_) => {
            // Subscriber not installed, so we can't use tracing::warn! — fall back to stderr.
            for (path, err) in cleanup_errors {
                eprintln!(
                    "dragon-fnd: failed to remove old log file '{}': {err}",
                    path.display()
                );
            }
            return Ok(None);
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

    // Run retention cleanup before opening new log file
    let cleanup_errors = if config.rotation != Rotation::Never
        && (config.retain_days.is_some() || config.retain_files.is_some())
    {
        cleanup_old_logs(
            &config.dir,
            &config.prefix,
            config.retain_days,
            config.retain_files,
        )
    } else {
        Vec::new()
    };

    // Create rolling appender
    let appender = match config.rotation {
        Rotation::Daily => tracing_appender::rolling::daily(&config.dir, &config.prefix),
        Rotation::Hourly => tracing_appender::rolling::hourly(&config.dir, &config.prefix),
        Rotation::Never => tracing_appender::rolling::never(&config.dir, &config.prefix),
    };

    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

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

/// Scan a directory for log files matching the given prefix and delete files
/// according to the retention policy.
///
/// Returns a list of files that could not be deleted (path + error).
/// The caller is responsible for surfacing these errors.
fn cleanup_old_logs(
    dir: &Path,
    prefix: &str,
    retain_days: Option<u32>,
    retain_files: Option<u32>,
) -> Vec<(PathBuf, std::io::Error)> {
    let mut errors = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            // Nonexistent directory is a no-op (no files to clean).
            // Other errors (e.g. permission denied) are surfaced to the caller.
            if e.kind() != std::io::ErrorKind::NotFound {
                errors.push((dir.to_path_buf(), e));
            }
            return errors;
        }
    };

    // Collect matching files with their modification times
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let name = path.file_name()?.to_str()?;
            // Match "{prefix}." to avoid matching unrelated files (e.g. "app" matching "application.log").
            // Rotated files always have the format "{prefix}.{date}", so the dot is always present.
            let match_prefix = format!("{prefix}.");
            if !name.starts_with(&match_prefix) {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            Some((path, mtime))
        })
        .collect();

    // Sort by modification time, newest first
    files.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some(days) = retain_days {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(u64::from(days) * 24 * 3600);
        for (path, mtime) in &files {
            if *mtime < cutoff {
                if let Err(e) = fs::remove_file(path) {
                    errors.push((path.clone(), e));
                }
            }
        }
    }

    if let Some(max_files) = retain_files {
        let max = max_files as usize;
        if files.len() > max {
            for (path, _) in &files[max..] {
                if let Err(e) = fs::remove_file(path) {
                    errors.push((path.clone(), e));
                }
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
        assert!(filter.is_err());
    }

    #[test]
    fn disabled_logging_returns_none() {
        let mut config = LoggingConfig::default();
        config.enabled = false;
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
        config.file.rotation = Rotation::Never;
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
            rotation: Rotation::Daily,
            retain_days: None,
            retain_files: None,
        };
        let base_filter = build_env_filter("info", &BTreeMap::new()).unwrap();
        let result = build_file_layer(&config, &base_filter);
        assert!(result.is_ok());
        assert!(log_dir.exists());
    }

    #[test]
    fn cleanup_old_logs_days_retention() {
        let dir = tempfile::tempdir().unwrap();

        // Create files with different modification times
        let old_file = dir.path().join("app.2024-01-01.log");
        let new_file = dir.path().join("app.2024-12-01.log");
        fs::write(&old_file, "old").unwrap();
        fs::write(&new_file, "new").unwrap();

        // Set the old file's mtime to 30 days ago
        let old_time = std::time::SystemTime::now()
            - std::time::Duration::from_secs(30 * 24 * 3600);
        let times = fs::FileTimes::new().set_modified(old_time);
        fs::File::open(&old_file).unwrap().set_times(times).unwrap();

        let errors = cleanup_old_logs(dir.path(), "app", Some(7), None);
        assert!(errors.is_empty());
        assert!(!old_file.exists(), "old file should be deleted");
        assert!(new_file.exists(), "new file should be kept");
    }

    #[test]
    fn cleanup_old_logs_files_retention() {
        let dir = tempfile::tempdir().unwrap();

        // Create 5 files with staggered mtimes
        let mut files = Vec::new();
        for i in 0..5 {
            let path = dir.path().join(format!("app.{i}.log"));
            fs::write(&path, format!("log {i}")).unwrap();
            let mtime = std::time::SystemTime::now()
                - std::time::Duration::from_secs((5 - i) * 3600);
            let times = fs::FileTimes::new().set_modified(mtime);
            fs::File::open(&path).unwrap().set_times(times).unwrap();
            files.push(path);
        }

        // Keep only 3 most recent
        let errors = cleanup_old_logs(dir.path(), "app", None, Some(3));
        assert!(errors.is_empty());

        // Files 3, 4 (newest) should exist; 0, 1 (oldest) should be deleted
        assert!(!files[0].exists(), "oldest file should be deleted");
        assert!(!files[1].exists(), "second oldest should be deleted");
        assert!(files[2].exists(), "third newest should be kept");
        assert!(files[3].exists(), "second newest should be kept");
        assert!(files[4].exists(), "newest should be kept");
    }

    #[test]
    fn cleanup_old_logs_nonexistent_dir() {
        let errors = cleanup_old_logs(Path::new("/nonexistent/dir"), "app", Some(7), None);
        assert!(errors.is_empty());
    }

    #[test]
    fn cleanup_old_logs_non_matching_prefix_ignored() {
        let dir = tempfile::tempdir().unwrap();

        let matching = dir.path().join("app.old.log");
        let non_matching = dir.path().join("other.old.log");
        fs::write(&matching, "match").unwrap();
        fs::write(&non_matching, "no match").unwrap();

        // Set both to old times
        let old_time = std::time::SystemTime::now()
            - std::time::Duration::from_secs(30 * 24 * 3600);
        let times = fs::FileTimes::new().set_modified(old_time);
        fs::File::open(&matching)
            .unwrap()
            .set_times(times)
            .unwrap();
        fs::File::open(&non_matching)
            .unwrap()
            .set_times(times)
            .unwrap();

        let errors = cleanup_old_logs(dir.path(), "app", Some(7), None);
        assert!(errors.is_empty());
        assert!(!matching.exists(), "matching old file should be deleted");
        assert!(
            non_matching.exists(),
            "non-matching file should be untouched"
        );
    }
}
