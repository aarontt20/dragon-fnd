# Plan: Logging Subsystem for dragon-fnd

## Context

dragon-fnd's config subsystem and AppContext are built. The next step per VISION.md is the logging subsystem — the first feature-gated subsystem. This plan covers the full implementation: config types, tracing-based subscriber initialization, file rotation with retention, AppContext integration, and the `build_sync()` signature change from infallible to fallible.

**Design decisions made during brainstorming:**
- **Environment**: No first-class concept. Users layer config files. Library has no opinion.
- **Scope**: Full — console + file with time-based rotation and retention
- **Trait**: None — tracing is the Rust ecosystem's de facto logging abstraction. The `tracing` crate itself is the trait boundary (instrument with `tracing::info!()`, swap subscribers freely). The library configures the subscriber; users extend via tracing-subscriber's layer system. CLAUDE.md Constraint 4 is satisfied by tracing's own architecture, not a library-level trait. Update CLAUDE.md and VISION.md to document this exception.
- **Config shape**: Nested `[logging]`, `[logging.console]`, `[logging.file]`
- **Rotation**: Time-based only (daily/hourly/never) via tracing-appender. Size-based deferred (see extensibility note).

---

## Module Structure

```
src/logging/
├── mod.rs          # Re-exports, pub(crate) init_logging
├── config.rs       # LoggingConfig, ConsoleConfig, FileConfig, LogFormat, Rotation (serde types)
├── builder.rs      # LoggingBuilder, ConsoleBuilder, FileBuilder (fluent API)
├── error.rs        # LoggingError (4 variants)
└── init.rs         # Subscriber initialization (layer composition), retention cleanup
```

All behind `#[cfg(feature = "logging")]`.

---

## Config Types (`src/logging/config.rs`) — Serde Deserialization

```toml
[logging]
enabled = true                     # master switch, default: true
filter = "info"                    # EnvFilter directive, default: "info"
modules = { sqlx = "warn" }       # per-module overrides, default: {}

[logging.console]
enabled = true                     # default: true
format = "pretty"                  # pretty | json | compact, default: pretty
# filter = "warn"                  # optional per-layer override of base filter

[logging.file]
enabled = false                    # default: false (opt-in)
dir = "./logs"                     # default: "./logs"
prefix = "app"                     # default: "app"
format = "json"                    # default: json
# filter = "debug"                 # optional per-layer override (e.g. verbose to file, terse to console)
rotation = "daily"                 # daily | hourly | never, default: daily
retain_days = 7                    # delete files older than N days, optional
# retain_files = 10               # keep only N most recent files, optional (mutually exclusive with retain_days)
```

Types:
- `LoggingConfig` — top level, `#[derive(Debug, Clone, Deserialize)]`, all fields have serde defaults
- `ConsoleConfig` — `enabled: bool`, `format: LogFormat`, `filter: Option<String>` (per-layer override)
- `FileConfig` — `enabled`, `dir: PathBuf`, `prefix: String`, `format: LogFormat`, `rotation: Rotation`, `filter: Option<String>` (per-layer override), `retain_days: Option<u32>`, `retain_files: Option<u32>`
- `LogFormat` — `#[serde(rename_all = "lowercase")]` enum: Pretty (default), Json, Compact
- `Rotation` — `#[serde(rename_all = "lowercase")]` enum: Daily (default), Hourly, Never

Retention uses two explicit fields (`retain_days`, `retain_files`) with mutual exclusion validation — at most one may be set. No custom deserializer needed. Validated during `init_logging()`, producing `InvalidRetention` if both are set.

`modules` uses `BTreeMap<String, String>` for deterministic ordering.

These types exist for deserialization only. Users interact with logging configuration through builders (see below).

---

## Builder Types (`src/logging/builder.rs`) — Public API

Each builder wraps its corresponding config type internally. `from_config()` bridges deserialized config into the builder; fluent methods override individual fields. No duplicate field definitions — the config types are the single source of truth for the data shape.

