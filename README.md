# dragon-fnd

A foundation library for Rust applications providing typed configuration loading and application context management.

## What It Does

dragon-fnd loads configuration from multiple sources (TOML files, environment variables, serializable structs, custom sources), deep-merges them in registration order, resolves `${path.to.field}` variable references, and deserializes the result into your typed config struct. An application context builder ties config to optional subsystems — logging, SQLite, HTTP, and graceful shutdown — using type-state to enforce correctness at compile time.

## Features

All subsystems are feature-gated. You only pay for what you enable.

| Feature | Provides |
|---------|----------|
| *(default)* | Config loading, variable resolution, AppContext builder |
| `logging` | Tracing subscriber setup with console/file output, size-based rotation, compression, retention |
| `sqlite` | SQLite connection pool via sqlx with PRAGMA configuration and migrations |
| `shutdown` | Graceful shutdown with signal handling, cancellation tokens, and cleanup hooks |
| `http` | Axum server lifecycle management with TCP binding and graceful shutdown (implies `shutdown`) |

## Quick Start

A complete microservice using config, logging, SQLite, HTTP, and graceful shutdown:

**`Cargo.toml`**:

```toml
[dependencies]
dragon-fnd = { path = "../dragon-fnd", features = ["logging", "sqlite", "http"] }
axum = "0.8"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

**`config/default.toml`**:

```toml
[logging]
filter = "info"

[http]
host = "127.0.0.1"
port = 3000

[database]
path = "data/app.db"
migrate = true
```

**`src/main.rs`**:

```rust
use dragon_fnd::config::ConfigBuilder;
use dragon_fnd::http::HttpBuilder;
use dragon_fnd::logging::LoggingBuilder;
use dragon_fnd::shutdown::ShutdownBuilder;
use dragon_fnd::sqlite::SqliteBuilder;
use dragon_fnd::AppContext;
use std::sync::Arc;

#[derive(serde::Deserialize, Debug, Clone)]
struct AppConfig {
    logging: dragon_fnd::logging::LoggingConfig,
    http: dragon_fnd::http::HttpConfig,
    database: dragon_fnd::sqlite::SqliteConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load config from file + env vars (APP__HTTP__PORT=9090 overrides http.port)
    let config: AppConfig = ConfigBuilder::new()
        .with_file("config/default.toml", true)
        .with_file("config/local.toml", false)
        .with_env("APP", "__")
        .build()?;

    // Extract sub-configs before moving the full config into the context
    let logging_config = config.logging.clone();
    let http_config = config.http.clone();
    let db_config = config.database.clone();

    // Build the application context — subsystems init in dependency order
    let ctx = Arc::new(
        AppContext::builder()
            .with_logging(LoggingBuilder::from_config(logging_config))
            .with_config(config)
            .with_shutdown(ShutdownBuilder::new())
            .with_http(HttpBuilder::from_config(http_config))
            .with_sqlite(SqliteBuilder::from_config(db_config))
            .build()
            .await?,
    );

    // Build your router — you own routing, middleware, and handlers
    let router = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }));

    // serve() blocks until shutdown signal (SIGTERM/SIGINT/Ctrl+C)
    ctx.http().unwrap().serve(router).await?;

    // wait() runs cleanup hooks (e.g., pool.close()) within the grace period
    ctx.shutdown().unwrap().wait().await?;

    Ok(())
}
```

That's it. Config is loaded and merged, logging is live, the database pool is ready, and the HTTP server shuts down cleanly on signal. Everything below this point covers the individual subsystems in more detail.

---

## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
dragon-fnd = { path = "../dragon-fnd" }
# or with features:
dragon-fnd = { path = "../dragon-fnd", features = ["logging", "sqlite", "http"] }
```

### Config loading

```rust
use dragon_fnd::config::ConfigBuilder;

#[derive(serde::Deserialize)]
struct AppConfig {
    server: ServerConfig,
    database: DatabaseConfig,
}

let config: AppConfig = ConfigBuilder::new()
    .with_file("config/defaults.toml", true)
    .with_file("config/local.toml", false)
    .with_env("APP", "__")
    .build()?;
```

Sources are merged in registration order — later sources override earlier ones. Variable references like `${database.host}` are resolved after merge.

### Application context

```rust
use dragon_fnd::AppContext;

let ctx = AppContext::builder()
    .with_config(config)
    .build_sync()?;

let cfg = ctx.config();
```

Registering any async subsystem (sqlite, shutdown, http) transitions the builder to async — use `build().await` instead of `build_sync()`.

### Custom config sources

```rust
use dragon_fnd::config::{ConfigSource, ConfigEntry, ConfigValue, ConfigError};

struct MySource;

impl ConfigSource for MySource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        Ok(vec![ConfigEntry::at_path(
            vec!["my".into(), "key".into()],
            ConfigValue::string("value"),
        )])
    }
}

let config: T = ConfigBuilder::new()
    .with_source(MySource)
    .build()?;
```

## Building

```bash
cargo build                                       # Build the library
cargo test                                        # Run base tests
cargo test --features logging                     # Include logging tests
cargo test --features sqlite                      # Include sqlite tests
cargo test --features shutdown                    # Include shutdown tests
cargo test --features http                        # Include http tests (implies shutdown)
cargo test --features http,sqlite,logging         # Run all tests
cargo clippy                                      # Lint
```
