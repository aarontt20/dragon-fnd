# DESIGN.md — How the Built System Works

This document describes dragon-fnd as it exists in code today. For future plans and the full vision, see [VISION.md](VISION.md).

---

## What It Does

dragon-fnd is a foundation library for Rust applications that provides typed configuration loading from multiple sources (TOML files, environment variables, custom sources) with deep merge semantics, graph-based variable reference resolution, and a type-state application context builder. It ships as a single crate with feature-gated subsystems — currently config (always on) and logging (`logging` feature). The library does not own `main()` and does not prescribe application structure.

---

## Module Structure

```
src/
├── lib.rs                 # Crate root, re-exports public API
├── error.rs               # Top-level Error enum (2 variants)
├── config/
│   ├── mod.rs             # Public exports: ConfigBuilder, ConfigError, ConfigSource, ConfigEntry, ConfigValue, ConfigTable, SerdeSource
│   ├── source.rs          # ConfigSource trait, ConfigEntry, ConfigValue, ConfigTable, merge_at_path, deep_merge
│   ├── builder.rs         # ConfigBuilder: fluent API, generic build::<T>()
│   ├── file.rs            # FileSource: loads TOML files
│   ├── env.rs             # EnvSource: loads environment variables
│   ├── serde_source.rs    # SerdeSource: serializes any Serialize type into config
│   ├── resolve.rs         # Graph-based ${path.to.field} variable resolution
│   └── error.rs           # ConfigError enum (16 variants)
├── logging/               # Feature: "logging"
│   ├── mod.rs             # Re-exports, pub(crate) init_logging
│   ├── config.rs          # LoggingConfig, ConsoleConfig, FileConfig, LogFormat, RotationStrategy (serde, private fields)
│   ├── builder.rs         # LoggingBuilder, ConsoleBuilder, FileBuilder (fluent API)
│   ├── error.rs           # LoggingError enum (5 variants)
│   ├── init.rs            # Subscriber initialization, layer composition, validation
│   ├── retain.rs          # Retention cleanup: delete old rotated log files
│   └── writer.rs          # SizeRotatingWriter: size-based rotation with compression
└── context/
    └── mod.rs             # AppContext<C> with type-state AppContextBuilder, extension slot
```

18 source files. Tests are inline (`#[cfg(test)]` modules) plus integration tests in `tests/`.

---

## Config System

### ConfigSource Trait and ConfigEntry

All configuration sources implement one trait (`src/config/source.rs`):

```rust
pub trait ConfigSource: Send + Sync + std::fmt::Debug {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>;
}
```

Sources produce `ConfigEntry` values — each pairing a path with a `ConfigValue`:

```rust
pub struct ConfigEntry {
    pub path: Vec<String>,
    pub value: ConfigValue,
}
```

`ConfigValue` is the library-owned value type that replaces `toml::Value` in the public API. Variants: `String`, `Integer(i64)`, `Float(f64)`, `Boolean(bool)`, `Datetime(String)`, `Array(Vec<ConfigValue>)`, `Table(ConfigTable)`. `ConfigTable` is a newtype over `BTreeMap<String, ConfigValue>`. Conversion to `toml::Value` happens at a single boundary inside `ConfigBuilder::build()`.

Two constructors serve different use cases:
- `ConfigEntry::root(table)` — `pub(crate)`, empty path, for internal sources that produce a complete `toml::Table` (files)
- `ConfigEntry::at_path(path, value)` — public, specific path segments, for sources that produce individual `ConfigValue` entries (env vars, custom sources)

`merge_at_path()` returns `Result<(), ConfigError>` and handles all merge scenarios with a `let-else` pattern for the empty-path case:
1. **Empty path + table value**: deep merge at root level — nested tables merge recursively, non-table values return `ConfigError::RootNotTable`
2. **Non-empty path, final key**: if both existing and incoming are tables, deep merge; otherwise replace entirely
3. **Non-empty path, intermediate segments**: navigate to the target, auto-creating missing intermediate tables. If a non-table value already exists at an intermediate path, returns `ConfigError::TypeConflict` (e.g., scalar `server` when a later source tries to set `server.port`)

