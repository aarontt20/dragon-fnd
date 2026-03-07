use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::builder::ShutdownBuilder;
use super::error::ShutdownError;
use super::signal::{install_signal_handlers, wait_for_signal, SignalState};

type BoxedCleanupHook = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

struct ShutdownInner {
    token: CancellationToken,
    hooks: Mutex<Vec<(String, BoxedCleanupHook)>>,
    grace_period: Duration,
    result: tokio::sync::OnceCell<Result<(), ShutdownError>>,
    signal_state: Mutex<Option<SignalState>>,
}

/// Runtime handle for the shutdown subsystem.
///
/// Provides cancellation token distribution, cleanup hook registration,
/// and orchestrated shutdown via `wait()`.
pub struct Shutdown {
    inner: Arc<ShutdownInner>,
}

impl std::fmt::Debug for Shutdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shutdown")
            .field("is_triggered", &self.is_triggered())
            .field(
                "hooks",
                &self
                    .inner
                    .hooks
                    .lock()
                    .ok()
                    .map(|h| h.len())
                    .unwrap_or(0),
            )
            .field("grace_period", &self.inner.grace_period)
            .finish()
    }
}

impl Drop for Shutdown {
    fn drop(&mut self) {
        // Best-effort diagnostic: warn if hooks were registered but wait() was never called.
        // Uses try_lock to avoid panicking on a poisoned mutex during stack unwinding.
        if !self.inner.result.initialized() {
            if let Ok(hooks) = self.inner.hooks.try_lock() {
                let count = hooks.len();
                if count > 0 {
                    #[cfg(feature = "logging")]
                    tracing::warn!(
                        "shutdown handle dropped with {count} registered cleanup hooks \
                         that were never executed; call wait().await to run cleanup"
                    );
                    #[cfg(not(feature = "logging"))]
                    eprintln!(
                        "warning: shutdown handle dropped with {count} registered cleanup hooks \
                         that were never executed; call wait().await to run cleanup"
                    );
                }
            }
        }
    }
}

impl Shutdown {
    /// Returns a clone of the cancellation token.
    ///
    /// The token is `Send + Sync + Clone` — pass it to worker tasks,
    /// threads, or other subsystems to observe shutdown.
    pub fn token(&self) -> CancellationToken {
        self.inner.token.clone()
    }

    /// Programmatically trigger shutdown.
    ///
    /// Idempotent — calling on an already-cancelled token is a no-op.
    pub fn trigger(&self) {
        self.inner.token.cancel();
    }

    /// Check if shutdown has been triggered (signal or manual).
    pub fn is_triggered(&self) -> bool {
        self.inner.token.is_cancelled()
    }

