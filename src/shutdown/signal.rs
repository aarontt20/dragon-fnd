use std::sync::Arc;

use super::error::ShutdownError;

/// Pre-installed signal handler state consumed by `wait_for_signal()`.
#[cfg(unix)]
pub(crate) struct SignalState {
    sigterm: tokio::signal::unix::Signal,
    sigint: tokio::signal::unix::Signal,
}

#[cfg(not(unix))]
pub(crate) struct SignalState {}

/// Pre-install signal handlers. Called from `init_shutdown()`.
///
/// Eagerly installs handlers so failures surface at build time, not when
/// the first signal arrives. On Unix, both SIGTERM and SIGINT are
/// pre-registered so that signals arriving before `wait()` are buffered.
pub(crate) fn install_signal_handlers() -> Result<SignalState, ShutdownError> {
    #[cfg(unix)]
    {
        let sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| ShutdownError::SignalHandler {
                source: Arc::new(e),
            })?;
        let sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .map_err(|e| ShutdownError::SignalHandler {
                source: Arc::new(e),
            })?;
        Ok(SignalState { sigterm, sigint })
    }

    #[cfg(not(unix))]
    {
        Ok(SignalState {})
    }
}

/// Block until a shutdown signal fires (SIGTERM/SIGINT on Unix, Ctrl+C elsewhere).
pub(crate) async fn wait_for_signal(state: &mut SignalState) -> Result<(), ShutdownError> {
    #[cfg(unix)]
    {
        tokio::select! {
            _ = state.sigint.recv() => {}
            _ = state.sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = state; // suppress unused warning — state is empty on non-unix
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| ShutdownError::SignalHandler {
                source: Arc::new(e),
            })?;
    }

    Ok(())
}
