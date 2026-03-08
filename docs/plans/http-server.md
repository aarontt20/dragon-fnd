# HTTP Server Design Plan

Axum server lifecycle management behind an `http` feature flag. The library manages binding, serving, and graceful shutdown integration. The user owns routing, middleware, and request handling.

---

## Problem Statement

Applications built on dragon-fnd need HTTP server lifecycle management — binding a listener, serving requests, and shutting down gracefully on signals. This is pure boilerplate that every HTTP service rewrites. The library should own the lifecycle; the user should own the application logic.

---

## Design Decisions

### Hard dependency on shutdown

The `http` feature activates `shutdown` automatically. Every HTTP server needs graceful shutdown — a server without it is a production footgun. If the user calls `with_http()` without `with_shutdown()`, a default `ShutdownBuilder` (30s grace period) is auto-registered during `build()`.

### Handle-centric architecture

`Http` struct on `AppContext`, matching the pattern established by `Shutdown`. The handle holds parsed config and a cloned `CancellationToken`. Created at `build()` time (infallible — no I/O). Accessed via `ctx.http() -> Option<&Http>`.

### Two-step lifecycle: bind then serve

`bind()` and `serve()` are separate calls on the `Http` handle. This supports port-0 binding (OS-assigned ports for tests), logging the listen address before serving, and users who bring their own listener.

### serve() returns a future

`serve()` is a regular async method — the user drives the future however they want (`.await` directly, `tokio::spawn`, `select!`). The common case is `ctx.http().unwrap().serve(listener, router).await?` as the last line in `main()`. The future completes when shutdown fires and axum finishes draining connections.

### No cleanup hook registration

The `serve()` future IS the lifecycle. When the shutdown token is cancelled, axum's `with_graceful_shutdown` stops accepting connections and drains in-flight requests. The future then completes naturally. No shutdown cleanup hook needed — the server's teardown is expressed by the future completing, not by a side effect.

### No trait boundary

Same deferral as shutdown and sqlite. There is no second HTTP server implementation to design a trait against. Users who want a different server (e.g., warp, poem) skip the `http` feature and use `with_extension()`.

---

## Dependencies

| Crate | Version | What for | Feature |
|-------|---------|----------|---------|
| `axum` | 0.8 | `axum::serve`, `Router`, `with_graceful_shutdown` | `http` |
| `tokio` | 1 | `TcpListener::bind` (needs `net` feature added) | via `shutdown` |

Feature flag in `Cargo.toml`:

```toml
http = ["dep:axum", "shutdown"]
```

Tokio feature list updated from `signal, rt, time, macros` to `signal, rt, time, macros, net`.

No new transitive dependencies — axum already depends on tokio, hyper, tower, etc.

---

## Module Structure

```
src/http/
├── mod.rs          # Re-exports, pub(crate) init_http
├── config.rs       # HttpConfig (serde, pub(crate) fields)
├── builder.rs      # HttpBuilder (fluent API)
├── init.rs         # Http handle, BoundListener, bind(), serve()
└── error.rs        # HttpError enum
```

---

## Public API Surface

### HttpConfig

Serde-deserializable configuration with `pub(crate)` fields:

```rust
pub struct HttpConfig {
    pub(crate) host: String,        // default: "0.0.0.0"
    pub(crate) port: u16,           // default: 8080
    pub(crate) tcp_nodelay: bool,   // default: true
}
```

TOML shape:

```toml
[http]
host = "127.0.0.1"
port = 3000
tcp_nodelay = true
```

Defaults via explicit `Default` impl with `#[serde(default)]`.

### HttpBuilder

Fluent API wrapping `HttpConfig`. `#[must_use]`, `Debug`, `Clone`.

```rust
HttpBuilder::new()                       // 0.0.0.0:8080, tcp_nodelay=true
HttpBuilder::from_config(http_config)    // bridge from deserialized TOML

.host(impl Into<String>)                 // override host
.port(u16)                               // override port
.tcp_nodelay(bool)                       // override tcp_nodelay
.into_config() -> HttpConfig             // pub(crate)
```

### Http

Runtime handle. Created at `build()` time, stored on `AppContext`.

```rust
pub struct Http {
    config: HttpConfig,
    token: CancellationToken,
}
```

Methods:

- `bind(&self) -> Result<BoundListener, HttpError>` — binds a TCP listener from config
- `serve(&self, TcpListener, axum::Router) -> Result<(), HttpError>` — serves with graceful shutdown wired to the token

### BoundListener

Thin wrapper for address inspection:

- `local_addr(&self) -> SocketAddr`
- `into_listener(self) -> TcpListener`

### AppContext integration

```rust
#[cfg(feature = "http")]
pub fn http(&self) -> Option<&Http>
```

Builder: `with_http(HttpBuilder)` transitions `SyncBuild → AsyncBuild` (or stays `AsyncBuild`).

### init_http (pub(crate))

```rust
pub(crate) fn init_http(builder: HttpBuilder, token: CancellationToken) -> Http
```

Infallible — no I/O at init time.

---

## Error Handling

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpError {
    #[error("failed to bind listener to {addr}")]
    BindFailed {
        addr: String,
        #[source]
        source: std::io::Error,
    },

    #[error("server error")]
    ServeFailed {
        #[source]
        source: std::io::Error,
    },
}
```

Top-level `Error`:

```rust
#[cfg(feature = "http")]
#[error(transparent)]
Http(#[from] HttpError),
```

No `#[from]` on `HttpError` variants — both wrap `io::Error` with different semantics.

---

## AppContext Build Order

Inside `async fn build()`:

1. **Logging** (sync) — so other subsystems can log during init
2. **Shutdown** — signal handlers installed; auto-registered if `with_http()` was called without `with_shutdown()`
3. **HTTP** — receives cloned token from shutdown (infallible)
4. **SQLite** — database pool

Auto-registration logic:

```rust
let shutdown = match (self.shutdown, self.http.is_some()) {
    (Some(builder), _) => Some(init_shutdown(builder)?),
    (None, true) => Some(init_shutdown(ShutdownBuilder::new())?),
    (None, false) => None,
};
```

AppContext field order (for correct drop sequence):

```rust
pub struct AppContext<C> {
    config: C,
    extensions: HashMap<...>,
    #[cfg(feature = "http")]
    http: Option<Http>,
    #[cfg(feature = "shutdown")]
    shutdown: Option<Shutdown>,
    #[cfg(feature = "sqlite")]
    sqlite_pool: Option<SqlitePool>,
    #[cfg(feature = "logging")]
    log_guard: Option<WorkerGuard>,
}
```

`Http` holds no resources requiring ordered cleanup (just config + token clone), so position is for consistency with the existing placeholder comment, not correctness.

---

## Usage Example

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use dragon_fnd::http::HttpBuilder;

#[derive(Debug, Deserialize)]
struct MyConfig {
    http: dragon_fnd::http::HttpConfig,
    // ...
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: MyConfig = ConfigBuilder::new()
        .with_file("config.toml", true)
        .build()?;

    let ctx = AppContext::builder()
        .with_http(HttpBuilder::from_config(config.http))
        .with_config(config)
        .build()
        .await?;

    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    let http = ctx.http().unwrap();
    let listener = http.bind().await?;
    tracing::info!("listening on {}", listener.local_addr());
    http.serve(listener.into_listener(), router).await?;

    Ok(())
}
```

---

## Test Strategy

### Unit tests (~8-10)

- **config.rs** — default values, serde deserialization from TOML, partial TOML with defaults
- **builder.rs** — `new()` defaults, fluent setters, `from_config()` round-trip, `Debug`/`Clone`
- **init.rs** — `init_http()` wires config and token correctly, `Http` debug output

### Integration tests (~8-10)

- `bind()` succeeds on available port
- `bind()` with port 0 yields OS-assigned port, `local_addr()` reflects it
- `bind()` on invalid address returns `BindFailed`
- `serve()` shuts down cleanly when token is cancelled
- `serve()` with `tcp_nodelay(false)` doesn't panic
- `into_listener()` yields a usable `TcpListener`
- AppContext `with_http()` transitions to `AsyncBuild`, `build().await` succeeds, `ctx.http()` returns `Some`
- AppContext without `with_http()` — `ctx.http()` returns `None`
- Auto-registration of shutdown when only `with_http()` is called
- `compile_fail` — `with_http().build_sync()` rejected

### Not tested

- Axum's request handling, routing, middleware — axum's responsibility
- Actual HTTP request/response round-trips
- Signal delivery — covered by shutdown subsystem tests