    /// Register an async cleanup hook.
    ///
    /// Hooks run in reverse registration order when shutdown is triggered.
    /// `name` is used in logging and error reporting.
    ///
    /// Returns `Err(ShutdownError::AlreadyTriggered)` if shutdown has already
    /// been triggered. This prevents silent loss of cleanup hooks during
    /// startup/shutdown races.
    pub fn register_cleanup<F, Fut>(&self, name: &str, hook: F) -> Result<(), ShutdownError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.is_triggered() {
            return Err(ShutdownError::AlreadyTriggered);
        }
        let boxed: BoxedCleanupHook = Box::new(move || Box::pin(hook()));
        let mut hooks = self.inner.hooks.lock().unwrap_or_else(|e| e.into_inner());
        hooks.push((name.to_owned(), boxed));
        Ok(())
    }

    /// Wait for shutdown to complete (signal + all cleanup hooks).
    ///
    /// Returns when cleanup finishes or the grace period expires.
    /// All callers receive the same result — the outcome is stored and shared.
    ///
    /// # Cancellation Safety
    ///
    /// This method is **not cancellation-safe**. Do not use inside
    /// `tokio::select!`. Use the grace period for bounded completion.
    pub async fn wait(&self) -> Result<(), ShutdownError> {
        let inner = self.inner.clone();
        self.inner
            .result
            .get_or_init(|| async move {
                // Outer catch_unwind: ensure OnceCell is always populated
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    Self::run_shutdown(inner)
                }));
                match result {
                    Ok(fut) => fut.await,
                    Err(_) => {
                        #[cfg(feature = "logging")]
                        tracing::error!("unexpected panic in shutdown orchestration");
                        Ok(())
                    }
                }
            })
            .await
            .clone()
    }

    async fn run_shutdown(inner: Arc<ShutdownInner>) -> Result<(), ShutdownError> {
        // Take signal state — only the first wait() caller does this
        let signal_state = inner
            .signal_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();

        match signal_state {
            Some(mut state) => {
                // Wait for signal or manual trigger
                tokio::select! {
                    result = wait_for_signal(&mut state) => {
                        result?;
                        inner.token.cancel();
                    }
                    _ = inner.token.cancelled() => {
                        // Manual trigger — token already cancelled
                    }
                }
            }
            None => {
                // Should not happen given non-cancellable contract, but
                // handle gracefully (Design Constraint 1: no panicking)
                #[cfg(feature = "logging")]
                tracing::warn!("signal state already consumed — proceeding with cleanup");
            }
        }

        // Drain hooks under the lock, release immediately
        let hooks: Vec<(String, BoxedCleanupHook)> = {
            let mut guard = inner.hooks.lock().unwrap_or_else(|e| e.into_inner());
            let mut drained: Vec<_> = guard.drain(..).collect();
            drained.reverse();
            drained
        };

        if hooks.is_empty() {
            return Ok(());
        }

        let hook_names: Vec<String> = hooks.iter().map(|(name, _)| name.clone()).collect();
        let mut completed = Vec::with_capacity(hooks.len());

        let cleanup = async {
            for (name, hook) in hooks {
                // catch_unwind on the closure call (synchronous panic in hook creation)
                let fut = std::panic::catch_unwind(AssertUnwindSafe(hook));
                match fut {
                    Ok(fut) => {
                        // Spawn as a task so panics during .await are caught
                        // via JoinHandle rather than propagating
                        let handle = tokio::task::spawn(fut);
                        if let Err(e) = handle.await {
                            if e.is_panic() {
                                #[cfg(feature = "logging")]
                                tracing::error!("cleanup hook '{name}' panicked");
                                #[cfg(not(feature = "logging"))]
                                eprintln!("error: cleanup hook '{name}' panicked");
                            }
                        }
                    }
                    Err(_) => {
                        #[cfg(feature = "logging")]
                        tracing::error!("cleanup hook '{name}' panicked during creation");
                        #[cfg(not(feature = "logging"))]
                        eprintln!("error: cleanup hook '{name}' panicked during creation");
                    }
                }
                completed.push(name);
            }
        };

        let timeout_result =
            tokio::time::timeout(inner.grace_period, cleanup).await;

        if timeout_result.is_err() {
            let remaining: Vec<String> = hook_names
                .into_iter()
                .filter(|name| !completed.contains(name))
                .collect();
            return Err(ShutdownError::GracePeriodExceeded {
                elapsed: inner.grace_period,
                completed,
                remaining,
            });
        }

        Ok(())
    }
}

