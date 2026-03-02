# dragon-fnd API Documentation

Foundation library providing configuration management and application context.

## Quick Example

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use serde::Deserialize;

#[derive(Deserialize)]
struct MyConfig {
    name: String,
    port: u16,
}

let ctx = AppContext::builder()
    .with_config(
        ConfigBuilder::new()
            .with_file("config/default.toml", true)
            .with_file("config/local.toml", false)  // optional override
            .build::<MyConfig>()?,
    )
    .build_sync()?;

let config = ctx.config();  // &MyConfig, zero-cost
```

Configuration files support `${path.to.field}` variable references.

---

## Module: `config`

Configuration loading and management.

### `ConfigValue`

Library-owned value type for configuration data. Replaces `toml::Value` in the public API.

**Variants:**

- `String(String)` — string value
- `Integer(i64)` — integer value
- `Float(f64)` — floating-point value
- `Boolean(bool)` — boolean value
- `Datetime(String)` — datetime as string (parsed to TOML datetime internally)
- `Array(Vec<ConfigValue>)` — array of values
- `Table(ConfigTable)` — key-value table

**Constructors:**

- `string(s: impl Into<String>) -> Self`
- `integer(i: i64) -> Self`
- `float(f: f64) -> Self`
- `boolean(b: bool) -> Self`
- `datetime(s: impl Into<String>) -> Result<Self, ConfigError>` — validates the string is a valid TOML datetime

### `ConfigTable`

Newtype over `BTreeMap<String, ConfigValue>`.

**Methods:**

- `new() -> Self` — empty table
- `insert(self, key: impl Into<String>, value: ConfigValue) -> Self` — insert a key-value pair (consuming builder pattern)
- `get(key: &str) -> Option<&ConfigValue>` — look up a value by key
- `iter() -> impl Iterator<Item = (&String, &ConfigValue)>` — iterate over entries
- `len() -> usize` — number of entries
- `is_empty() -> bool` — true if no entries
- Implements `IntoIterator` for consuming iteration

### `ConfigEntry`

A single configuration entry to merge into the config table.

All configuration sources produce entries in this format, enabling
unified merge logic regardless of source type.

**Fields:**

- `path: Vec<String>` - Path segments to the target location.
  Empty path means root-level merge (for complete tables like files).
  Non-empty path like `["database", "host"]` targets nested locations.

- `value: ConfigValue` - The value to merge at the target path.

**Methods:**

- `root(table: Table) -> Self` - Creates a root-level entry (for merging complete tables).

- `at_path(path: Vec<String>, value: ConfigValue) -> Self` - Creates an entry at a specific path.

### `ConfigSource` (trait)

A source of configuration entries.

Implement this trait to create custom configuration sources.
The builder collects entries from all sources and merges them
in registration order.

```rust
struct MySource { /* ... */ }

impl ConfigSource for MySource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        Ok(vec![
            ConfigEntry::at_path(
                vec!["my".into(), "key".into()],
                ConfigValue::string("value"),
            ),
        ])
    }
}
```

**Methods:**

- `entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>` - Produces configuration entries to merge. Returns a vector of entries, each specifying a path and value. Entries are applied in order, so later entries override earlier ones.

---

## Module: `config::builder`

### `ConfigBuilder`

Builder for loading configuration from multiple sources.

Sources are merged in registration order, with later sources overriding
earlier ones. Nested tables are merged recursively; other values
(including arrays) are replaced entirely.

#### Variable References

String values can reference other config values using `${path.to.field}` syntax.

**String Interpolation** - References embedded in text are converted to strings:

```toml
[server]
host = "localhost"
port = 8080
url = "http://${server.host}:${server.port}/api"
# Result: url = "http://localhost:8080/api"
```

**Full Value Substitution** - A string containing *only* a reference (`"${path}"`)
preserves the original value's type:

```toml
[defaults]
port = 8080
tags = ["api", "v1"]
timeout = { connect = 5, read = 30 }

