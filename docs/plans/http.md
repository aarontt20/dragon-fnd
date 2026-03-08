# HTTP Subsystem Design Plan

## Problem Statement

dragon-fnd needs an HTTP subsystem that manages axum server lifecycle — binding, serving, and graceful shutdown — so downstream applications don't reimplement the same boilerplate. The library owns the lifecycle; the user owns everything between request and response (routing, middleware, handlers).

## Design Decisions

### 1. Compile-time dependency on shutdown

The `http` feature activates the `shutdown` feature in `Cargo.toml`. At the feature level, enabling HTTP guarantees shutdown code exists. At the builder level, `build()` returns `HttpError::ShutdownRequired` if `with_http()` was called without `with_shutdown()`. This is a runtime check, not a type-state dimension — adding a type-state axis for every inter-subsystem dependency would be explosive and inconsistent with how the builder handles other configuration (type-state enforces the async/sync boundary; the builder validates inter-subsystem requirements). The feature gate ensures the API exists; the builder ensures it's configured.

### 2. Bind at build time

The TCP listener is bound during `build().await`, consistent with how SQLite creates the pool and shutdown installs signal handlers at build time. Port-in-use is a build error, not a serve error. The bound address is available immediately via `ctx.http()`.

### 3. Token-based graceful shutdown

`serve()` calls `axum::serve(listener, router).with_graceful_shutdown(token.cancelled_owned())`. Uses `cancelled_owned()` rather than `cancelled()` because axum's `with_graceful_shutdown` requires `F: Future<Output = ()> + Send + 'static` — `cancelled()` borrows the token and cannot satisfy the `'static` bound. The `CancellationToken` comes from the shutdown subsystem. When shutdown triggers (signal or programmatic), axum stops accepting new connections and drains in-flight requests. No cleanup hooks needed — axum's built-in graceful shutdown does the work.

### 4. Interior mutability for the listener

The `Http` handle stores the listener in a `Mutex<Option<TcpListener>>`. `serve()` takes `&self` and extracts the listener internally via `Mutex::lock()` + `Option::take()`. This allows `AppContext` to be behind `Arc` (standard axum state pattern) without requiring `&mut self`. Calling `serve()` twice returns `HttpError::AlreadyServing`. `local_addr()` is stored as a plain `SocketAddr` at bind time — it returns the bound address regardless of whether `serve()` has been called or has returned.

### 5. Minimal config

`HttpConfig` has two fields: `host` (default `"0.0.0.0"`) and `port` (default `8080`). No listener tuning knobs (TCP keepalive, backlog, etc.) in this iteration. Future enhancement opportunity documented.

### 6. Router type

`serve()` accepts `axum::Router` (i.e., `Router<()>` — state already applied). This is the terminal router type in axum's type system. The user calls `.with_state()` before passing the router to `serve()`.

### 7. Architecture Decision: No Trait Boundary

The HTTP subsystem does not define its own trait. `axum` is the Rust ecosystem's dominant HTTP framework — analogous to `tracing` for logging. Adding a library-level trait on top of axum's `serve()` would add indirection with no real value. Users who want a different HTTP server (hyper directly, warp, etc.) skip the `http` feature and wire their own server using `ctx.shutdown().unwrap().token()` directly via `with_extension()`. This is the fourth explicit exception to CLAUDE.md Constraint 4 (after logging, SQLite, and shutdown), following the same justification pattern.

### 8. Hardcoded initialization order

Subsystem initialization order is hardcoded in `build()`, not resolved via a dependency graph. The order is small (4 subsystems), readable, and easy to verify. VISION.md describes graph-based dependency resolution as a pattern at the config-value scale (where it's built and working), but at the subsystem scale the graph adds complexity that isn't justified — the dependency edges are few, stable, and obvious. The existing TODO comment in `context/mod.rs` about topological sort should be removed; hardcoded order is the deliberate choice, not a temporary measure.

## Dependencies

| Crate | Version | What it's used for | New? |
|-------|---------|-------------------|------|
| `axum` | 0.8 | `axum::serve()`, `Router` type | **Yes** |
| `tokio` | 1 | `TcpListener::bind()`, async runtime | No (via `shutdown`) |
| `tokio-util` | 0.7 | `CancellationToken` | No (via `shutdown`) |

Axum features: `default-features = false, features = ["http1", "tokio"]` — the minimum for `axum::serve()`. The user enables additional features (`json`, `ws`, etc.) in their own `Cargo.toml`. Cargo unifies features across the dependency graph.

Feature definition:
```toml
http = ["dep:axum", "shutdown"]
```

This activates `shutdown`, which activates `_async` and brings in `tokio` + `tokio-util`.

## Public API Surface

### HttpConfig (`src/http/config.rs`)

```rust
/// Configuration for the HTTP server.
///
/// All fields have sensible defaults and can be deserialized from TOML:
///
/// ```toml
/// [http]
/// host = "127.0.0.1"
/// port = 3000
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    // pub(crate) fields, accessed through HttpBuilder
    host: String,   // default: "0.0.0.0"
    port: u16,      // default: 8080
}
```

Implements `Default` with `0.0.0.0:8080`.

Future enhancement: additional listener tuning fields (TCP keepalive, SO_REUSEADDR, backlog). These can be added as defaulted fields without breaking existing configs.

### HttpBuilder (`src/http/builder.rs`)

```rust
/// Builder for HTTP server configuration.
#[must_use]
pub struct HttpBuilder {
    config: HttpConfig,
}

