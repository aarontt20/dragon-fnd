# dragon-fnd

A foundation library for Rust applications providing typed configuration loading and application context management.

## What It Does

dragon-fnd loads configuration from multiple sources (TOML files, environment variables, serializable structs, custom sources), deep-merges them in registration order, resolves `${path.to.field}` variable references, and deserializes the result into your typed config struct. An application context builder ties config to optional subsystems — logging, SQLite, and graceful shutdown — using type-state to enforce correctness at compile time.

## Features

All subsystems are feature-gated. You only pay for what you enable.

| Feature | Provides |
|---------|----------|
| *(default)* | Config loading, variable resolution, AppContext builder |
| `logging` | Tracing subscriber setup with console/file output, size-based rotation, compression, retention |
| `sqlite` | SQLite connection pool via sqlx with PRAGMA configuration and migrations |
| `shutdown` | Graceful shutdown with signal handling, cancellation tokens, and cleanup hooks |

## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
dragon-fnd = { path = "../dragon-fnd" }
# or with features:
dragon-fnd = { path = "../dragon-fnd", features = ["logging", "sqlite", "shutdown"] }
```

### Config loading

```rust
use dragon_fnd::config::{ConfigBuilder, FileSource, EnvSource};

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
use dragon_fnd::context::AppContext;

let ctx = AppContext::builder()
    .with_config(config)
    .build_sync()?;

let cfg = ctx.config();
```

With SQLite (requires `sqlite` feature and async runtime):

```rust
let ctx = AppContext::builder()
    .with_config(config)
    .with_sqlite(sqlite_builder)
    .build()
    .await?;

let pool = ctx.sqlite(); // Option<&SqlitePool>
```

With graceful shutdown (requires `shutdown` feature and async runtime):

```rust
use dragon_fnd::shutdown::ShutdownBuilder;
use std::time::Duration;

let ctx = AppContext::builder()
    .with_config(config)
    .with_shutdown(ShutdownBuilder::new().grace_period(Duration::from_secs(30)))
    .build()
    .await?;

let shutdown = ctx.shutdown().unwrap();
let token = shutdown.token();

// Pass token to workers for cooperative cancellation
tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = do_work() => {}
        }
    }
});

// Register cleanup for your resources
shutdown.register_cleanup("cache-flush", move || async move {
    cache.flush().await;
})?;

// Block until signal + cleanup complete
shutdown.wait().await?;
```

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
cargo test --features shutdown,sqlite,logging     # Run all tests
cargo clippy                                      # Lint
```
