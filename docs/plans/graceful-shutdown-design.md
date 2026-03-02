# Graceful Shutdown Subsystem Design

**Feature:** `shutdown`
**Dependencies:** `tokio` (signal, rt, time), `tokio-util` (CancellationToken)
**Depends on:** None (pure runtime infrastructure)
**Required by:** HTTP subsystem (planned)

---

## Problem

The old dragon-fnd implementation coupled shutdown handling tightly to the HTTP server — signal listeners lived in `http.rs` and the grace period was part of `HttpConfig`. This made shutdown logic unavailable to non-HTTP applications and forced every HTTP consumer to accept the library's signal handling whether they wanted it or not.

Shutdown is a cross-cutting concern. A CLI tool that opens a database pool needs cleanup on SIGTERM. A background worker processing a queue needs to finish its current item before exiting. A web server needs to drain in-flight requests. All of these need the same infrastructure: signal detection, cancellation propagation, and orchestrated cleanup.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Signaling mechanism | `tokio_util::CancellationToken` | `Send + Sync + Clone`, late-subscriber safe, child token support. Purpose-built for cancellation. |
| Cleanup hooks | Async callbacks, reverse registration order, sequential execution | Ordering guarantee is the point. Concurrency can be layered inside a single hook. |
| Configuration | Builder-only (no serde config, no TOML section) | VISION.md: "no config dependency." Grace period is set programmatically via the builder. |
| Trigger sources | OS signals (SIGTERM/SIGINT) + manual `trigger()` | Manual trigger enables shutdown from fatal errors, admin endpoints, or test code. |
| Architecture | Monolithic `Shutdown` handle | Token + signal listener + cleanup registry + grace period in one struct. Users who want just a token can ignore cleanup. |
| Execution model | Background tokio task | Spawned at init. Awaits token cancellation, runs hooks, enforced grace period. `wait()` joins the task. |
| Grace period scope | Total budget for all cleanup hooks combined | Not per-hook, not drain time. Drain time is the consuming subsystem's concern. |

## Public API

### ShutdownBuilder

Fluent builder, registered on AppContextBuilder. Transitions the builder to `AsyncBuild` (signal handling requires tokio).

```rust
use std::time::Duration;

let builder = ShutdownBuilder::new()           // 30s default grace period
    .grace_period(Duration::from_secs(60));    // override
```

No `from_config()` — this subsystem has no serde-deserializable config struct. The builder is the only configuration surface.

### Shutdown (Runtime Handle)

Held by `AppContext`, accessed via `ctx.shutdown() -> Option<&Shutdown>`.

```rust
impl Shutdown {
    /// Clone of the cancellation token. Send + Sync + Clone.
    /// Pass to worker threads, tasks, or other subsystems.
    pub fn token(&self) -> CancellationToken;

    /// Programmatically trigger shutdown.
    /// Idempotent — calling on an already-cancelled token is a no-op.
    pub fn trigger(&self);

    /// Check if shutdown has been triggered (signal or manual).
    pub fn is_triggered(&self) -> bool;

    /// Register an async cleanup hook.
    /// Hooks run in reverse registration order when shutdown is triggered.
    /// `name` is used in logging and error reporting.
    ///
    /// If called after shutdown is already triggered and hooks are executing,
    /// the hook will not run (the background task snapshots the hook list).
    pub fn register_cleanup<F, Fut>(&self, name: &str, hook: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static;

    /// Wait for shutdown to complete (signal + all cleanup hooks).
    /// Returns when cleanup finishes or the grace period expires.
    ///
    /// Idempotent — first caller gets the result, subsequent callers get Ok(()).
    /// Blocks until a signal fires or trigger() is called, then runs cleanup.
    pub async fn wait(&self) -> Result<(), ShutdownError>;
}
```

### ShutdownError

```rust
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ShutdownError {
    #[error("failed to install signal handler: {0}")]
    SignalHandler(String),

    #[error("cleanup grace period exceeded ({} completed, {} remaining)",
        completed.len(), remaining.len())]
    GracePeriodExceeded {
        elapsed: Duration,
        completed: Vec<String>,
        remaining: Vec<String>,
    },
}
```

## Internal Architecture

### Module Structure

```
src/shutdown/
├── mod.rs          # Re-exports, pub(crate) init_shutdown
├── builder.rs      # ShutdownBuilder (fluent API)
├── handle.rs       # Shutdown struct, cleanup orchestration, init_shutdown
├── signal.rs       # OS signal listener (platform-aware)
└── error.rs        # ShutdownError enum
```

### Background Task Flow

```
Signal (SIGTERM/SIGINT) ──┐
                          ├──> CancellationToken cancelled
Manual trigger() ─────────┘
                                    │
                          ┌─────────▼──────────┐
                          │  Background task    │
                          │  awakens            │
                          ├─────────────────────┤
                          │  Snapshot hook list  │
                          │  Reverse order       │
                          │  Run sequentially    │
                          │  Total grace period  │
                          │  budget enforced     │
                          └─────────┬───────────┘
                                    │
                          ┌─────────▼──────────┐
                          │  wait() returns     │
                          │  Ok(()) or          │
                          │  GracePeriodExceeded │
                          └─────────────────────┘
```