`deep_merge()` is the recursive helper: for each key in the overlay, if both sides have tables at that key, recurse; otherwise the overlay value replaces the base value.

### FileSource

`src/config/file.rs` — a TOML file loader. Wraps a `PathBuf` and a `required: bool` flag.

- Produces a single `ConfigEntry::root(table)` by reading and parsing the file
- When `required` is false and the file is missing, returns an empty entries vec (no error)
- When `required` is true and the file is missing, returns `ConfigError::FileNotFound`
- I/O failures return `ConfigError::ReadError`; invalid TOML returns `ConfigError::ParseError`

### EnvSource

`src/config/env.rs` — loads configuration from environment variables. Configured with a `prefix` and `separator`.

The constructor is infallible — validation is deferred to `entries()`, consistent with how `FileSource` validates at `entries()` time. An empty prefix produces `ConfigError::InvalidPrefix`; an empty separator produces `ConfigError::InvalidSeparator`.

Scanning logic:
1. For each env var, check if the key starts with `{prefix}{separator}`
2. Strip the prefix and separator
3. Split the remainder on the separator to produce path segments
4. Lowercase each segment

Produces one `ConfigEntry::at_path(path, coerced_value)` per matching variable.

Value coercion applies in order:
1. **Boolean** — case-insensitive `true`/`false`
2. **Integer** — optional leading `-` followed by ASCII digits only (no scientific notation, no hex, no leading zeros)
3. **Float** — must contain `.` and parse as `f64`
4. **String** — fallback for everything else

### SerdeSource

`src/config/serde_source.rs` — serializes any `T: Serialize` into the config pipeline. Most commonly used to feed parsed CLI arguments into `ConfigBuilder`, but works with any serializable struct.

- `SerdeSource::new(&value)?` serializes eagerly via `toml::Table::try_from()` at construction time, unlike `FileSource` and `EnvSource` which validate at `entries()` time. This is intentional — the input is available immediately, so errors are reported at the call site where the user created the source.
- Produces a single `ConfigEntry::root(table)` — the same pattern as `FileSource`.
- `Option::None` fields are omitted from the table, so they do not override values from lower-priority sources. This relies on the `toml` crate's handling of `UnsupportedNone` during serialization.
- Types that cannot be represented as a TOML table (bare scalars, bare arrays, `u64` exceeding `i64::MAX`) produce `ConfigError::SerializeError`.
- Always-on (no feature gate) — uses only `serde` + `toml`, both already always-on dependencies.

### ConfigBuilder

`src/config/builder.rs` — orchestrates sources into a typed config value.

```rust
pub struct ConfigBuilder {
    sources: Vec<Box<dyn ConfigSource>>,
}
```

Fluent API:
- `ConfigBuilder::new()` — creates an empty builder
- `.with_file(path, required)` — adds a `FileSource`
- `.with_env(prefix, separator)` — adds an `EnvSource`
- `.with_source(impl ConfigSource + 'static)` — adds any custom source

`build::<T>()` executes in three steps:
1. Iterate all sources in registration order, collect entries, merge into a single `toml::Table` via `merge_at_path` (later sources override earlier ones)
2. Resolve `${...}` variable references in the merged table
3. Deserialize the table into `T` via `toml::Value::try_into()`

Deserialization happens once at build time. After `build()`, config access is a plain struct field reference with no parsing overhead.

### Variable Resolution

`src/config/resolve.rs` — the most complex module. Uses a graph-based approach with four phases:

**Phase 1 — Collection.** Walk the entire merged table. For each string value, parse `${path.to.field}` references. Collect them as `(source_path, target_path)` edge pairs. `$$` is recognized as an escape (skipped during collection). Unclosed `${` produces `ConfigError::UnclosedReference`. References inside array elements are collected with position-indexed paths (e.g., `["arr", "0"]`), so a string at `arr[0]` that contains `${some.ref}` is tracked and resolved correctly. Note: user-written reference paths like `${arr.0}` cannot traverse into arrays — `lookup_value` only navigates tables.

