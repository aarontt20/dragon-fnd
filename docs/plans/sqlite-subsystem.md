# Plan: SQLite Database Subsystem

## Context

dragon-fnd needs its first async subsystem: SQLite database support via sqlx. This is feature-gated behind `sqlite` and introduces the `AsyncBuild` type-state marker, setting the pattern for all future async subsystems (HTTP, shutdown). The old version (`dragon-fnd-old/src/database.rs`) has a working reference implementation with known issues to avoid.

## Design Decisions

**1. No library-level Database trait.** Unlike tracing (which provides a genuine ecosystem-wide interface — any crate can `tracing::info!()` and any subscriber can consume it), sqlx is a concrete implementation, not a trait boundary. There is no useful database abstraction in the Rust ecosystem that sqlx implements and others could substitute. This is the second explicit exception to Constraint 4, justified differently from logging: tracing IS the interface; sqlx is merely the default implementation. Users who want a custom pool (rusqlite, diesel, SurrealDB) skip the `sqlite` feature and use `with_extension()`.

**2. `path` field, not `url`.** Config takes `"app.db"` or `":memory:"`. Library builds the sqlx connection string internally. Cleaner for SQLite-only.

**3. Configurable PRAGMAs.** `journal_mode` (enum, default WAL), `foreign_keys` (bool, default true), and `busy_timeout` (seconds, default 5) exposed in config/builder. Set via `SqliteConnectOptions` so they apply per-connection, not just the first connection in the pool. `busy_timeout` controls how long SQLite waits when the database is locked by another connection — critical for concurrent access with WAL mode. Note: `journal_mode` is a database-level setting — per-connection setting is redundant after the first, but harmless.

**4. `build().await` only on AsyncBuild.** Sync-only users always use `build_sync()`. No dual availability.

**5. Connectivity always tested at init.** No `test_connection` toggle — the old version's deferred testing was a known problem.

**6. Runtime migrations.** Filesystem-based via `sqlx::migrate::Migrator::new(path)`. Error if `migrate=true` and directory missing (no silent failures).

**7. No special shutdown logic yet.** sqlx Pool handles drop gracefully. Future `shutdown` subsystem will call `pool.close().await` before drop for explicit async teardown.

**8. WAL + `:memory:` detection.** WAL journal mode is unsupported for in-memory databases — SQLite silently falls back to `memory` mode. Per Constraint 2 (no silent failures), `init_pool` logs a warning when `journal_mode = Wal` and `path = ":memory:"`.

**9. `Option` accessor, not type-level availability.** VISION.md aspires to making `ctx.sqlite()` not exist when `with_sqlite()` wasn't called. The pragmatic choice is `Option<&SqlitePool>` — same pattern as `extension::<T>()`. This does not close doors; the builder's type-state transition (SyncBuild → AsyncBuild) is the hard part and is handled correctly. The accessor return type can evolve independently.

**10. Module named `sqlite/`, not `database/`.** Module name matches the feature flag and the specific backend. Accessor is `ctx.sqlite()`. This scales to multiple backends without naming collisions — a future Postgres subsystem would be `src/postgres/` with `ctx.postgres()`, and exotic backends use `with_extension()`.

## Usage Example

```toml
# config.toml
[app]
name = "my-service"

[sqlite]
path = "data/app.db"
migrate = true
migrations_dir = "./migrations"
```

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use dragon_fnd::sqlite::SqliteBuilder;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MyConfig {
    app: AppConfig,
    sqlite: SqliteConfig,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: MyConfig = ConfigBuilder::new()
        .with_file("config.toml", true)
        .with_env("MYAPP", "__")
        .build()?;

    // Note: .clone() because config.sqlite is moved into from_config,
    // and config itself is moved into with_config. This is a one-time
    // startup cost — the compiler will tell you if you forget.
    let ctx = AppContext::builder()
        .with_sqlite(SqliteBuilder::from_config(config.sqlite.clone()))
        .with_config(config)
        .build()
        .await?;

    let pool = ctx.sqlite().expect("sqlite was registered");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    println!("{} users", count.0);
    Ok(())
}
```

Programmatic setup without TOML (same result, no config file):

```rust
let ctx = AppContext::builder()
    .with_sqlite(
        SqliteBuilder::new("data/app.db")
            .migrate(true)
            .migrations_dir("./migrations")
    )
    .with_config(app_config)
    .build()
    .await?;