[server]
port = "${defaults.port}"           # → integer 8080
tags = "${defaults.tags}"           # → array ["api", "v1"]
timeout = "${defaults.timeout}"     # → table { connect = 5, read = 30 }
```

**Escape Sequences** - Use `$$` to produce a literal `$` when the string also
contains references:

```toml
amount = 50
price = "$$${amount} USD"  # → "$50 USD"
```

Note: `$$` is processed in all strings — both those with `${...}` references and those
with only `$$` escapes.

**Resolution Order** - References are resolved using topological sort, so
dependencies are always resolved before dependents. This allows chained references:

```toml
a = "base"
b = "${a}-middle"    # → "base-middle"
c = "${b}-end"       # → "base-middle-end"
```

**Errors**:
- Circular references (e.g., `a = "${b}"`, `b = "${a}"`) are detected and reported
- Missing references return `ReferenceNotFound`
- Unclosed references (`"${missing_brace"`) return `UnclosedReference`
- Referencing arrays/tables in string interpolation returns `NonScalarReference`

#### Example

```rust
use dragon_fnd::ConfigBuilder;
use serde::Deserialize;

#[derive(Deserialize)]
struct MyConfig {
    name: String,
    port: u16,
}

let config: MyConfig = ConfigBuilder::new()
    .with_file("config/default.toml", true)
    .with_file("config/local.toml", false)
    .build()?;