**Phase 2 — Topological sort.** Build a dependency graph from the collected edges. Sort via DFS with an `in_progress` stack (Vec) for cycle detection. If a node is visited while already in the stack, the cycle path is reconstructed from the stack position where the node first appeared → `ConfigError::CircularReference(cycle_path)`. The result is a resolution order where dependencies come before dependents.

**Phase 3 — Resolution.** Process each path in topological order. Two modes:

- **Pure reference** — the string is exactly `${path}` (no whitespace, single reference, nothing else). The entire value is replaced with the target value's type — integers, booleans, arrays, and tables pass through intact. This enables type-preserving references. Whitespace around the reference (e.g., `"  ${path}  "`) is treated as string interpolation, not pure substitution.
- **String interpolation** — the string contains `${path}` embedded among other text. Each reference is replaced with the string representation of the target value. Scalars (string, integer, float, boolean, datetime) convert naturally. Non-scalar targets (arrays, tables) produce `ConfigError::NonScalarReference`.

Escape sequences: `$$` becomes literal `$` during resolution. Strings containing `${...}` references have their `$$` escapes processed during Phase 3. Strings that contain `$$` but no references are tracked separately and processed after graph-based resolution (Phase 4 — escape-only pass).

The `get_value` and `get_value_mut` functions are structurally identical — duplicated because Rust's borrow checker requires separate shared and mutable traversal paths. Both use `split_first()` for panic-safe path navigation.

---

## Logging System

**Feature:** `logging` — opt-in via `Cargo.toml`.

Structured logging via `tracing`, configured from the config layer or programmatically via fluent builders. The subsystem reads logging configuration, builds a `tracing-subscriber` with per-layer filters, and returns a `WorkerGuard` that `AppContext` holds for the application lifetime.

### Architecture Decision: No Trait Boundary

The logging subsystem does not define its own trait. `tracing` is the Rust ecosystem's de facto logging abstraction — instrument with `tracing::info!()`, swap subscribers freely via `tracing-subscriber`'s layer system. The library configures the subscriber; users extend via layers. This is an explicit exception to CLAUDE.md Constraint 4 ("trait boundary for every subsystem"), justified because `tracing` already provides the interface contract.

### Config Types (`src/logging/config.rs`)

Serde-deserializable types for TOML configuration:

- `LoggingConfig` — top-level: `enabled`, `filter` (EnvFilter directive), `modules` (per-module overrides), nested `console` and `file`
- `ConsoleConfig` — `enabled`, `format`, optional `filter` override
- `FileConfig` — `enabled`, `dir`, `prefix`, `format`, `rotation` (`RotationStrategy`), optional `filter` override, `compress` (gzip rotated files), optional `retain_days` or `retain_files` (mutually exclusive). All fields are `pub(crate)` — accessed through builders, not direct field mutation. Uses custom `Deserialize` impl to map flat TOML into `RotationStrategy` enum.
- `LogFormat` — `Pretty` | `Json` | `Compact`
- `RotationStrategy` — `Daily` | `Hourly` | `SizeBased { max_bytes: u64 }` | `Never`. Replaces the previous `Rotation` enum + separate `max_bytes` field — the invalid combination of `max_bytes` + time-based rotation is now structurally impossible.

### Builder Types (`src/logging/builder.rs`)

Fluent API wrapping the config types — no field duplication:

- `LoggingBuilder` wraps `LoggingConfig`. Constructed via `new()` (defaults) or `from_config()` (bridging from deserialized config). Methods: `filter()`, `module()`, `enabled()`, `console()`, `file()`. `into_config()` is `pub(crate)` — consumed by `build_sync()`.
- `ConsoleBuilder` wraps `ConsoleConfig`. Methods: `format()`, `filter()`, `enabled()`.
- `FileBuilder` wraps `FileConfig`. Constructed via `new(dir)` which auto-enables file output. Methods: `prefix()`, `format()`, `filter()`, `rotation(RotationStrategy)`, `compress()`, `retain_days()`, `retain_files()`. The two retention methods are mutually exclusive — each clears the other. Size-based rotation is configured via `RotationStrategy::SizeBased { max_bytes }` — no separate `max_bytes()` method.