1. Background task spawned during `init_shutdown()`.
2. Task `select!`s on `wait_for_signal()` and `token.cancelled()` — first one wins.
3. If signal triggered: cancel the token (so all token holders learn about shutdown).
4. Lock hooks mutex, drain in reverse order (snapshot — late registrations won't run).
5. `tokio::time::timeout(grace_period, run_all_hooks_sequentially)`.
6. Return `Ok(())` or `GracePeriodExceeded { completed, remaining }`.

### Internal Types

```rust
type BoxedCleanupHook = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

struct ShutdownInner {
    token: CancellationToken,
    hooks: Mutex<Vec<(String, BoxedCleanupHook)>>,
    grace_period: Duration,
    task: Mutex<Option<JoinHandle<Result<(), ShutdownError>>>>,
}
```

The background task holds clones of the `CancellationToken` and `Arc<Mutex<Vec<...>>>` hooks — **not** an `Arc<ShutdownInner>`. This avoids a reference cycle between `ShutdownInner` (which holds the `JoinHandle`) and the spawned task.

### Drop Behavior

`Shutdown::drop()` aborts the background task (via `JoinHandle::abort()`) to prevent a leaked task. It does **not** cancel the token — dropping the context is not a shutdown trigger. Users who want orchestrated cleanup call `wait().await` before dropping.

### Signal Handling

```rust
// signal.rs — platform-aware
pub(crate) async fn wait_for_signal() -> Result<(), ShutdownError> {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).map_err(|e| ShutdownError::SignalHandler(e.to_string()))?;

        tokio::select! {
            result = ctrl_c => {
                result.map_err(|e| ShutdownError::SignalHandler(e.to_string()))?;
            }
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        ctrl_c.await.map_err(|e| ShutdownError::SignalHandler(e.to_string()))?;
    }

    Ok(())
}
```

Returns `Result` — no `.expect()` panics (design constraint #1).

## AppContext Integration

### Builder Changes

`with_shutdown()` transitions the builder to `AsyncBuild` (same as `with_sqlite()`). Both the `SyncBuild -> AsyncBuild` and `AsyncBuild -> AsyncBuild` overloads are provided.

The `AsyncBuild` struct, `into_async()` helper, and `async fn build()` all widen their cfg gate from `#[cfg(feature = "sqlite")]` to `#[cfg(any(feature = "sqlite", feature = "shutdown"))]`.

### Drop Order in AppContext

Fields are ordered for correct drop sequence:

1. `config` — pure data, no cleanup
2. `extensions` — user-provided, drop before subsystems
3. `shutdown` — abort background task
4. `sqlite_pool` — close database connections
5. `log_guard` — MUST be last, flushes pending log writes

Shutdown drops before sqlite: the token fires (if not already cancelled), then the pool closes (logging during pool drop is captured by the still-alive log guard).

### Usage Shape

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use dragon_fnd::shutdown::ShutdownBuilder;
use std::time::Duration;

let config: MyConfig = ConfigBuilder::new()
    .with_file("config.toml", true)
    .build()?;

let ctx = AppContext::builder()
    .with_config(config)
    .with_shutdown(ShutdownBuilder::new().grace_period(Duration::from_secs(30)))
    .build()    // async required — shutdown needs tokio
    .await?;

// Pass token to workers
let token = ctx.shutdown().unwrap().token();
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = do_work() => {}
        }
    }
});

// Register cleanup
ctx.shutdown().unwrap().register_cleanup("db", || async {
    pool.close().await;
});

// Block until shutdown completes
ctx.shutdown().unwrap().wait().await?;
```

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| `trigger()` called twice | Idempotent — CancellationToken ignores redundant cancels |
| `wait()` called twice | First caller gets the result; subsequent callers get `Ok(())` |
| `wait()` called before any signal | Blocks until signal fires or `trigger()` is called |
| Hook registered after shutdown triggered | Silently won't run (background task already snapshotted the list) |
| Signal handler fails to install | `init_shutdown()` returns `Err(ShutdownError::SignalHandler(...))` |
| All hooks complete before grace period | `wait()` returns `Ok(())` immediately |
| Grace period expires mid-hook | Current hook is interrupted; `GracePeriodExceeded` returned with completed/remaining lists |
| AppContext dropped without `wait()` | Background task aborted (best-effort cleanup only) |

## Test Strategy

**Unit tests:**
- `ShutdownBuilder` defaults and fluent API
- `ShutdownError` Display messages and Debug output

**Integration tests:**
- Manual trigger: `trigger()` cancels token, `wait()` returns `Ok`
- Cleanup reverse order: register 3 hooks, trigger, verify execution order via shared state
- Grace period exceeded: slow hook + short grace period -> `GracePeriodExceeded` error
- `wait()` idempotency: two sequential `wait()` calls both succeed
- AppContext integration: build with shutdown, accessor returns `Some`
- AppContext without shutdown: accessor returns `None`
- Token cross-task: spawn a task with cloned token, trigger, verify task observes cancellation
- Compile-fail doctest: `with_shutdown()` + `build_sync()` does not compile

## Dependencies Added

```toml
# Cargo.toml
[features]
shutdown = ["dep:tokio", "dep:tokio-util"]

[dependencies]
tokio = { version = "1", features = ["signal", "rt", "time"], optional = true }
tokio-util = { version = "0.7", optional = true }
```

Both are optional and only pulled in when the `shutdown` feature is enabled.