/// Initialize the shutdown subsystem. Installs signal handlers eagerly.
///
/// Called from `AppContextBuilder::build()`.
pub(crate) fn init_shutdown(builder: ShutdownBuilder) -> Result<Shutdown, ShutdownError> {
    let signal_state = install_signal_handlers()?;

    Ok(Shutdown {
        inner: Arc::new(ShutdownInner {
            token: CancellationToken::new(),
            hooks: Mutex::new(Vec::new()),
            grace_period: builder.grace_period,
            result: tokio::sync::OnceCell::new(),
            signal_state: Mutex::new(Some(signal_state)),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_builder() -> ShutdownBuilder {
        ShutdownBuilder::new().grace_period(Duration::from_secs(5))
    }

    #[tokio::test]
    async fn init_succeeds() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        assert!(!shutdown.is_triggered());
    }

    #[tokio::test]
    async fn trigger_is_idempotent() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        shutdown.trigger();
        assert!(shutdown.is_triggered());
        shutdown.trigger(); // no-op
        assert!(shutdown.is_triggered());
    }

    #[tokio::test]
    async fn token_observes_cancellation() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        let token = shutdown.token();
        assert!(!token.is_cancelled());
        shutdown.trigger();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn manual_trigger_completes_wait() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        shutdown.trigger();
        let result = shutdown.wait().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cleanup_hooks_run_in_reverse_order() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        let order = Arc::new(Mutex::new(Vec::new()));

        let o1 = order.clone();
        shutdown
            .register_cleanup("first", move || async move {
                o1.lock().unwrap().push(1);
            })
            .unwrap();

        let o2 = order.clone();
        shutdown
            .register_cleanup("second", move || async move {
                o2.lock().unwrap().push(2);
            })
            .unwrap();

        let o3 = order.clone();
        shutdown
            .register_cleanup("third", move || async move {
                o3.lock().unwrap().push(3);
            })
            .unwrap();

        shutdown.trigger();
        shutdown.wait().await.unwrap();

        let executed = order.lock().unwrap();
        assert_eq!(*executed, vec![3, 2, 1]);
    }

    #[tokio::test]
    async fn grace_period_exceeded() {
        let shutdown =
            init_shutdown(ShutdownBuilder::new().grace_period(Duration::from_millis(50))).unwrap();

        shutdown
            .register_cleanup("fast", || async {})
            .unwrap();
        shutdown
            .register_cleanup("slow", || async {
                tokio::time::sleep(Duration::from_secs(10)).await;
            })
            .unwrap();

        shutdown.trigger();
        let result = shutdown.wait().await;
        let err = result.unwrap_err();

        match err {
            ShutdownError::GracePeriodExceeded {
                completed,
                remaining,
                ..
            } => {
                // "slow" runs first (reverse order), "fast" is remaining
                assert!(remaining.contains(&"fast".to_string()));
                assert!(completed.is_empty() || completed.contains(&"slow".to_string()));
            }
            other => panic!("expected GracePeriodExceeded, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn wait_result_shared() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        shutdown.trigger();

        let r1 = shutdown.wait().await;
        let r2 = shutdown.wait().await;
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }

    #[tokio::test]
    async fn late_registration_rejected() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        shutdown.trigger();

        let result = shutdown.register_cleanup("too-late", || async {});
        assert!(matches!(result, Err(ShutdownError::AlreadyTriggered)));
    }

    #[tokio::test]
    async fn hook_panic_does_not_poison() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        let ran = Arc::new(AtomicUsize::new(0));

        let r = ran.clone();
        shutdown
            .register_cleanup("good", move || async move {
                r.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();

        shutdown
            .register_cleanup("panicker", || async {
                panic!("boom");
            })
            .unwrap();

        shutdown.trigger();
        let result = shutdown.wait().await;
        assert!(result.is_ok());
        // "panicker" runs first (reverse order), "good" still runs
        assert_eq!(ran.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn token_cross_task() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        let token = shutdown.token();

        let handle = tokio::spawn(async move {
            token.cancelled().await;
            true
        });

        shutdown.trigger();
        let completed = handle.await.unwrap();
        assert!(completed);
    }

    #[tokio::test]
    async fn debug_output() {
        let shutdown = init_shutdown(test_builder()).unwrap();
        let debug = format!("{:?}", shutdown);
        assert!(debug.contains("Shutdown"));
        assert!(debug.contains("is_triggered"));
        assert!(debug.contains("grace_period"));
    }
}