```

**Methods:**

- `new() -> Self` - Creates a new configuration builder.

- `with_file(self, path: impl AsRef<Path>, required: bool) -> Self` - Adds a TOML file to be loaded. If `required` is `true`, the build will fail if the file doesn't exist. Optional files that are missing are silently skipped. Sources are applied in registration order, so later sources override earlier ones.

- `with_env(self, prefix: impl Into<String>, separator: impl Into<String>) -> Self` - Loads configuration from environment variables with the given prefix.

  Environment variables are mapped to config paths by:
  1. Removing the prefix and separator
  2. Splitting remaining segments on the separator
  3. Converting path segments to lowercase

  Values are coerced from strings to the most specific type:
  integer, float, boolean, or string (fallback).

  Sources are applied in registration order. This allows flexible layering:

  ```rust
  // defaults -> env overrides -> local file overrides env
  let config: MyConfig = ConfigBuilder::new()
      .with_file("config/default.toml", true)
      .with_env("MYAPP", "__")
      .with_file("config/local.toml", false)
      .build()?;
  ```

  Example with nested config:

  ```rust
  use dragon_fnd::ConfigBuilder;
  use serde::Deserialize;

  #[derive(Deserialize)]
  struct MyConfig {
      database: Database,
  }

  #[derive(Deserialize)]
  struct Database {
      host: String,
      port: u16,
  }

  // With MYAPP__DATABASE__HOST=localhost and MYAPP__DATABASE__PORT=5432
  let config: MyConfig = ConfigBuilder::new()
      .with_file("config/default.toml", true)
      .with_env("MYAPP", "__")
      .build()?;
  ```

- `with_source(mut self, source: impl ConfigSource + 'static) -> Self` - Adds a custom configuration source. This enables extension with custom source types (CLI args, remote config, etc.) by implementing the `ConfigSource` trait.

  ```rust
  use dragon_fnd::{ConfigSource, ConfigEntry, ConfigError};

  struct MyCustomSource { /* ... */ }

  impl ConfigSource for MyCustomSource {
      fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
          // Return configuration entries
          Ok(vec![])
      }
  }

  let config: MyConfig = ConfigBuilder::new()
      .with_file("defaults.toml", true)
      .with_source(MyCustomSource::new())
      .build()?;
  ```

- `build<T: DeserializeOwned>(self) -> Result<T, ConfigError>` - Builds the configuration by loading, merging, resolving, and deserializing. This performs deserialization once at build time rather than on each access, making subsequent config reads zero-cost.

---

## Module: `config::file`

File-based configuration source.

### `FileSource`

A configuration source that loads from a TOML file.

Files can be marked as required or optional. Required files that don't exist
cause an error; optional files that don't exist are silently skipped.

**Methods:**

- `new(path: impl AsRef<Path>, required: bool) -> Self` - Creates a new file source. If `required` is true, the build will fail if the file doesn't exist.

### `load_config_file` (private)

Loads and parses a TOML config file.

Returns `Ok(None)` if the file doesn't exist and `required` is false.

---

## Module: `config::env`

Environment variable configuration source.

### `EnvSource`

A configuration source that loads from environment variables.

Environment variables are mapped to config paths by:
1. Removing the prefix and separator
2. Splitting remaining segments on the separator
3. Converting path segments to lowercase

For example, with prefix `"APP"` and separator `"__"`:
- `APP__DATABASE__HOST=localhost` -> `["database", "host"]` = "localhost"
- `APP__SERVER__PORT=8080` -> `["server", "port"]` = 8080

Values are coerced from strings to the most specific type:
- Boolean (`true`/`false`, case-insensitive)
- Integer (if all digits with optional leading `-`, no leading zeros)
- Float (if contains `.` and parses successfully)
- String (fallback)

**Methods:**

- `new(prefix: impl Into<String>, separator: impl Into<String>) -> Self` - Creates a new environment variable source.
  - `prefix` - The prefix that identifies relevant env vars (e.g., "MYAPP")
  - `separator` - The separator between path segments (e.g., "__"). Must not be empty.
  - The constructor is infallible. An empty prefix produces `ConfigError::InvalidPrefix` and an empty separator produces `ConfigError::InvalidSeparator` when `entries()` is called.

### `coerce_value` (private)

Coerces a string value to the most specific TOML type.

### `looks_like_integer` (private)

Checks if a string looks like an integer (optional minus followed by digits).

---

## Module: `config::serde_source`

Struct-to-config adapter.

### `SerdeSource`

A `ConfigSource` that serializes any `T: Serialize` into the config pipeline. Most commonly used to feed parsed CLI arguments into `ConfigBuilder`, but works with any serializable struct.

`Option::None` fields are omitted from the resulting table, so they do not override values from lower-priority sources. For maximum robustness, annotate Option fields with `#[serde(skip_serializing_if = "Option::is_none")]`.

```rust
use dragon_fnd::{ConfigBuilder, SerdeSource};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Args {
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verbose: Option<bool>,
}

#[derive(Deserialize)]
struct MyConfig {
    port: u16,
    verbose: bool,
}

let args = Args { port: Some(9090), verbose: None };

let config: MyConfig = ConfigBuilder::new()
    .with_file("config/default.toml", true)
    .with_source(SerdeSource::new(&args)?)  // registered last = highest priority
    .build()?;
```

**Methods:**

- `new<T: Serialize>(value: &T) -> Result<Self, ConfigError>` - Serializes `value` into a TOML table at construction time. Takes `&T` so the caller retains ownership. Returns `Err(ConfigError::SerializeError)` if the value cannot be represented as a TOML table (e.g., bare scalars, `u64` exceeding `i64::MAX`).

**Limitations:**

Values take a roundtrip through TOML's type system. Types that cannot be represented as a TOML table produce `ConfigError::SerializeError`. Serde field names in the serialized output must match the TOML keys they are intended to override — `#[serde(rename)]` changes the serialized key.

---

## Module: `config::resolve`

Variable reference resolution for configuration values.

Supports `${section.field}` syntax for cross-referencing values within config.
Use `$$` to produce a literal `$` in any string.

### Resolution Algorithm

Resolution uses a graph-based approach with four phases:

1. **Collection** - Walk the config tree and collect all `${...}` references
   as `(source_path, target_path)` pairs

2. **Topological Sort** - Build a dependency graph and sort references so
   dependencies are resolved before dependents. Circular references are
   detected during this phase via DFS cycle detection.

3. **Resolution** - Process references in topological order. Each reference
   is resolved exactly once, with its dependencies guaranteed to be resolved.

4. **Escape Processing** - Strings containing `$$` but no `${...}` references
   have their `$$` sequences replaced with literal `$`.

This approach is O(n) compared to iterative resolution which could be O(n × depth).

### `resolve_references`

```rust
pub fn resolve_references(table: &mut Table) -> Result<(), ConfigError>
```

Resolves all `${path.to.field}` references in the configuration table.

**Behavior:**
- Pure references (`"${path}"` with nothing else) perform full value substitution,
  preserving the original type (integer, boolean, array, table, etc.)
- Embedded references (`"prefix${path}suffix"`) perform string interpolation,
  converting referenced values to strings
- Circular references are detected and return `ConfigError::CircularReference`
- Missing references return `ConfigError::ReferenceNotFound`
- Unclosed references (`${missing_brace`) return `ConfigError::UnclosedReference`

### Key Functions (private)

- `topological_sort` - Orders references by dependency, detects cycles
- `resolve_at_path` - Resolves references at a specific config path
- `is_pure_reference` - Checks if string is exactly `${path}` (full substitution)
- `resolve_string` - Resolves references in a string value
- `lookup_value` - Navigates dotted path to find referenced value
- `value_to_string` - Converts value to string for interpolation

---

## Module: `config::error`

### `ConfigError`

Errors that can occur when loading or parsing configuration.

Variants:
- `FileNotFound(PathBuf)` - Required config file not found
- `ReadError { path, source }` - Failed to read config file
- `ParseError { path, source }` - Failed to parse config file (manually constructed with file path context)
- `DeserializeError(toml::de::Error)` - Failed to deserialize config (no `#[from]`)
- `SerializeError(toml::ser::Error)` - Failed to serialize value to TOML table (no `#[from]`)
- `RootNotTable(String)` - Root-level config entry must be a table
- `CircularReference(Vec<String>)` - Circular reference detected, with cycle path
- `ReferenceNotFound(String)` - Referenced path not found
- `InvalidReferencePath(String)` - Invalid reference path
- `NonScalarReference(String)` - Cannot reference non-scalar value
- `UnclosedReference` - Unclosed reference (missing `}`)
- `InvalidSeparator` - EnvSource separator is empty
- `InvalidPrefix` - EnvSource prefix is empty
- `InvalidDatetime(String)` - Invalid datetime string passed to `ConfigValue::datetime()`
- `TypeConflict { path, existing, incoming }` - Non-table value at intermediate path would be replaced by table
- `EmptyPathSegment { var }` - Environment variable produces empty path segment (consecutive separators)

---

## Module: `error`

### `Error`

Top-level error type for the dragon-fnd library.

Variants:
- `Config(ConfigError)` - Configuration error
- `Logging(LoggingError)` - Logging error (feature: `logging`)
- `Sqlite(SqliteError)` - SQLite error (feature: `sqlite`)

---

## Module: `context`

Application context for managing shared application state.

### `AppContext<C>`

Central application context holding configuration and subsystem handles.

Generic over the configuration type `C`, which is deserialized once at build time.
Access configuration via `config()` for zero-cost reads.

#### Example

```rust
use dragon_fnd::{AppContext, ConfigBuilder};
use serde::Deserialize;

#[derive(Deserialize)]
struct MyConfig {
    name: String,
    port: u16,
}

let ctx = AppContext::builder()
    .with_config(
        ConfigBuilder::new()
            .with_file("config.toml", true)
            .build::<MyConfig>()?
    )
    .build_sync()?;

let config = ctx.config();  // &MyConfig, zero-cost
```

**Methods:**

- `config(&self) -> &C` - Returns a reference to the configuration. This is a zero-cost operation since the config was deserialized at build time.

- `extension<T: Send + Sync + 'static>(&self) -> Option<&T>` - Returns a reference to an extension of type `T`, if one was registered via `with_extension()`.

- `sqlite(&self) -> Option<&SqlitePool>` - Returns a reference to the SQLite connection pool, if one was registered via `with_sqlite()`. Feature: `sqlite`.

- `builder() -> AppContextBuilder<NoConfig>` - Creates a new builder for constructing an `AppContext`.

### `AppContextBuilder<Cfg, Async>`

Type-state builder for constructing an `AppContext`.

The builder tracks two type-level dimensions:
- `Cfg`: `NoConfig` → `Configured<C>` (via `with_config()`)
- `Async`: `SyncBuild` → `AsyncBuild` (via `with_sqlite()` or future async subsystems)

**Methods:**

- `with_config<C>(self, config: C) -> AppContextBuilder<Configured<C>, A>` - Provides the application configuration. Only available when `Cfg = NoConfig`. Preserves the async type parameter.

- `with_extension<T: Send + Sync + 'static>(self, ext: T) -> Self` - Stores an extension value, retrievable later via `AppContext::extension()`. Available on all builder states. If the same type is registered twice, the last value wins.

- `with_logging(self, builder: LoggingBuilder) -> Self` - Registers a logging configuration to be initialized at build time. Available on all builder states (before or after `with_config()`). Feature: `logging`.

- `with_sqlite(self, builder: SqliteBuilder) -> AppContextBuilder<Cfg, AsyncBuild>` - Registers a SQLite database pool to be initialized at build time. Transitions the builder to `AsyncBuild` — `build_sync()` is no longer available. Feature: `sqlite`.

- `build_sync(self) -> Result<AppContext<C>, Error>` - Builds the `AppContext`, initializing all registered subsystems. Only available when config is provided (`Cfg = Configured<C>`) and no async subsystems are registered (`Async = SyncBuild`). Returns an error if any subsystem fails to initialize.

- `async build(self) -> Result<AppContext<C>, Error>` - Builds the `AppContext`, initializing all registered subsystems asynchronously. Only available when config is provided and the builder is in `AsyncBuild` state (e.g., after `with_sqlite()`). Returns an error if any subsystem fails to initialize. Feature: `sqlite`.

---

## Module: `logging` (feature: `logging`)

Structured logging via `tracing` with console and file outputs.

### `LoggingConfig`

Serde-deserializable logging configuration. All fields are `pub(crate)` — accessed through `LoggingBuilder` or deserialized from TOML.

- `enabled: bool` (default: `true`) — master switch; accessible via `enabled()` getter
- `filter: String` (default: `"info"`) — base EnvFilter directive
- `modules: BTreeMap<String, String>` (default: `{}`) — per-module overrides
- `console: ConsoleConfig` — console output settings
- `file: FileConfig` — file output settings

### `ConsoleConfig`

All fields `pub(crate)`.

- `enabled: bool` (default: `true`)
- `format: LogFormat` (default: `Pretty`)
- `filter: Option<String>` — optional per-layer filter override

### `FileConfig`

All fields are `pub(crate)` — configured through builders or deserialized from TOML. Uses custom `Deserialize` impl.

- `enabled: bool` (default: `false`)
- `dir: PathBuf` (default: `"./logs"`)
- `prefix: String` (default: `"app"`)
- `format: LogFormat` (default: `Json`)
- `rotation: RotationStrategy` (default: `Daily`)
- `filter: Option<String>` — optional per-layer filter override
- `compress: bool` (default: `false`) — gzip rotated files in background thread; requires rotation (not `Never`)
- `retain_days: Option<u32>` — delete files older than N days
- `retain_files: Option<u32>` — keep only N most recent files

`retain_days` and `retain_files` are mutually exclusive.

### `LogFormat`

`Pretty` | `Json` | `Compact`

### `RotationStrategy`

`Daily` | `Hourly` | `SizeBased { max_bytes: u64 }` | `Never`

In TOML, `rotation = "size"` with `max_bytes = 10485760`. The `SizeBased` variant carries its `max_bytes` threshold (minimum 4096), making it structurally impossible to combine size-based settings with time-based rotation.

### `LoggingBuilder`

Fluent builder for logging configuration. Wraps `LoggingConfig` internally.

**Methods:**

- `new() -> Self` — defaults: console enabled at info, file disabled
- `from_config(config: LoggingConfig) -> Self` — bridge from deserialized config (takes ownership)
- `filter(self, filter: impl Into<String>) -> Self` — base filter directive
- `module(self, module: impl Into<String>, level: impl Into<String>) -> Self` — per-module override
- `enabled(self, enabled: bool) -> Self` — master switch
- `console(self, console: ConsoleBuilder) -> Self` — configure console output
- `file(self, file: FileBuilder) -> Self` — configure file output

### `ConsoleBuilder`

- `new() -> Self` — defaults: enabled, pretty format
- `format(self, format: LogFormat) -> Self`
- `filter(self, filter: impl Into<String>) -> Self`
- `enabled(self, enabled: bool) -> Self`

### `FileBuilder`

- `new(dir: impl Into<PathBuf>) -> Self` — auto-enables file output
- `prefix(self, prefix: impl Into<String>) -> Self`
- `format(self, format: LogFormat) -> Self`
- `filter(self, filter: impl Into<String>) -> Self`
- `rotation(self, rotation: RotationStrategy) -> Self`
- `compress(self, compress: bool) -> Self` — gzip rotated files in background thread
- `retain_days(self, days: u32) -> Self` — clears retain_files
- `retain_files(self, count: u32) -> Self` — clears retain_days

### `LoggingError`

Errors that can occur when initializing the logging subsystem.

Variants:
- `InvalidFilter(String)` — malformed EnvFilter directive
- `InvalidRotation(String)` — invalid rotation config (SizeBased max_bytes too small, compress without rotation)
- `InvalidRetention(String)` — invalid retention config (mutual exclusion, zero values, Never without max_bytes + retention)
- `FileSetupFailed { dir, source }` — could not create log directory
- `SubscriberAlreadySet` — global tracing subscriber already set; logging configuration was not applied

### Example: From Config File

```toml
[logging]
filter = "info"
modules = { sqlx = "warn" }

[logging.console]
format = "pretty"

[logging.file]
enabled = true
dir = "./logs"
prefix = "myapp"
rotation = "size"
max_bytes = 10485760  # 10 MB size-based rotation
compress = true
retain_files = 5
```

```rust
use dragon_fnd::logging::LoggingBuilder;

let ctx = AppContext::builder()
    .with_logging(LoggingBuilder::from_config(config.logging))
    .with_config(config)
    .build_sync()?;
```

### Example: Programmatic Builder

```rust
use dragon_fnd::logging::{LoggingBuilder, ConsoleBuilder, FileBuilder, LogFormat, RotationStrategy};

let ctx = AppContext::builder()
    .with_logging(
        LoggingBuilder::new()
            .filter("debug")
            .module("sqlx", "warn")
            .console(ConsoleBuilder::new().format(LogFormat::Pretty))
            .file(
                FileBuilder::new("./logs")
                    .prefix("myapp")
                    .format(LogFormat::Json)
                    .rotation(RotationStrategy::Daily)
                    .retain_days(14),
            ),
    )
    .with_config(config)
    .build_sync()?;
```

---

## Module: `sqlite` (feature: `sqlite`)

SQLite database pool initialization and lifecycle via `sqlx`.

### `SqliteConfig`

Serde-deserializable SQLite configuration. All fields are `pub(crate)` — accessed through `SqliteBuilder` or deserialized from TOML.

- `path: String` (default: `""`) — database file path, or `":memory:"` for in-memory. Empty string errors at init.
- `max_connections: u32` (default: `5`)
- `min_connections: u32` (default: `1`)
- `acquire_timeout_secs: u64` (default: `10`)
- `idle_timeout_secs: u64` (default: `300`)
- `migrate: bool` (default: `false`) — run filesystem-based migrations at init
- `migrations_dir: PathBuf` (default: `"./migrations"`)
- `journal_mode: JournalMode` (default: `Wal`)
- `foreign_keys: bool` (default: `true`)
- `busy_timeout_secs: u64` (default: `5`) — how long SQLite waits when the database is locked

### `JournalMode`

`Wal` | `Delete` | `Memory`

In TOML: `journal_mode = "wal"`, `"delete"`, or `"memory"`.

### `SqliteBuilder`

Fluent builder for SQLite configuration. Wraps `SqliteConfig` internally.

**Methods:**

- `new(path: impl Into<String>) -> Self` — creates a builder with the given database path and defaults
- `from_config(config: SqliteConfig) -> Self` — bridge from deserialized config (takes ownership)
- `migrate(self, enable: bool) -> Self` — enable/disable runtime migrations
- `migrations_dir(self, dir: impl Into<PathBuf>) -> Self` — path to migrations directory
- `max_connections(self, n: u32) -> Self`
- `min_connections(self, n: u32) -> Self`
- `acquire_timeout_secs(self, secs: u64) -> Self`
- `idle_timeout_secs(self, secs: u64) -> Self`
- `journal_mode(self, mode: JournalMode) -> Self`
- `foreign_keys(self, enable: bool) -> Self`
- `busy_timeout_secs(self, secs: u64) -> Self`

### `SqlitePool`

Re-exported from `sqlx::SqlitePool` so users can reference it without depending on sqlx directly.

### `SqliteError`

Errors that can occur when initializing the SQLite subsystem.

Variants:
- `EmptyPath` — database path is empty
- `DirectoryCreationFailed { dir, source }` — could not create parent directory
- `PoolCreationFailed { source }` — pool creation or connection options failed
- `ConnectivityTestFailed { source }` — `SELECT 1` test query failed
- `MigrationsDirNotFound(PathBuf)` — migrations enabled but directory missing
- `MigrationFailed { source }` — migration execution failed

### Example: From Config File

```toml
[sqlite]
path = "data/app.db"
migrate = true
migrations_dir = "./migrations"
journal_mode = "wal"
foreign_keys = true
busy_timeout_secs = 10
```

```rust
use dragon_fnd::sqlite::SqliteBuilder;

let ctx = AppContext::builder()
    .with_sqlite(SqliteBuilder::from_config(config.sqlite.clone()))
    .with_config(config)
    .build()
    .await?;

let pool = ctx.sqlite().expect("sqlite was registered");
```

### Example: Programmatic Builder

```rust
use dragon_fnd::sqlite::{SqliteBuilder, JournalMode};

let ctx = AppContext::builder()
    .with_sqlite(
        SqliteBuilder::new("data/app.db")
            .migrate(true)
            .migrations_dir("./migrations")
            .journal_mode(JournalMode::Wal)
            .max_connections(10)
            .busy_timeout_secs(10),
    )
    .with_config(config)
    .build()
    .await?;
```