```rust
/// Top-level logging builder. This is what `with_logging()` accepts.
pub struct LoggingBuilder {
    config: LoggingConfig,  // wrapped, not duplicated
}

impl LoggingBuilder {
    /// Programmatic construction with sensible defaults.
    pub fn new() -> Self;

    /// Bridge from deserialized config.
    pub fn from_config(config: &LoggingConfig) -> Self;

    /// Override the base filter directive.
    pub fn filter(mut self, filter: impl Into<String>) -> Self;

    /// Add a per-module filter override.
    pub fn module(mut self, module: impl Into<String>, level: impl Into<String>) -> Self;

    /// Enable/disable logging entirely.
    pub fn enabled(mut self, enabled: bool) -> Self;

    /// Configure console output.
    pub fn console(mut self, console: ConsoleBuilder) -> Self;

    /// Configure file output.
    pub fn file(mut self, file: FileBuilder) -> Self;
}
```

```rust
pub struct ConsoleBuilder {
    config: ConsoleConfig,
}

impl ConsoleBuilder {
    pub fn new() -> Self;
    pub fn format(mut self, format: LogFormat) -> Self;
    pub fn filter(mut self, filter: impl Into<String>) -> Self;
    pub fn enabled(mut self, enabled: bool) -> Self;
}
```

```rust
pub struct FileBuilder {
    config: FileConfig,
}

impl FileBuilder {
    /// Takes directory path — file output is enabled by default when constructed.
    pub fn new(dir: impl Into<PathBuf>) -> Self;
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self;
    pub fn format(mut self, format: LogFormat) -> Self;
    pub fn filter(mut self, filter: impl Into<String>) -> Self;
    pub fn rotation(mut self, rotation: Rotation) -> Self;
    pub fn retain_days(mut self, days: u32) -> Self;
    pub fn retain_files(mut self, count: u32) -> Self;
}
```

Key design decisions:
- `FileBuilder::new(dir)` enables file output automatically — if you construct a `FileBuilder`, you want file logging. No need to separately set `enabled = true`.
- `LoggingBuilder::new()` starts with the same defaults as `LoggingConfig`'s serde defaults (console enabled, file disabled, filter "info").
- `from_config()` takes `&LoggingConfig` and clones — allows the hybrid pattern where you load from config and override.
- Builders implement `Debug` and `Clone`.
- `init_logging` takes `&LoggingConfig` internally — builders resolve to their wrapped config via `pub(crate) fn into_config(self) -> LoggingConfig`.

### Usage patterns

```rust
// Config-driven (most common)
let ctx = AppContext::builder()
    .with_logging(LoggingBuilder::from_config(&config.logging))
    .with_config(config)
    .build_sync()?;

// Pure programmatic (no config file needed for logging)
let ctx = AppContext::builder()
    .with_logging(
        LoggingBuilder::new()
            .filter("debug")
            .console(ConsoleBuilder::new().format(LogFormat::Pretty))
            .file(
                FileBuilder::new("./logs")
                    .prefix("myapp")
                    .rotation(Rotation::Daily)
                    .retain_days(7)
            )
    )
    .with_config(config)
    .build_sync()?;

// Hybrid — load from config, override for this run
let ctx = AppContext::builder()
    .with_logging(
        LoggingBuilder::from_config(&config.logging)
            .filter("debug")
    )
    .with_config(config)
    .build_sync()?;
```

---

## Error Type (`src/logging/error.rs`)

```rust
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LoggingError {
    #[error("failed to initialize tracing subscriber")]
    InitFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("invalid log filter directive: {0}")]
    InvalidFilter(String),

    #[error("invalid retention config: {0}")]
    InvalidRetention(String),

    #[error("failed to create log directory '{}'", dir.display())]
    FileSetupFailed { dir: PathBuf, source: std::io::Error },
}
```

`InvalidRotation` removed — serde handles invalid rotation strings before library code sees them. `InitFailed` preserves the source error as `Box<dyn Error>` to avoid coupling the public API to tracing-subscriber types while maintaining the error chain.

Added to top-level Error as `#[cfg(feature = "logging")] Logging(#[from] LoggingError)`.

---

## Init Logic (`src/logging/init.rs`)

`pub(crate) fn init_logging(config: &LoggingConfig) -> Result<Option<WorkerGuard>, LoggingError>`

