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
| Cleanup hooks | Async callbacks, reverse registration order, sequential execution | Ordering guarantee is the point. Concurrency can be layered inside a single hook. One slow hook can starve subsequent hooks — users who need per-hook timeouts should wrap their hook body in `tokio::time::timeout`. Hooks are for **user-owned resources** — see "Relationship to Graph-Based Teardown" below. |
| Configuration | Builder-only (no serde config, no TOML section) | VISION.md: "no config dependency." Grace period is set programmatically via the builder. |
| Trigger sources | OS signals (SIGTERM/SIGINT) + manual `trigger()` | Manual trigger enables shutdown from fatal errors, admin endpoints, or test code. |
| Architecture | Monolithic `Shutdown` handle | Token + signal state + cleanup registry + grace period in one struct. No trait boundary — see "Trait Boundary" section below. |
| Execution model | Inline in `wait()` | No background task. `wait()` blocks on signal/token, then runs hooks sequentially with grace period enforcement. Simpler than a spawned task — no JoinHandle lifecycle, no abort-on-drop, no Arc splitting. |
| Grace period scope | Total budget for all cleanup hooks combined | Not per-hook, not drain time. Drain time is the consuming subsystem's concern. |

### Trait Boundary

VISION.md states every subsystem follows the three-layer pattern (trait → default impl → custom impl). The shutdown subsystem defers a trait boundary to a future iteration, for the same pragmatic reason SQLite deferred one: there is currently no second implementation to design the trait against. Unlike logging (where `tracing` IS the ecosystem interface), shutdown trigger sources are diverse (OS signals, message queues, file watches, Windows service control) — but until a concrete second source materializes, designing the trait risks getting the abstraction wrong. When a second trigger source is needed, the concrete `Shutdown` API will inform the trait extraction. This is an acknowledged exception, not an oversight.

### Relationship to Graph-Based Teardown

VISION.md describes shutdown teardown as "the same graph, walked backwards" — subsystems declare dependencies, and teardown order is derived automatically. This plan uses reverse-registration-order hooks instead, which is a different model. This is not a contradiction — the two models serve different scopes:

- **Library-managed subsystems** (database pools, HTTP servers): teardown will be automatic and graph-ordered inside `build()`/`wait()` when subsystem dependency resolution lands. The HTTP subsystem (which depends on both config and shutdown) is the natural trigger for implementing this. Users should NOT register cleanup hooks for library-managed resources — the library will handle them.
- **User-owned resources** (custom connections, caches, background workers): cleanup hooks via `register_cleanup()` are the escape hatch. These run in reverse registration order within the grace period budget.

When graph-based subsystem teardown arrives, hooks remain as the user-facing API for custom cleanup. The library internally walks the dependency graph for its own subsystems, then runs user hooks. This is additive — no breaking change to the hook API.

## Public API

### ShutdownBuilder

Fluent builder, registered on AppContextBuilder. Transitions the builder to `AsyncBuild` (signal handling requires tokio).

```rust
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct ShutdownBuilder { /* ... */ }

use std::time::Duration;

let builder = ShutdownBuilder::new()           // 30s default grace period
    .grace_period(Duration::from_secs(60));    // override
```

No `from_config()` or `into_config()` for now — this subsystem has no serde-deserializable config struct. The builder is consumed directly by `init_shutdown()`, unlike `LoggingBuilder` and `SqliteBuilder` which extract a config struct via `pub(crate) into_config()`. This is a **deferral**, not a permanent departure: grace period is the kind of setting users will eventually want in config files (`[shutdown] grace_period_secs = 30`). When that need arises, add `ShutdownConfig`, `from_config()`, and `into_config()` following the existing pattern. For now, builder-only is simpler and matches VISION.md's "no config dependency" statement.

### Shutdown (Runtime Handle)

Held by `AppContext`, accessed via `ctx.shutdown() -> Option<&Shutdown>`.