impl HttpBuilder {
    pub fn new() -> Self;                              // defaults: 0.0.0.0:8080
    pub fn from_config(config: HttpConfig) -> Self;    // bridge from deserialized config
    pub fn host(mut self, host: impl Into<String>) -> Self;
    pub fn port(mut self, port: u16) -> Self;
    pub(crate) fn into_config(self) -> HttpConfig;
}
```

Implements `Default`, `Debug`, `Clone`.

Future enhancement: `from_listener(TcpListener)` constructor for pre-bound listeners.

### Http (`src/http/init.rs`)

```rust
/// Handle to the HTTP server, providing serve and address inspection.
pub struct Http {
    listener: Mutex<Option<TcpListener>>,
    local_addr: SocketAddr,
    token: CancellationToken,
}

impl Http {
    /// Returns the local address the server is bound to.
    ///
    /// This returns the address that was bound at build time, regardless
    /// of whether `serve()` has been called or has already returned.
    pub fn local_addr(&self) -> SocketAddr;

    /// Serves the given router until shutdown is triggered.
    ///
    /// Consumes the bound listener internally. This is a long-running
    /// future that blocks until the shutdown subsystem's cancellation
    /// token is triggered. The router must have state already applied
    /// via `.with_state()`.
    ///
    /// After `serve()` returns, call `ctx.shutdown().unwrap().wait().await`
    /// to run registered cleanup hooks (e.g., database pool close).
    ///
    /// Returns `HttpError::AlreadyServing` if called more than once.
    pub async fn serve(&self, router: axum::Router) -> Result<(), HttpError>;
}
```

`init_http` is `pub(crate)`:

```rust
pub(crate) async fn init_http(
    config: &HttpConfig,
    token: CancellationToken,
) -> Result<Http, HttpError>;
```

Binds `TcpListener` to `config.host:config.port`, stores the local address (mapping `local_addr()` failure to `BindFailed` since it's part of the bind process), wraps everything in the `Http` handle.

### HttpError (`src/http/error.rs`)

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpError {
    /// TCP listener failed to bind to the configured address.
    #[error("failed to bind to {addr}")]
    BindFailed {
        addr: String,
        source: std::io::Error,
    },

    /// serve() has already been called — it may only be called once.
    #[error("serve() has already been called — it may only be called once")]
    AlreadyServing,

    /// HTTP was registered but shutdown was not.
    #[error("HTTP subsystem requires shutdown — call with_shutdown() on the builder")]
    ShutdownRequired,

    /// The axum server returned an error.
    #[error("server error")]
    ServeFailed {
        source: std::io::Error,
    },
}
```

### AppContext integration

Two `impl` blocks for `with_http()`, following the same pattern as `with_shutdown()` and `with_sqlite()`:

```rust
// On AppContext<C>
#[cfg(feature = "http")]
pub fn http(&self) -> Option<&Http>;

// On AppContextBuilder — SyncBuild → AsyncBuild
// (calls into_async(), then sets http field)
#[cfg(feature = "http")]
pub fn with_http(self, builder: HttpBuilder) -> AppContextBuilder<Cfg, AsyncBuild>;

// On AppContextBuilder — AsyncBuild → AsyncBuild
// (sets http field directly, already in async state)
#[cfg(feature = "http")]
pub fn with_http(mut self, builder: HttpBuilder) -> Self;
```

### Top-level Error

Add `Http(HttpError)` variant to `src/error.rs`:

```rust
#[cfg(feature = "http")]
#[error(transparent)]
Http(#[from] HttpError),
```

### AppContext field ordering

The `Http` field is inserted between `extensions` and `shutdown` in `AppContext`. This matches the reserved comment slot already in the code (`// 3. [future: http_handle]`):

```rust
pub struct AppContext<C> {
    config: C,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "http")]
    http: Option<Http>,           // After serve() returns, the listener has been consumed
                                  // and this is effectively empty — drop is a no-op.
                                  // Placed before shutdown so the port is unbound first
                                  // in the edge case where serve() was never called.
    #[cfg(feature = "shutdown")]
    shutdown: Option<Shutdown>,
    #[cfg(feature = "sqlite")]
    sqlite_pool: Option<SqlitePool>,
    #[cfg(feature = "logging")]
    log_guard: Option<WorkerGuard>,
}
```

### Build sequence

Inside `async fn build()`:

```
1. Logging (sync) — so other subsystems can log during init
2. Shutdown — installs signal handlers, produces CancellationToken
3. HTTP — binds listener, requires token from step 2
4. SQLite — creates pool
```

This is a hardcoded sequence, not a dependency graph (see Design Decision 8). The order is correct: HTTP needs the shutdown token, and all subsystems benefit from logging being live during init. The existing TODO comment about topological sort in `context/mod.rs` should be removed.