### Subscriber Initialization (`src/logging/init.rs`)

`init_logging(&LoggingConfig) -> Result<Option<WorkerGuard>, LoggingError>`

Layer composition uses `Vec<Box<dyn Layer<Registry> + Send + Sync>>` — layers are collected into a Vec and applied to the registry once via `.with(layers)`. This avoids the type-mismatch problem where each `.with()` call changes the subscriber type.

Each layer gets its own `EnvFilter`: the base filter (from `config.filter` + `config.modules`) plus an optional per-layer override. This enables patterns like "console at warn, file at debug".

`try_init()` failure returns `LoggingError::SubscriberAlreadySet` — the user's logging configuration was not applied and they need to know.

Validation runs when file logging is enabled, checking rotation rules before retention rules:
1. `SizeBased { max_bytes }` where `max_bytes < 4096` returns `InvalidRotation`
2. `compress` without rotation (`Never`) returns `InvalidRotation`
3. Retention mutual exclusion (`retain_days` + `retain_files` both set), zero values
4. `Never` rotation with retention returns `InvalidRetention`

Note: the previous checks for `max_bytes + time-based rotation` and `compress + time-based rotation` are no longer needed — `RotationStrategy` makes these invalid combinations structurally impossible.

### Retention Cleanup (`src/logging/retain.rs`)

`cleanup_old_logs()` scans a directory for rotated log files matching the prefix (both plain and `.gz` compressed), sorts by modification time, and deletes according to the retention policy (days-based or file-count-based). Deletion errors are collected as `Vec<(PathBuf, io::Error)>` — the caller decides how to surface them.

For time-based rotation, cleanup runs at startup before the subscriber is live. For size-based rotation, cleanup runs inline after each rotation inside `SizeRotatingWriter`.

### Size-Based Rotation (`src/logging/writer.rs`)

`SizeRotatingWriter` implements `std::io::Write` for size-based log file rotation. It writes to `{dir}/{prefix}` and when the file exceeds `max_bytes`, renames the active file with a UTC timestamp suffix (`{prefix}.YYYYMMDDTHHmmss.SSS`), opens a new active file, optionally spawns a background thread to compress the rotated file with gzip, and runs retention cleanup.

Key design decisions:
- **No internal locking.** `tracing_appender::non_blocking` serializes all writes through a single worker thread, so the writer is single-threaded.
- **All rotation errors are soft.** The writer IS the tracing writer — `tracing::warn!` would recurse. Errors use `eprintln!` with `dragon-fnd:` prefix. `non_blocking` silently swallows `io::Error` from `write()`, so hard errors would just be lost.
- **Background compression.** `compress_file()` runs in a detached `std::thread::spawn`. No shared state — the thread owns the path. Uses streaming `io::copy` from `BufReader` to `GzEncoder` to avoid loading entire files into memory.
- **Cascading rotation prevention.** `bytes_written` is reset to 0 at the start of `rotate()` before any I/O, so if rotation fails partway through, subsequent writes don't re-trigger rotation on every call.
- **Timestamp collision handling.** If the timestamp already exists (rapid rotation within 1ms), appends `.1`, `.2`, etc. TOCTOU race is acceptable because `non_blocking` enforces single-writer.

### Error Types (`src/logging/error.rs`)

`LoggingError` — 5 variants, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `InvalidFilter(String)` | Malformed EnvFilter directive |
| `InvalidRotation(String)` | Invalid rotation config (max_bytes too small, compress without rotation) |
| `InvalidRetention(String)` | Invalid retention config (mutual exclusion, zero values, retention without rotation) |
| `FileSetupFailed { dir, source }` | Could not create log directory (display shows path only; `source` available via `std::error::Error::source()`) |
| `SubscriberAlreadySet` | Global tracing subscriber already set — logging configuration was not applied |

