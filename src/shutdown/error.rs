use std::io;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Errors from the shutdown subsystem.
///
/// Derives `Clone` because `wait()` shares its result via `OnceCell` —
/// multiple callers receive the same result. `Arc<io::Error>` wraps the
/// non-`Clone` `io::Error` to enable this (standard pattern: hyper, tonic).
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum ShutdownError {
    #[error("failed to install signal handler")]
    SignalHandler {
        #[source]
        source: Arc<io::Error>,
    },

    #[error("cleanup grace period exceeded ({} completed, {} panicked, {} remaining)",
        completed.len(), panicked.len(), remaining.len())]
    GracePeriodExceeded {
        elapsed: Duration,
        completed: Vec<String>,
        panicked: Vec<String>,
        remaining: Vec<String>,
    },

    #[error("shutdown already triggered, cleanup hook will not run; \
             call the hook directly if cleanup is still needed")]
    AlreadyTriggered,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn signal_handler_display() {
        let err = ShutdownError::SignalHandler {
            source: Arc::new(io::Error::new(io::ErrorKind::PermissionDenied, "access denied")),
        };
        assert_eq!(err.to_string(), "failed to install signal handler");
        assert!(err.source().is_some());
    }

    #[test]
    fn signal_handler_source_chain() {
        let io_err = io::Error::new(io::ErrorKind::Other, "no signals");
        let err = ShutdownError::SignalHandler {
            source: Arc::new(io_err),
        };
        let source = err.source().unwrap();
        assert_eq!(source.to_string(), "no signals");
    }

    #[test]
    fn grace_period_exceeded_display() {
        let err = ShutdownError::GracePeriodExceeded {
            elapsed: Duration::from_secs(30),
            completed: vec!["db-flush".into()],
            panicked: vec!["bad-hook".into()],
            remaining: vec!["cache-clear".into(), "log-sync".into()],
        };
        assert_eq!(
            err.to_string(),
            "cleanup grace period exceeded (1 completed, 1 panicked, 2 remaining)"
        );
    }

    #[test]
    fn already_triggered_display() {
        let err = ShutdownError::AlreadyTriggered;
        assert_eq!(
            err.to_string(),
            "shutdown already triggered, cleanup hook will not run; \
             call the hook directly if cleanup is still needed"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn clone() {
        let err = ShutdownError::SignalHandler {
            source: Arc::new(io::Error::new(io::ErrorKind::Other, "test")),
        };
        let cloned = err.clone();
        assert_eq!(err.to_string(), cloned.to_string());
    }

    #[test]
    fn debug() {
        let err = ShutdownError::AlreadyTriggered;
        let debug = format!("{:?}", err);
        assert!(debug.contains("AlreadyTriggered"));
    }

    #[test]
    fn top_level_error_from_shutdown() {
        let shutdown_err = ShutdownError::AlreadyTriggered;
        let err: crate::Error = shutdown_err.into();
        assert_eq!(
            err.to_string(),
            "shutdown error: shutdown already triggered, cleanup hook will not run; \
             call the hook directly if cleanup is still needed"
        );
    }
}
