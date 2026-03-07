use std::sync::Arc;

use super::error::ShutdownError;

/// Pre-installed signal handler state consumed by `wait_for_signal()`.
#[cfg(unix)]
pub(crate) struct SignalState {
    sigterm: tokio::signal::unix::Signal,
}

#[cfg(not(unix))]
pub(crate) struct SignalState {}

/// Pre-install signal handlers. Called from `init_shutdown()`.
///
/// Eagerly installs handlers so failures surface at build time, not when
/// the first signal arrives.
pub(crate) fn install_signal_handlers() -> Result<SignalState, ShutdownError> {
    #[cfg(unix)]
    {
        let sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| ShutdownError::SignalHandler {
                source: Arc::new(e),
            })?;
        Ok(SignalState { sigterm })
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
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::select! {
            result = ctrl_c => {
                result.map_err(|e| ShutdownError::SignalHandler {
                    source: Arc::new(e),
                })?;
            }
            _ = state.sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = state;
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| ShutdownError::SignalHandler {
                source: Arc::new(e),
            })?;
    }

    Ok(())
}