---

## Error Types

### ConfigError

`src/config/error.rs` — 16 variants, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `FileNotFound(PathBuf)` | Required config file missing |
| `ReadError { path, source }` | I/O failure reading a file |
| `ParseError { path, source }` | Invalid TOML syntax (manually constructed with file path context) |
| `DeserializeError(toml::de::Error)` | Final deserialization into target type failed (no `#[from]` — explicit `map_err` at call sites) |
| `SerializeError(toml::ser::Error)` | Value cannot be serialized to TOML table (no `#[from]` — matches `DeserializeError` pattern) |
| `RootNotTable(String)` | Root-level `ConfigEntry` has non-table value (e.g., integer at empty path) |
| `CircularReference(Vec<String>)` | Cycle detected in variable reference graph, with dotted-path chain (e.g., `a.b -> c.d -> a.b`) |
| `ReferenceNotFound(String)` | `${path}` points to a nonexistent key |
| `InvalidReferencePath(String)` | Empty or malformed reference path (e.g., `${a..b}`) |
| `NonScalarReference(String)` | Embedded reference targets a table or array |
| `UnclosedReference` | Missing closing `}` in `${...}` |
| `InvalidSeparator` | EnvSource separator is empty |
| `InvalidPrefix` | EnvSource prefix is empty |
| `InvalidDatetime(String)` | Invalid datetime string passed to `ConfigValue::datetime()` or present in `ConfigValue::Datetime` variant during conversion |
| `TypeConflict { path, existing, incoming }` | Non-table value at intermediate path would be replaced by table (e.g., scalar `server` when env var sets `server.port`) |
| `EmptyPathSegment { var }` | Environment variable produces empty path segment (consecutive separators) |

### Top-level Error

`src/error.rs` — 2 variants, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `Config(ConfigError)` | Wraps any config error (with `#[from]` for `?` conversion) |
| `Logging(LoggingError)` | Wraps any logging error (cfg-gated behind `logging` feature) |

---

## AppContext

`src/context/mod.rs` — a generic container for application state, built with a type-state pattern.

```rust
pub struct AppContext<C> {
    config: C,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "logging")]
    log_guard: Option<WorkerGuard>,  // LAST field — drops last
}
```

`AppContext::config()` returns `&C` — zero-cost after build. `extension::<T>()` returns `Option<&T>` — looks up a type-keyed value registered via `with_extension()` on the builder. The `extensions` field is declared before `log_guard` so extensions drop before the logging guard flushes. The `log_guard` is held for its `Drop` implementation — when the context is dropped, the guard flushes pending log writes. It is the last struct field so it outlives all other subsystem handles during drop (Rust drops fields in declaration order).

### Type-State Builder

The builder uses type-state to enforce correct construction at compile time:

```rust
pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "logging")]
    logging: Option<LoggingBuilder>,
}
```

Two type-level dimensions:

**Config presence** — `NoConfig` vs `Configured<C>`:
- `AppContext::builder()` returns `AppContextBuilder<NoConfig, SyncBuild>`
- `.with_config(config)` transitions to `AppContextBuilder<Configured<C>, A>` (preserving the async parameter)
- `build_sync()` only exists on `Configured<C>` — calling it without config is a compile error

**Async requirements** — `SyncBuild` (only marker defined currently):
- `build_sync()` only exists on `SyncBuild` — future async subsystems will transition to `AsyncBuild`, removing `build_sync()` and requiring `build().await` instead
- `PhantomData<Async>` reserves the parameter with no layout cost

`build_sync()` returns `Result<AppContext<C>, Error>`. Even with no subsystems registered, the signature is fallible — future subsystems also need it, and feature-flag-dependent signatures are confusing.