```

## Module Structure

```
src/sqlite/
├── mod.rs       # Re-exports, pub(crate) init_pool, pub use sqlx::SqlitePool
├── config.rs    # SqliteConfig, JournalMode (serde, private fields)
├── builder.rs   # SqliteBuilder (fluent API)
├── error.rs     # SqliteError enum
└── init.rs      # Pool creation, connectivity test, migrations
```

## Files to Create

### `src/sqlite/mod.rs`

```rust
mod builder;
mod config;
mod error;
mod init;

pub(crate) use init::init_pool;

pub use builder::SqliteBuilder;
pub use config::{SqliteConfig, JournalMode};
pub use error::SqliteError;

// Re-export pool type so users can write dragon_fnd::sqlite::SqlitePool
// without depending on sqlx directly.
pub use sqlx::SqlitePool;
```

### `src/sqlite/error.rs`

```rust
use std::io;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SqliteError {
    #[error("database path cannot be empty")]
    EmptyPath,

    #[error("failed to create database directory '{}'", dir.display())]
    DirectoryCreationFailed {
        dir: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create database connection pool")]
    PoolCreationFailed {
        #[source]
        source: sqlx::Error,
    },

    #[error("database connectivity test failed")]
    ConnectivityTestFailed {
        #[source]
        source: sqlx::Error,
    },

    #[error("migrations directory not found: '{}'", _0.display())]
    MigrationsDirNotFound(PathBuf),

    #[error("database migration failed")]
    MigrationFailed {
        #[source]
        source: sqlx::migrate::MigrateError,
    },
}
```

### `src/sqlite/config.rs`

```rust
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SqliteConfig {
    pub(crate) path: String,              // Required. "" = error at init
    pub(crate) max_connections: u32,      // Default: 5
    pub(crate) min_connections: u32,      // Default: 1
    pub(crate) acquire_timeout_secs: u64, // Default: 10
    pub(crate) idle_timeout_secs: u64,    // Default: 300
    pub(crate) migrate: bool,             // Default: false
    pub(crate) migrations_dir: PathBuf,   // Default: "./migrations"
    pub(crate) journal_mode: JournalMode, // Default: Wal
    pub(crate) foreign_keys: bool,        // Default: true
    pub(crate) busy_timeout_secs: u64,    // Default: 5
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            max_connections: 5,
            min_connections: 1,
            acquire_timeout_secs: 10,
            idle_timeout_secs: 300,
            migrate: false,
            migrations_dir: PathBuf::from("./migrations"),
            journal_mode: JournalMode::Wal,
            foreign_keys: true,
            busy_timeout_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JournalMode {
    Wal,
    Delete,
    Memory,
}
```

Private fields, access via builder only (matches logging config pattern).

### `src/sqlite/builder.rs`

```rust
#[must_use = "builders do nothing until passed to AppContextBuilder::with_sqlite()"]
pub struct SqliteBuilder {
    config: SqliteConfig,
}
```

- `new(path: impl Into<String>)` — quick setup with defaults
- `from_config(SqliteConfig)` — bridge from deserialized TOML
- Fluent setters: `migrate(bool)`, `migrations_dir(impl Into<PathBuf>)`, `max_connections(u32)`, `min_connections(u32)`, `acquire_timeout_secs(u64)`, `idle_timeout_secs(u64)`, `journal_mode(JournalMode)`, `foreign_keys(bool)`, `busy_timeout_secs(u64)`
- `pub(crate) into_config(self) -> SqliteConfig`

Note: no `Default` impl — `path` is required and has no sensible default. This is an intentional deviation from `LoggingBuilder::new()` which has meaningful defaults without arguments.

### `src/sqlite/init.rs`

```rust
pub(crate) async fn init_pool(config: &SqliteConfig) -> Result<SqlitePool, SqliteError>
```

Sequence:
1. Validate path (non-empty) → `SqliteError::EmptyPath`
2. Warn if `journal_mode = Wal` and `path = ":memory:"` (WAL unsupported for in-memory databases)
3. Create parent directory if file-based (not `:memory:`) → `SqliteError::DirectoryCreationFailed`
4. Build `SqliteConnectOptions` with `.filename()`, `.create_if_missing(true)`, `.journal_mode()`, `.foreign_keys()`, `.busy_timeout(Duration::from_secs(config.busy_timeout_secs))`
5. Create pool via `SqlitePoolOptions::new().connect_with(options).await` → `SqliteError::PoolCreationFailed`
6. Test connectivity: `SELECT 1` → `SqliteError::ConnectivityTestFailed`
7. Run migrations if `config.migrate` is true → `SqliteError::MigrationsDirNotFound` or `SqliteError::MigrationFailed`

Key: PRAGMAs set via `SqliteConnectOptions`, not raw PRAGMA queries. This ensures every connection in the pool gets them, not just the first.

## Files to Modify

### `src/context/mod.rs` — Major changes

**Add `AsyncBuild` marker** (uncomment + flesh out):
```rust
#[doc(hidden)]
pub struct AsyncBuild;
```

**Update `AppContext` struct** with field ordering contract:
```rust
pub struct AppContext<C> {
    // Fields ordered for correct drop sequence (Rust drops in declaration order):
    // 1. config — pure data, no cleanup
    // 2. extensions — user-provided, drop before subsystems
    // 3. [future: http_handle] — drop server before database
    // 4. sqlite_pool — close database connections (logging during drop is captured)
    // 5. log_guard — MUST be last, flushes pending log writes
    config: C,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "sqlite")]
    sqlite_pool: Option<sqlx::SqlitePool>,  // Future: shutdown subsystem will call pool.close().await before drop
    #[cfg(feature = "logging")]
    #[allow(dead_code)]
    log_guard: Option<WorkerGuard>,
}
```

**Update `AppContext` Debug impl:**
```rust
impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContext");
        s.field("config", &self.config);
        s.field("extensions", &self.extensions.len());
        #[cfg(feature = "sqlite")]
        s.field("sqlite_pool", &self.sqlite_pool.is_some());
        #[cfg(feature = "logging")]
        s.field("log_guard", &"<WorkerGuard>");
        s.finish()
    }
}
```

**Add accessor:**
```rust
/// Returns a reference to the SQLite connection pool, if one was registered.
///
/// Returns `None` if `with_sqlite()` was not called during builder construction.
#[cfg(feature = "sqlite")]
pub fn sqlite(&self) -> Option<&sqlx::SqlitePool> {
    self.sqlite_pool.as_ref()
}
```

**Add `sqlite` field to `AppContextBuilder`:**
```rust
#[cfg(feature = "sqlite")]
sqlite: Option<crate::sqlite::SqliteBuilder>,
```

**Update `AppContextBuilder` Debug impl:**
```rust
impl<Cfg, A> std::fmt::Debug for AppContextBuilder<Cfg, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContextBuilder");
        s.field("extensions", &self.extensions.len());
        #[cfg(feature = "logging")]
        s.field("logging", &self.logging.is_some());
        #[cfg(feature = "sqlite")]
        s.field("sqlite", &self.sqlite.is_some());
        s.finish()
    }
}
```

**Update `builder()` to initialize the new field:**
```rust
pub fn builder() -> AppContextBuilder<NoConfig> {
    AppContextBuilder {
        cfg: NoConfig,
        _async: PhantomData,
        extensions: HashMap::new(),
        #[cfg(feature = "logging")]
        logging: None,
        #[cfg(feature = "sqlite")]
        sqlite: None,
    }
}
```

**Update `with_config()` to propagate the new field:**
```rust
pub fn with_config<C>(self, config: C) -> AppContextBuilder<Configured<C>, A> {
    AppContextBuilder {
        cfg: Configured(config),
        _async: PhantomData,
        extensions: self.extensions,
        #[cfg(feature = "logging")]
        logging: self.logging,
        #[cfg(feature = "sqlite")]
        sqlite: self.sqlite,
    }
}
```

**Add `into_async()` helper** — centralizes SyncBuild→AsyncBuild field propagation in one place. Without this, every async subsystem's SyncBuild→AsyncBuild transition must manually propagate every other subsystem's field — O(N*M) boilerplate that grows with each new subsystem. With `into_async()`, new subsystems add one line to this method instead of editing every other subsystem's transition:

```rust
impl<Cfg> AppContextBuilder<Cfg, SyncBuild> {
    fn into_async(self) -> AppContextBuilder<Cfg, AsyncBuild> {
        AppContextBuilder {
            cfg: self.cfg,
            _async: PhantomData,
            extensions: self.extensions,
            #[cfg(feature = "logging")]
            logging: self.logging,
            #[cfg(feature = "sqlite")]
            sqlite: self.sqlite,
            // Future async subsystems add ONE line here
        }
    }
}
```

**Add `with_sqlite()` — two impl blocks:**

```rust
// SyncBuild → AsyncBuild transition (uses into_async helper)
#[cfg(feature = "sqlite")]
impl<Cfg> AppContextBuilder<Cfg, SyncBuild> {
    pub fn with_sqlite(self, builder: SqliteBuilder) -> AppContextBuilder<Cfg, AsyncBuild> {
        let mut b = self.into_async();
        b.sqlite = Some(builder);
        b
    }
}