Note: this follows the existing `Option`-based accessor pattern (`ctx.sqlite()` returns `Option<&SqlitePool>`). VISION.md aspires to type-level subsystem availability where the accessor would not exist if the subsystem was not registered. That work is deferred — see VISION.md "Subsystem availability" section.

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
    /// Returns `Err(ShutdownError::AlreadyTriggered)` if shutdown has already
    /// been triggered. This prevents silent loss of cleanup hooks during
    /// startup/shutdown races (Design Constraint 2: no silent failures).
    pub fn register_cleanup<F, Fut>(&self, name: &str, hook: F) -> Result<(), ShutdownError>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static;

    /// Wait for shutdown to complete (signal + all cleanup hooks).
    /// Returns when cleanup finishes or the grace period expires.
    ///
    /// All callers receive the same result — the outcome is stored and
    /// shared via Clone on ShutdownError.
    ///
    /// # Cancellation Safety
    ///
    /// This method is **not cancellation-safe**. Do not use inside
    /// `tokio::select!`. Use the grace period for bounded completion.
    pub async fn wait(&self) -> Result<(), ShutdownError>;
}
```

### ShutdownError

```rust
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum ShutdownError {
    #[error("failed to install signal handler")]
    SignalHandler {
        #[source]
        source: Arc<io::Error>,
    },

    #[error("cleanup grace period exceeded ({} completed, {} remaining)",
        completed.len(), remaining.len())]
    GracePeriodExceeded {
        elapsed: Duration,
        completed: Vec<String>,
        remaining: Vec<String>,
    },

    #[error("shutdown already triggered, cleanup hook will not run; \
             call the hook directly if cleanup is still needed")]
    AlreadyTriggered,
}
```

Note: `ShutdownError` derives `Clone`, unlike `LoggingError` and `SqliteError`. This is required by the `OnceCell`-based result sharing in `wait()` — multiple callers need to receive the same result. `Arc<io::Error>` wraps the non-`Clone` `io::Error` to enable this, which is a standard ecosystem pattern (hyper, tonic). The `#[source]` attribute on the `Arc` preserves the error source chain. This asymmetry with other subsystem error types is driven by requirements, not stylistic preference — a comment in the error module should explain why.

## Internal Architecture

### Module Structure

```
src/shutdown/
├── mod.rs          # Re-exports: pub(crate) use init::init_shutdown
├── builder.rs      # ShutdownBuilder (fluent API, Debug, Clone, #[must_use])
├── init.rs         # Shutdown struct, init_shutdown(), cleanup orchestration
├── signal.rs       # OS signal listener (platform-aware)
└── error.rs        # ShutdownError enum
```

Follows the existing convention: `logging/init.rs` defines `init_logging`, `sqlite/init.rs` defines `init_pool`. The init function and runtime handle live together in `init.rs` because the handle is the init function's return value — the same relationship as `init_pool` returning `SqlitePool`.

### Init Function

```rust
/// Initialize the shutdown subsystem. Installs signal handlers eagerly.
/// Called from AppContextBuilder::build().
pub(crate) async fn init_shutdown(builder: ShutdownBuilder) -> Result<Shutdown, ShutdownError>
```

Follows the existing pattern: `init_logging(&LoggingConfig) -> Result<Option<WorkerGuard>, LoggingError>` and `init_pool(&SqliteConfig) -> Result<SqlitePool, SqliteError>`. Takes ownership of the builder (no `into_config()` intermediary since there is no config struct). The function is `async` because it is called from `async fn build()` and requires an active tokio runtime (`tokio::signal::unix::signal()` calls `Handle::current()` internally). The signal installation itself is synchronous, but the runtime-existence requirement means it must execute within an async context.

### Inline `wait()` Flow

No background task. `wait()` does all work inline:

```
Signal (SIGTERM/SIGINT) ──┐
                          ├──> CancellationToken cancelled
Manual trigger() ─────────┘
                                    │
                          ┌─────────▼──────────┐
                          │  wait() awakens     │
                          │  (was blocked on    │
                          │  select! of signal  │
                          │  + token.cancelled) │
                          ├─────────────────────┤
                          │  Lock hooks mutex   │
                          │  Drain (snapshot)   │
                          │  Release lock       │
                          │  Reverse order      │
                          │  Run sequentially   │
                          │  Grace period via   │
                          │  tokio::time::      │
                          │  timeout wrapping   │
                          │  entire sequence    │
                          └─────────┬───────────┘
                                    │
                          ┌─────────▼──────────┐
                          │  Store result       │
                          │  Return to all      │
                          │  callers            │
                          └─────────────────────┘
```

1. `wait()` calls `OnceCell::get_or_init` — all work happens inside the init closure. Subsequent callers await the same cell and clone the stored result.
2. Inside the init closure: lock `signal_state` mutex, `.take()` the `Option<SignalState>`. If `None` (should not happen given non-cancellable contract), return an error result rather than panicking (Design Constraint 1). Release lock.
3. `select!` on `wait_for_signal()` and `token.cancelled()` — first one wins.
4. If signal triggered: cancel the token (so all token holders learn about shutdown).
5. Lock hooks mutex, drain in reverse order, release lock immediately.
6. Run hooks inside a `tokio::select!` racing against `tokio::time::sleep(grace_period)`. Each hook is wrapped in `std::panic::AssertUnwindSafe` + `catch_unwind` — if a user hook panics, the panic is caught, logged via `tracing::error!("cleanup hook '{}' panicked", name)`, and execution continues to the next hook. The library must not propagate user panics (Design Constraint 1). The hook runner tracks completed hook names in a local `Vec<String>` as each hook finishes. If the timeout wins, the remaining (unrun) names are computed from the original list minus completed — this produces the `completed` and `remaining` fields for `GracePeriodExceeded`.
7. The entire init closure is wrapped in `catch_unwind` as a final safety net, ensuring the `OnceCell` is always populated regardless of unexpected panics. This prevents the unrecoverable state described in the Cancellation Safety section.
8. Return `Ok(())` or `GracePeriodExceeded`.

The lock-drain-release pattern is critical: the `std::sync::MutexGuard` must not be held across `.await` points. Drain the Vec under the lock, release, then await hooks. The compiler enforces this — `MutexGuard` is `!Send`, so holding it across `.await` in a `Send` future fails to compile.

### Internal Types

```rust
struct ShutdownInner {
    token: CancellationToken,
    hooks: std::sync::Mutex<Vec<(String, BoxedCleanupHook)>>,
    grace_period: Duration,
    result: tokio::sync::OnceCell<Result<(), ShutdownError>>,
    signal_state: std::sync::Mutex<Option<SignalState>>,
}

type BoxedCleanupHook = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;
```

`SignalState` holds the pre-installed signal handlers from `init_shutdown()` (see "Signal Handling" below). It is wrapped in `std::sync::Mutex<Option<SignalState>>` — `wait()` takes it out via `.take()` to get `&mut` access without requiring `&mut self` on the `Arc`-wrapped inner struct. `tokio::sync::OnceCell` stores the first `wait()` result and provides async-aware `get_or_init` — subsequent callers await the same cell and clone the stored result.

`Shutdown` wraps `Arc<ShutdownInner>` so the handle can be shared (accessed via `&self` on `AppContext`).

### Cancellation Safety

**`wait()` is not cancellation-safe and must not be used inside `tokio::select!`.** The `Mutex<Option<SignalState>>.take()` inside the `OnceCell::get_or_init` closure is a non-idempotent side effect. If `wait()` is cancelled after `.take()` consumes the signal state but before the `OnceCell` is populated, the system enters an unrecoverable state: signal state is gone, result cell is empty, subsequent `wait()` calls cannot re-acquire signal handlers.

`tokio::sync::OnceCell::get_or_init` IS cancellation-safe (a cancelled init lets a new caller retry), but the `.take()` side-effect within the init closure is NOT — once taken, the signal state cannot be restored.

This is an acceptable constraint: `wait()` is designed to be the terminal call in `main()` — "block until shutdown completes." External timeouts are unnecessary because the grace period already provides a bounded completion guarantee. Users who need "shut down, but force-exit if it takes too long" should set the grace period accordingly, not wrap `wait()` in `select!`.