## Files to Modify

Beyond the new `src/http/` module, the following existing files need changes:

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `axum` dependency, uncomment and define `http` feature |
| `src/lib.rs` | Add `#[cfg(feature = "http")] pub mod http;` re-export |
| `src/error.rs` | Add `Http(HttpError)` variant to top-level `Error` |
| `src/context/mod.rs` | Add `http: Option<Http>` field to `AppContext` struct |
| | Add `http: Option<HttpBuilder>` field to `AppContextBuilder` struct |
| | Initialize `http: None` in `AppContext::builder()` constructor |
| | Add `http` field propagation to `into_async()` (future-safety: currently dead code for HTTP since `with_http()` on `SyncBuild` sets the field *after* `into_async()`, but other async subsystems calling `into_async()` on a builder with `http` already set would lose it without this) |
| | Add `http` field propagation to `with_config()` (same pattern — copies all builder fields during `NoConfig → Configured<C>` transition) |
| | Add `http` field to `Debug` impl for `AppContextBuilder` |
| | Add `with_http()` impl blocks (SyncBuild and AsyncBuild) |
| | Add HTTP init step to `async fn build()` (between shutdown and sqlite) |
| | Update `build()` docstring to include HTTP in init sequence |
| | Remove the TODO comment about topological sort |
| | Update the `[future: http_handle]` comment to reflect the actual field |

## Usage Examples

### Simple microservice

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use dragon_fnd::http::HttpBuilder;
use dragon_fnd::shutdown::ShutdownBuilder;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: MyConfig = ConfigBuilder::new()
        .with_file("config.toml", true)
        .build()?;

    let http_config = config.http.clone();

    let ctx = Arc::new(
        AppContext::builder()
            .with_config(config)
            .with_shutdown(ShutdownBuilder::new())
            .with_http(HttpBuilder::from_config(http_config))
            .build()
            .await?
    );

    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .with_state(ctx.clone());

    // serve() blocks until the shutdown signal fires and axum drains in-flight requests.
    // wait() then runs registered cleanup hooks (e.g., database pool close).
    if let Some(http) = ctx.http() {
        http.serve(router).await?;
    }
    if let Some(shutdown) = ctx.shutdown() {
        shutdown.wait().await?;
    }
    Ok(())
}
```

### Full API server

```rust
let http_config = config.http.clone();
let logging_config = config.logging.clone();
let db_config = config.database.clone();

let ctx = Arc::new(
    AppContext::builder()
        .with_logging(LoggingBuilder::from_config(logging_config))
        .with_config(config)
        .with_shutdown(ShutdownBuilder::new())
        .with_http(HttpBuilder::from_config(http_config))
        .with_sqlite(SqliteBuilder::from_config(db_config))
        .build()
        .await?
);

let router = api::router(ctx.clone());  // user builds complex router

if let Some(http) = ctx.http() {
    http.serve(router).await?;
}
if let Some(shutdown) = ctx.shutdown() {
    shutdown.wait().await?;
}
```

## Error Handling

All errors are explicit, no panics:

| Scenario | Error | When |
|----------|-------|------|
| Port in use | `HttpError::BindFailed` | `build().await` |
| No shutdown registered | `HttpError::ShutdownRequired` | `build().await` |
| `serve()` called twice | `HttpError::AlreadyServing` | `serve().await` |
| Axum server error | `HttpError::ServeFailed` | `serve().await` |

## Module Structure

```
src/http/
├── mod.rs      # Re-exports, pub(crate) init_http
├── config.rs   # HttpConfig (serde, private fields)
├── builder.rs  # HttpBuilder (fluent API)
├── init.rs     # Http struct, init_http(), serve()
└── error.rs    # HttpError enum
```

5 source files. Follows the exact pattern of `shutdown/`, `sqlite/`, and `logging/`.

## Test Strategy

### Unit tests (in-module `#[cfg(test)]`)

- **HttpConfig**: default values, custom values, addr formatting
- **HttpBuilder**: `new()` defaults, `from_config()` bridging, fluent setters, `into_config()` round-trip
- **HttpError**: Display messages for each variant

### Integration tests (`tests/http_*.rs`)

- **Build with HTTP**: verify `ctx.http()` returns `Some`, `local_addr()` returns bound address
- **Build without HTTP**: verify `ctx.http()` returns `None` (when feature enabled but `with_http()` not called)
- **Bind to port 0**: verify `local_addr()` returns an assigned port (standard test pattern)
- **Bind failure**: bind to a port already in use, verify `HttpError::BindFailed`
- **Missing shutdown**: call `with_http()` without `with_shutdown()`, verify `HttpError::ShutdownRequired`
- **Serve and shutdown**: start serving, trigger shutdown programmatically, verify `serve()` returns `Ok(())`
- **Serve twice**: call `serve()` twice, verify `HttpError::AlreadyServing`
- **Config deserialization**: deserialize `HttpConfig` from TOML, verify fields

### Doc-tests

- Usage example on `Http::serve()` (compile-only, `no_run`)