**Subsystem registration** — `with_logging(LoggingBuilder)` and `with_extension(T: Send + Sync + 'static)` are available on all builder states (`NoConfig` and `Configured<C>`) via generic `impl<Cfg, A>` blocks. This lets users register subsystems and extensions before or after providing config. Fields are propagated through state transitions (`builder()` → `with_config()` → `build_sync()`). Extensions use a type-map (`HashMap<TypeId, Box<dyn Any + Send + Sync>>`) — if the same type is registered twice, the last value wins.

Marker types (`NoConfig`, `Configured<C>`, `SyncBuild`) are `pub` with `#[doc(hidden)]` — nameable for compiler error messages, hidden from generated docs. Standard Rust ecosystem convention for type-state markers.

`AppContext` uses a manual `Debug` impl rather than `#[derive(Debug)]` — `WorkerGuard` does not implement `Debug`, so the derive would break. The `log_guard` field is rendered as `"<WorkerGuard>"` in Debug output. `AppContextBuilder` also uses a manual Debug impl that shows whether logging is configured.

A `compile_fail` doc-test verifies that `AppContext::builder().build_sync()` does not compile (no config provided).

### Teardown Contract

`AppContext` follows two teardown strategies depending on the subsystem:

- **Sync handles** (e.g., `WorkerGuard` for logging): cleaned up via `Drop`. Rust drops struct fields in declaration order — fields are declared so that dependent handles drop before their dependencies.
- **Async handles** (e.g., db pool, HTTP server): will require an explicit `ctx.shutdown().await` method, implemented when the shutdown subsystem lands. `Drop` cannot `await`.

---

## Dependencies

Three always-on crates, plus optional crates behind feature flags:

| Crate | Version | Role | Feature |
|-------|---------|------|---------|
| `serde` | 1 (with `derive`) | `DeserializeOwned` bound on `build::<T>()` | always |
| `toml` | 0.8 | File parsing, `Value`/`Table` as intermediate representation | always |
| `thiserror` | 2 | Error derive macros | always |
| `tracing` | 0.1 | Logging instrumentation API | `logging` |
| `tracing-subscriber` | 0.3 (env-filter, json, fmt) | Subscriber layers and filtering | `logging` |
| `tracing-appender` | 0.2 | Non-blocking file appender with rotation | `logging` |
| `flate2` | 1 | Gzip compression for rotated log files | `logging` |
| `time` | 0.3 (formatting, macros, std) | UTC timestamps for rotated filenames | `logging` |

Dev dependencies: `tempfile` (filesystem tests), `serial_test` (env-var test serialization), `toml` (integration tests for private-field configs).

No async runtime. No CLI parser.

---

## Design Principles (as implemented)

- **Trait-based extensibility** — `ConfigSource` is the single extension point. Custom sources implement one method and integrate via `with_source()` with zero changes to library code.
- **Unified merge semantics** — All sources produce `ConfigEntry`. Root-level entries deep-merge tables; path-targeted entries create intermediate structure. Registration order determines priority.
- **Deserialization at build time** — The merged TOML table is deserialized once into `T`. After `build()`, config access is a plain struct field reference.
- **Explicit error handling** — No panics in library code. All fallible operations return `Result`. All validation deferred to `entries()` for consistent error flow through `build()`.
- **Compile-time safety** — The AppContext builder uses type-state to enforce that config is provided and async requirements are met. Invalid usage is rejected by the compiler, not at runtime.
- **Minimal dependency surface** — Three always-on crates plus five optional behind the `logging` feature, all widely used and stable.

---

## Known Limitations

- **Resolution operates on TOML intermediate** — Variable references can only target paths that exist in the merged TOML table, not in the final typed struct. References are resolved before deserialization.
- **User-written references cannot traverse arrays** — The internal `get_value`/`get_value_mut` functions support position-indexed array traversal (used for resolving references *inside* array elements), but user-written reference paths like `${arr.0}` go through `lookup_value`, which only navigates tables. Array elements can contain references, but references cannot target array elements.