The doc-comment on `wait()` must state this explicitly: "This method is not cancellation-safe. Do not use inside `tokio::select!`."

### Drop Behavior

`Shutdown` implements `Drop` with a best-effort diagnostic. If hooks are registered and `wait()` was never called (detected via `result.initialized()` being false and hooks vec being non-empty), `Drop` emits `tracing::warn!("shutdown handle dropped with {} registered cleanup hooks that were never executed; call wait().await to run cleanup", count)`.

This is not guaranteed to be visible — but given the AppContext drop order (`log_guard` is last), the logging subsystem will typically still be alive when `Shutdown` drops. The `Drop` impl uses `try_lock().ok()` on the hooks mutex to avoid panicking on a poisoned mutex during stack unwinding (Design Constraint 1: no panicking).

The API contract remains: call `wait().await` for cleanup. The `Drop` warning is a diagnostic aid, not a substitute. There is no background task to abort.

### Signal Handling

Signal handlers are installed eagerly in `init_shutdown()`, not deferred to `wait()`. This follows the fail-fast principle — if signal installation fails (permissions, platform limitations), the error surfaces immediately during `build()`, not hours later when the first signal arrives.

```rust
// signal.rs — platform-aware

/// Pre-install signal handlers. Called from init_shutdown().
/// Returns a SignalState that wait_for_signal() later consumes.
pub(crate) fn install_signal_handlers() -> Result<SignalState, ShutdownError> {
    #[cfg(unix)]
    {
        let sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate()
        ).map_err(|e| ShutdownError::SignalHandler {
            source: Arc::new(e),
        })?;
        Ok(SignalState { sigterm })
    }

    #[cfg(not(unix))]
    {
        // ctrl_c() installs its handler lazily on first .await,
        // but we validate here that the platform supports it.
        Ok(SignalState {})
    }
}

/// Block until a signal fires. Uses the pre-installed handlers via &mut.
/// Ownership transfer happens at the .take() call site in wait(), not here.
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
        tokio::signal::ctrl_c().await.map_err(|e| ShutdownError::SignalHandler {
            source: Arc::new(e),
        })?;
    }

    Ok(())
}
```

Returns `Result` — no `.expect()` panics (Design Constraint 1).

### Mutex Usage

All internal mutexes are `std::sync::Mutex`, not `tokio::sync::Mutex`. Rationale:

- `register_cleanup()` is synchronous — callers should not need to `.await` to register a hook.
- The hooks mutex is held only briefly (lock, drain/push, release). Never held across `.await` points.
- The `Drop` impl uses `.lock().ok()` to avoid panicking on a poisoned mutex during stack unwinding. This is a library-wide constraint (Design Constraint 1: no panicking).

## AppContext Integration

### Builder Changes

`with_shutdown()` transitions the builder to `AsyncBuild` (same as `with_sqlite()`). Both the `SyncBuild -> AsyncBuild` and `AsyncBuild -> AsyncBuild` overloads are provided.

The `AsyncBuild` marker, `into_async()` helper, and `async fn build()` use the `_async` synthetic feature flag:

```toml
# Cargo.toml
[features]
_async = []  # internal — activated by any async subsystem
shutdown = ["dep:tokio", "dep:tokio-util", "_async"]
sqlite = ["dep:sqlx", "_async"]
```

All async-build gates become `#[cfg(feature = "_async")]` — a single stable predicate instead of `#[cfg(any(feature = "sqlite", feature = "shutdown", ...))]` expanding with each subsystem. This is a standard Cargo pattern (tokio uses internal features the same way). Downstream crates must not activate `_async` directly — the leading underscore signals "internal."

**Migration:** This is a cross-cutting refactor that must be done atomically. The following `#[cfg]` gates in `context/mod.rs` must be audited and split:

**Change to `#[cfg(feature = "_async")]`** (shared async scaffolding):
- `AsyncBuild` marker type definition
- `into_async()` helper function and its impl block
- `async fn build()` method and its impl block
- `PhantomData<Async>` and related type parameter usage