1. If `!config.enabled`, return `Ok(None)`
2. Validate retention config: if both `retain_days` and `retain_files` are set, return `InvalidRetention`
3. Build base `EnvFilter` from `config.filter` + per-module directives from `config.modules`
4. Build console layer (if `console.enabled`):
   - Build per-layer `EnvFilter` from `console.filter` override (if set), else clone base filter
   - Build fmt layer with format-specific variant, `.with_filter(console_filter)`, boxed
5. Build file layer (if `file.enabled`):
   - `create_dir_all(dir)` — fail with `FileSetupFailed` if can't
   - Run retention cleanup (if rotation != Never and retention is configured) — returns `Vec<(PathBuf, io::Error)>` of failures
   - Create `tracing_appender::rolling::{daily|hourly|never}` appender
   - Wrap in `tracing_appender::non_blocking()` → `(NonBlocking, WorkerGuard)`
   - Build per-layer `EnvFilter` from `file.filter` override (if set), else clone base filter
   - Build fmt layer with `with_writer(non_blocking)`, `with_ansi(false)`, format-specific variant, `.with_filter(file_filter)`, boxed
6. Compose: `tracing_subscriber::registry().with(console_layer).with(file_layer)`
7. Call `try_init()` — on any error, return `Ok(None)` (the only failure mode is "already set")
8. Log retention cleanup errors via `tracing::warn!` (subscriber is now live)
9. Return `Ok(guard)` — the `Option<WorkerGuard>` from the file layer

Key details:
- `try_init()` not `init()` — tests may initialize logging multiple times. All `try_init()` errors treated as "subscriber already set" (no string matching — this is the only failure mode)
- `with_ansi(false)` on file layer — no ANSI escape codes in log files
- Per-layer `EnvFilter` — each layer gets its own filter (base + optional override), enabling patterns like "console at warn, file at debug"
- No `RUST_LOG` special handling — config is the single source of truth. Users override via `MYAPP__LOGGING__FILTER` through the existing `EnvSource`

### Retention cleanup (`cleanup_old_logs`)

`fn cleanup_old_logs(dir: &Path, prefix: &str, retain_days: Option<u32>, retain_files: Option<u32>) -> Vec<(PathBuf, std::io::Error)>`