// AsyncBuild → AsyncBuild (chaining with future async subsystems)
#[cfg(feature = "sqlite")]
impl<Cfg> AppContextBuilder<Cfg, AsyncBuild> {
    pub fn with_sqlite(mut self, builder: SqliteBuilder) -> Self {
        self.sqlite = Some(builder);
        self
    }
}
```

**Ensure `with_logging` and `with_extension` work on AsyncBuild.** The existing `impl<Cfg, A>` blocks are generic over `A`, so they already cover `AsyncBuild`. The `with_config()` and `builder()` changes above handle field propagation.

**Add `async fn build()` on `Configured<C>, AsyncBuild`:**
```rust
impl<C> AppContextBuilder<Configured<C>, AsyncBuild> {
    pub async fn build(self) -> Result<AppContext<C>, Error> {
        // Init logging (sync) — before other subsystems so they can log
        #[cfg(feature = "logging")]
        let log_guard = match self.logging {
            Some(builder) => crate::logging::init_logging(&builder.into_config())?,
            None => None,
        };

        // Init sqlite (async)
        // TODO: Replace hardcoded init sequence with topological sort when 3+ subsystems exist
        #[cfg(feature = "sqlite")]
        let sqlite_pool = match self.sqlite {
            Some(builder) => Some(crate::sqlite::init_pool(&builder.into_config()).await?),
            None => None,
        };

        Ok(AppContext {
            config: self.cfg.0,
            extensions: self.extensions,
            #[cfg(feature = "sqlite")]
            sqlite_pool,
            #[cfg(feature = "logging")]
            log_guard,
        })
    }
}
```

**Update `build_sync()` to propagate the new field:**
```rust
pub fn build_sync(self) -> Result<AppContext<C>, Error> {
    #[cfg(feature = "logging")]
    let log_guard = match self.logging {
        Some(builder) => crate::logging::init_logging(&builder.into_config())?,
        None => None,
    };

    Ok(AppContext {
        config: self.cfg.0,
        extensions: self.extensions,
        #[cfg(feature = "sqlite")]
        sqlite_pool: None,
        #[cfg(feature = "logging")]
        log_guard,
    })
}
```

**Add `compile_fail` doc-test** verifying `build_sync()` is unavailable after `with_sqlite()`:
```rust
/// ```compile_fail
/// use dragon_fnd::context::AppContext;
/// use dragon_fnd::sqlite::SqliteBuilder;
/// // ERROR: build_sync() is not available on AppContextBuilder<_, AsyncBuild>
/// let _ctx = AppContext::builder()
///     .with_config(())
///     .with_sqlite(SqliteBuilder::new("test.db"))
///     .build_sync();
/// ```
```

### `src/error.rs`

Add:
```rust
#[cfg(feature = "sqlite")]
#[error("sqlite error: {0}")]
Sqlite(#[from] crate::sqlite::SqliteError),
```

### `src/lib.rs`

Add module declaration (no crate-root re-exports — matches logging pattern):
```rust
#[cfg(feature = "sqlite")]
pub mod sqlite;
```

### `Cargo.toml`

```toml
[features]
sqlite = ["dep:sqlx"]

[dependencies]
sqlx = { version = "0.8", features = ["sqlite", "runtime-tokio"], optional = true }

[dev-dependencies]
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros"] }
```

Note: no direct `dep:tokio` in the `sqlite` feature. sqlx's `runtime-tokio` brings in tokio transitively. The library code uses only `async`/`.await` (language features), not tokio APIs directly. Dev-dependency `tokio` provides `#[tokio::test]` for integration tests.

## Implementation Sequence

### Step 1: SQLite module skeleton
Create `src/sqlite/{mod.rs, error.rs, config.rs, builder.rs, init.rs}`. Wire up `mod sqlite` in `lib.rs` behind `#[cfg(feature = "sqlite")]`. Add `sqlite` feature and deps to `Cargo.toml`. Verify: `cargo check --features sqlite`.

### Step 2: SqliteError + top-level Error variant
Implement error types with full `#[error]` and `#[source]` annotations. Add `Sqlite` variant to `src/error.rs`. Unit tests for Display output.

### Step 3: SqliteConfig + JournalMode
Serde-deserializable types with private fields. `#[derive(Debug, Clone, PartialEq, Deserialize)]` with `#[serde(default)]` on `SqliteConfig`. `#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]` with `#[serde(rename_all = "lowercase")]` on `JournalMode`. Explicit `Default` impl on `SqliteConfig`. Unit tests for TOML round-trips, default values, JournalMode deserialization.

### Step 4: SqliteBuilder
Fluent API wrapping SqliteConfig. `#[must_use]` attribute. `new(path: impl Into<String>)`, `from_config()`, all setters including `migrate(bool)` and `busy_timeout_secs(u64)`. No `Default` impl (path is required). Unit tests for builder chain, from_config bridge.

### Step 5: init_pool implementation
Async init function: validate → warn WAL+:memory: → create dir → build options → create pool → test connectivity → migrate. Unit tests with `:memory:` databases.

### Step 6: AsyncBuild type-state + AppContextBuilder changes
Add `AsyncBuild` marker, `into_async()` helper for centralized field propagation, `sqlite` field, `with_sqlite()` transitions (both SyncBuild→AsyncBuild via `into_async()` and AsyncBuild→AsyncBuild), `async fn build()` with cfg-gated logging and sqlite init. Propagate `sqlite` field through all state transitions: `builder()`, `with_config()`, `into_async()`, `build_sync()`. Update both manual `Debug` impls (AppContext and AppContextBuilder). Add `compile_fail` doc-test verifying `with_sqlite().build_sync()` does not compile.

### Step 7: AppContext sqlite accessor
Add `sqlite_pool` field with drop-ordering comment block documenting the full field order contract. Add `sqlite()` method returning `Option<&SqlitePool>` with doc comment explaining `None` when `with_sqlite()` wasn't called. Update `AppContext` `Debug` impl.

### Step 8: Integration tests
- `tests/sqlite.rs` — pool creation with `:memory:`, connectivity, migrations from temp dir
- `tests/sqlite.rs` — PRAGMA verification: `foreign_keys` and `busy_timeout` testable with `:memory:`; `journal_mode` (WAL) requires a file-based DB in a tempdir since `:memory:` doesn't support WAL
- `tests/context_async.rs` — full builder chain with `#[tokio::test]`
- Test with `cargo test --features sqlite`

### Step 9: Documentation updates
- `docs/DESIGN.md` — sqlite subsystem section
- `docs/VISION.md` — mark SQLite as built
- `DOC.md` — API docs
- `TEST.md` — updated counts
- `CLAUDE.md` — add `cargo test --features sqlite` build command

## Verification

```bash
cargo check --features sqlite                  # Compiles
cargo check                                     # Still compiles without sqlite
cargo clippy --features sqlite                  # No warnings
cargo clippy                                    # No warnings without sqlite
cargo test --features sqlite                    # SQLite tests pass
cargo test --features logging                   # Logging tests still pass
cargo test                                      # Base tests still pass
cargo test --features sqlite,logging            # Combined features work
cargo doc --features sqlite                     # Docs render
```