**Keep as subsystem-specific** (do not change to `_async`):
- `with_sqlite()` on `SyncBuild` — stays `#[cfg(feature = "sqlite")]`
- `with_sqlite()` on `AsyncBuild` — stays `#[cfg(feature = "sqlite")]`
- sqlite init code inside `build()` — stays `#[cfg(feature = "sqlite")]`
- `sqlite_pool` field on `AppContext` — stays `#[cfg(feature = "sqlite")]`

**Add new** (shutdown-specific):
- `with_shutdown()` on `SyncBuild` — `#[cfg(feature = "shutdown")]`
- `with_shutdown()` on `AsyncBuild` — `#[cfg(feature = "shutdown")]`
- shutdown init code inside `build()` — `#[cfg(feature = "shutdown")]`
- `shutdown` field on `AppContext` — `#[cfg(feature = "shutdown")]`

`into_async()` must propagate ALL feature-gated fields: `#[cfg(feature = "sqlite")] sqlite: self.sqlite` and `#[cfg(feature = "shutdown")] shutdown: self.shutdown`. Each field is individually cfg-gated within the shared `_async`-gated function.

All changes must land in the same commit that adds `_async` to the `sqlite` feature definition. Existing users with `features = ["sqlite"]` see no behavioral change — `_async` is purely additive and gates the same code that was previously gated behind `sqlite`.

Subsystem-specific code within `build()` remains individually gated: `#[cfg(feature = "sqlite")]` for sqlite init, `#[cfg(feature = "shutdown")]` for shutdown init. Only the shared async scaffolding uses `_async`.

### Top-Level Error

Add a cfg-gated variant to the top-level `Error` enum:

```rust
#[cfg(feature = "shutdown")]
#[error("shutdown error: {0}")]
Shutdown(#[from] ShutdownError),
```

This matches the existing convention (`"logging error: {0}"`, `"sqlite error: {0}"`) and enables `?` propagation from `init_shutdown()` through `build()`.

### lib.rs Changes

Add module declaration and re-exports:

```rust
#[cfg(feature = "shutdown")]
pub mod shutdown;
```

### Init Order

Shutdown initializes after logging (so shutdown can log during init) and before sqlite (so the token is available if sqlite init needs it in the future). This continues the hardcoded init approach.

VISION.md envisions graph-based topological sort for subsystem init ordering. This is deferred — shutdown has no subsystem dependencies ("pure runtime infrastructure"), so graph-based resolution adds no value yet. The HTTP subsystem (which depends on both config and shutdown) will be the natural trigger for implementing dependency-declared init ordering. The existing TODO in `context/mod.rs` should be updated to reference HTTP as the trigger point.

### Drop Order in AppContext

Fields are ordered for correct drop sequence:

1. `config` — pure data, no cleanup
2. `extensions` — user-provided, drop before subsystems
3. `shutdown` — Drop emits diagnostic warning, then frees the `Arc<ShutdownInner>`
4. `sqlite_pool` — close database connections
5. `log_guard` — MUST be last, flushes pending log writes

Shutdown drops before sqlite for conceptual ordering — shutdown orchestration should be released before the resources it orchestrates. In practice, the user should call `wait().await` before dropping the context for orderly cleanup. The drop order is a safety net for correct resource release ordering, not a substitute for explicit shutdown.

### AppContextBuilder Debug

The manual `Debug` impl on `AppContextBuilder` must be extended to show `shutdown: Some/None`, matching the existing pattern for `logging` and `sqlite`.

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
let shutdown = ctx.shutdown().expect("shutdown was registered");
let token = shutdown.token();
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = do_work() => {}
        }
    }
});

// Register cleanup for USER-OWNED resources.
// Library-managed subsystems (sqlite, logging) will be torn down
// automatically by the library when graph-based teardown lands.
let cache = my_cache.clone();
ctx.shutdown().expect("shutdown was registered")
    .register_cleanup("cache-flush", move || async move {
        cache.flush().await;
    })?;