Lives in `init.rs` (not a separate file — it's one function called from one place). Scans dir for files matching prefix, sorts by mtime.
- `retain_days`: delete files older than N days
- `retain_files`: keep only N most recent files

Returns a `Vec` of `(path, error)` pairs for files that couldn't be deleted. The caller (`init_logging`) logs these via `tracing::warn!` after the subscriber is initialized. This satisfies the "no silent failures" constraint without blocking startup. Uses `std::fs::FileTimes` (stable since Rust 1.75) in tests — no `filetime` dev-dependency needed.

---

## AppContext Integration

### `build_sync()` becomes fallible (BREAKING)

**Before**: `fn build_sync(self) -> AppContext<C>`
**After**: `fn build_sync(self) -> Result<AppContext<C>, Error>`

This change is **unconditional** (not cfg-gated) because:
- Feature-flag-dependent signatures are confusing
- Future subsystems also make it fallible
- Doc comment already promised this change
- No external consumers (pre-1.0)

### AppContext gains `log_guard` field

```rust
pub struct AppContext<C> {
    config: C,
    #[cfg(feature = "logging")]
    log_guard: Option<WorkerGuard>,  // LAST field — drops last
}
```

`Option` because: logging disabled, subscriber already set, or console-only (no guard needed).
Last field ensures logging outlives all other subsystem handles during drop.

### Builder gains `with_logging()` method

```rust
#[cfg(feature = "logging")]
pub fn with_logging(mut self, builder: LoggingBuilder) -> Self
```

Available on **all builder states** (`NoConfig` and `Configured<C>`) so users can register subsystems before moving config. Typical usage:

```rust
let ctx = AppContext::builder()
    .with_logging(LoggingBuilder::from_config(&config.logging))
    .with_config(config)
    .build_sync()?;
```

`AppContextBuilder` stores `Option<LoggingBuilder>`. `build_sync()` calls `builder.into_config()` then `init_logging` if Some.

### Builder field propagation

`builder()` and `with_config()` must initialize/carry the `#[cfg(feature = "logging")] logging: Option<LoggingBuilder>` field (initialized to `None`).

### Debug impl update

The manual `Debug` impl on `AppContext` must cfg-gate the `log_guard` field:

```rust
impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContext");
        s.field("config", &self.config);
        #[cfg(feature = "logging")]
        s.field("log_guard", &"<WorkerGuard>");
        s.finish()
    }
}
```

---

## Cargo.toml Changes

```toml
[features]
default = []
logging = ["dep:tracing", "dep:tracing-subscriber", "dep:tracing-appender"]

[dependencies]
tracing = { version = "0.1", optional = true }
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"], optional = true }
tracing-appender = { version = "0.2", optional = true }
```

No `filetime` dev-dependency — retention tests use `std::fs::FileTimes` (stable since Rust 1.75).

---

## lib.rs Changes

```rust
#[cfg(feature = "logging")]
pub mod logging;
```

`pub mod` because users need access to `LoggingConfig` (for their config structs), `LoggingBuilder`/`ConsoleBuilder`/`FileBuilder` (for the API), and shared types like `LogFormat` and `Rotation`.

---

## Size-Based Rotation Extensibility

Adding size-based rotation later means:
1. Add `file-rotate` as optional dependency gated behind `logging` feature
2. Add `Size(u64)` variant to `Rotation` with custom `Deserialize` for `"size:100mb"`
3. Add a match arm in `build_file_layer` for the `Size` variant
4. Skip retention cleanup for `Size` variant (file-rotate manages its own)

`Rotation` is not `#[non_exhaustive]` — adding `Size` changes match arms, and the compiler will flag every incomplete match (the behavior you want). This is a minor version bump for a pre-1.0 crate. Document in VISION.md.

---

## Testing Strategy

### Unit tests in `src/logging/config.rs`
- `LoggingConfig` defaults: verify all serde defaults are sensible
- Per-layer filter override: console and file configs parse `filter` field
- Retention fields: `retain_days` only, `retain_files` only, neither, both (validation tested in init)
- Full TOML round-trip: parse complete config, check all fields

### Unit tests in `src/logging/builder.rs`
- `LoggingBuilder::new()` defaults match `LoggingConfig` serde defaults
- `from_config()` round-trips: deserialize config, build from it, verify all fields
- Fluent overrides: `.filter()`, `.module()`, `.enabled()` modify the wrapped config
- `ConsoleBuilder::new()` defaults, `.format()`, `.filter()` overrides
- `FileBuilder::new(dir)` enables file output, `.prefix()`, `.rotation()`, `.retain_days()`, `.retain_files()` overrides
- `LoggingBuilder::console()` and `.file()` replace sub-configs

### Unit tests in `src/logging/init.rs`
- `build_env_filter`: valid base, with modules, invalid base, invalid module
- `build_file_layer`: creates directory, returns guard when enabled, returns None when disabled
- `disabled_logging_returns_none`: master switch off
- Retention validation: both `retain_days` and `retain_files` set → `InvalidRetention`
- `cleanup_old_logs`: days retention (deletes old, keeps recent), files retention (deletes oldest over limit, keeps all under), nonexistent dir (no panic), non-matching prefix (ignored)
- Uses `std::fs::FileTimes` for setting modification times in retention tests

### Integration tests
- `tests/logging_init.rs`: full `init_logging()` call (one test per binary to avoid global subscriber conflicts)
- `tests/context.rs`: update existing tests for `build_sync()` → `Result` change
- `tests/config_builder.rs`: update existing tests for `build_sync()` → `Result` change (indirect — these don't use AppContext but the example does)

### Doc tests
- Update existing `no_run` and `compile_fail` doc-tests for `build_sync()` returning `Result`

---

## Implementation Sequence

### Step 1: Make `build_sync()` return `Result` (prerequisite, no logging)
- Change signature in `src/context/mod.rs`
- Update all existing tests (`tests/context.rs`) to call `.unwrap()` or `?`
- Update doc-tests and example
- Commit separately — isolates the breaking change

### Step 2: Add logging config, builders, and error types
- Create `src/logging/mod.rs`, `config.rs`, `builder.rs`, `error.rs`
- Add `LoggingError` variant to `src/error.rs` (cfg-gated)
- Add `#[cfg(feature = "logging")] pub mod logging` to `src/lib.rs`
- Add unit tests for config parsing (defaults, per-layer filter, retention fields)
- Add unit tests for builders (defaults, from_config round-trip, fluent overrides)
- No tracing deps yet — these are pure serde types and builder wrappers

### Step 3: Add init logic and retention cleanup
- Create `src/logging/init.rs` (includes `cleanup_old_logs`)
- Add tracing dependencies to `Cargo.toml`
- Activate `logging` feature flag
- Add unit tests for filter building, layer construction, retention validation, retention cleanup

### Step 4: Integrate with AppContextBuilder
- Add `log_guard` field to `AppContext`
- Add `logging: Option<LoggingBuilder>` field to `AppContextBuilder`
- Add `with_logging(LoggingBuilder)` method (available on all builder states: `NoConfig` and `Configured<C>`)
- Wire `builder.into_config()` → `init_logging` into `build_sync()`
- Update `Debug` impl (cfg-gate the `log_guard` field)
- Add integration tests

### Step 5: Update docs and examples
- Update `examples/example.rs` for `build_sync()` → `Result`
- Update DESIGN.md: add logging section, update AppContext section
- Update CLAUDE.md: add logging to module structure, update test counts, document Constraint 4 exception for logging
- Update DOC.md: add logging module documentation
- Update TEST.md: add logging test coverage
- Update VISION.md: mark logging as built, update "Design Boundary" table for logging (tracing is the interface), remove `?` from `with_logging()` in usage shape, note size-based rotation as future extension

---

## Verification

1. `cargo test` — all tests pass (existing + new logging tests)
2. `cargo test --features logging` — logging-specific tests pass
3. `cargo test` (without logging feature) — existing tests still pass, no regressions
4. `cargo clippy --features logging` — no warnings
5. `cargo clippy` (without logging feature) — no warnings
6. `cargo build --features logging` — clean build
7. `cargo doc --features logging` — documentation generates cleanly
8. Example runs: `cargo run --features logging --example example`

---

## Critical Files

| File | Action |
|------|--------|
| `Cargo.toml` | Edit — add logging feature, optional deps |
| `src/lib.rs` | Edit — add cfg-gated `pub mod logging` |
| `src/error.rs` | Edit — add cfg-gated `Logging(LoggingError)` variant |
| `src/context/mod.rs` | Edit — `build_sync()` → Result, log_guard field, with_logging(LoggingBuilder) on all builder states, cfg-gated Debug field |
| `src/logging/mod.rs` | Create — re-exports |
| `src/logging/config.rs` | Create — serde config types (LoggingConfig, ConsoleConfig, FileConfig, LogFormat, Rotation) |
| `src/logging/builder.rs` | Create — fluent API (LoggingBuilder, ConsoleBuilder, FileBuilder) |
| `src/logging/error.rs` | Create — LoggingError (4 variants: InitFailed, InvalidFilter, InvalidRetention, FileSetupFailed) |
| `src/logging/init.rs` | Create — subscriber initialization + retention cleanup |
| `tests/context.rs` | Edit — adapt for build_sync() returning Result |
| `tests/logging_init.rs` | Create — integration test for init |
| `examples/example.rs` | Edit — adapt for build_sync() returning Result |
| `docs/DESIGN.md` | Edit — add logging section |
| `docs/VISION.md` | Edit — update logging status, update Design Boundary table, fix with_logging() usage shape, add size rotation note |
| `CLAUDE.md` | Edit — module structure, test counts, Constraint 4 logging exception |
| `DOC.md` | Edit — add logging API docs |
| `TEST.md` | Edit — add logging test coverage |

Reference files (read-only):
- `../dragon-fnd-old/src/logging.rs` — old implementation patterns
- `../dragon-seekers/ds-core/src/logging.rs` — real consumer patterns