// Block until shutdown completes
ctx.shutdown().expect("shutdown was registered").wait().await?;
```

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| `trigger()` called twice | Idempotent — CancellationToken ignores redundant cancels |
| `wait()` called twice | Both callers receive the same result (stored in `OnceCell`, cloned) |
| `wait()` called before any signal | Blocks until signal fires or `trigger()` is called |
| Hook registered after shutdown triggered | Returns `Err(ShutdownError::AlreadyTriggered)` — no silent loss |
| Signal handler fails to install | `init_shutdown()` returns `Err(ShutdownError::SignalHandler { ... })` at build time |
| All hooks complete before grace period | `wait()` returns `Ok(())` immediately |
| Grace period expires mid-hook | Current hook is interrupted; `GracePeriodExceeded` returned with completed/remaining lists |
| Cleanup hook panics | Panic caught via `catch_unwind`, logged via `tracing::error!`, next hook runs. `wait()` returns normally — panicked hooks do not poison the result. |
| AppContext dropped without `wait()` | Hooks do not run. `tracing::warn!` emitted if hooks were registered. Explicit contract: call `wait()` for cleanup. |

## Test Strategy

**Unit tests:**
- `ShutdownBuilder` defaults and fluent API
- `ShutdownBuilder` Debug and Clone
- `ShutdownError` Display messages, Debug output, and source chain
- `ShutdownError` Clone

**Integration tests:**
- Manual trigger: `trigger()` cancels token, `wait()` returns `Ok`
- Cleanup reverse order: register 3 hooks, trigger, verify execution order via shared state
- Grace period exceeded: slow hook + short grace period -> `GracePeriodExceeded` error
- `wait()` result sharing: two callers both receive the same result
- Late registration: register after trigger -> `Err(AlreadyTriggered)`
- AppContext integration: build with shutdown, accessor returns `Some`
- AppContext without shutdown: accessor returns `None`
- Token cross-task: spawn a task with cloned token, trigger, verify task observes cancellation
- Signal handler install: verify `init_shutdown()` succeeds in tokio runtime
- Hook panic safety: register a panicking hook, trigger, verify `wait()` returns `Ok` and subsequent hooks still run
- Drop warning: register hooks, drop without `wait()`, verify `tracing::warn!` emitted
- Compile-fail doctest: `with_shutdown()` + `build_sync()` does not compile

## Dependencies Added

```toml
# Cargo.toml
[features]
_async = []  # internal — activated by any async subsystem
shutdown = ["dep:tokio", "dep:tokio-util", "_async"]
sqlite = ["dep:sqlx", "_async"]  # updated: adds _async activation

[dependencies]
tokio = { version = "1", features = ["signal", "rt", "time"], optional = true }
tokio-util = { version = "0.7", features = ["rt"], optional = true }
```

Both are optional and only pulled in when the `shutdown` feature is enabled. The `_async` internal feature is activated by any async subsystem and gates the shared `AsyncBuild` scaffolding in `context/mod.rs`.

**Architectural note:** This makes `tokio` a direct production dependency of the library (currently it is dev-only; `sqlx` brings it in transitively via `runtime-tokio`). DESIGN.md's statement that "Async runtime (`tokio`) is a dev dependency only" must be updated when this subsystem lands. The library is now tokio-aligned for async subsystems — `tokio::signal` and `tokio::time` have no runtime-agnostic equivalents. This is a deliberate, justified shift.

Note: `sqlite` already pulls in `tokio` transitively via `sqlx`'s `runtime-tokio`. Cargo unifies features across the dependency graph, so enabling both `shutdown` and `sqlite` results in a single `tokio` with the union of all requested features. No conflict.

## VISION.md Updates

When this subsystem is implemented, VISION.md should be updated:
- Rename `ShutdownSignal` to `Shutdown` in the graceful shutdown section (line 194)
- Update the feature table status from "Planned" to "Built"
- Add a note under the three-layer pattern section acknowledging the deferred trait boundary
