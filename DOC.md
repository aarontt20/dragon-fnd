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

### `ConfigEntry`

A single configuration entry to merge into the config table.

All configuration sources produce entries in this format, enabling
unified merge logic regardless of source type.

**Fields:**

- `path: Vec<String>` - Path segments to the target location.
  Empty path means root-level merge (for complete tables like files).
  Non-empty path like `["database", "host"]` targets nested locations.

- `value: Value` - The value to merge at the target path.

**Methods:**

- `root(table: Table) -> Self` - Creates a root-level entry (for merging complete tables).

- `at_path(path: Vec<String>, value: Value) -> Self` - Creates an entry at a specific path.

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
                toml::Value::String("value".into()),
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
  - The constructor is infallible. An empty separator produces `ConfigError::InvalidSeparator` when `entries()` is called.

### `coerce_value` (private)

Coerces a string value to the most specific TOML type.

### `looks_like_integer` (private)

Checks if a string looks like an integer (optional minus followed by digits).

---

## Module: `config::resolve`

Variable reference resolution for configuration values.

Supports `${section.field}` syntax for cross-referencing values within config.
Use `$$` to produce a literal `$` in any string.

### Resolution Algorithm

Resolution uses a graph-based approach with three phases:

1. **Collection** - Walk the config tree and collect all `${...}` references
   as `(source_path, target_path)` pairs

2. **Topological Sort** - Build a dependency graph and sort references so
   dependencies are resolved before dependents. Circular references are
   detected during this phase via DFS cycle detection.

3. **Resolution** - Process references in topological order. Each reference
   is resolved exactly once, with its dependencies guaranteed to be resolved.

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
- `RootNotTable(String)` - Root-level config entry must be a table
- `CircularReference(Vec<String>)` - Circular reference detected, with cycle path
- `ReferenceNotFound(String)` - Referenced path not found
- `InvalidReferencePath(String)` - Invalid reference path
- `NonScalarReference(String)` - Cannot reference non-scalar value
- `UnclosedReference` - Unclosed reference (missing `}`)
- `InvalidSeparator` - EnvSource separator is empty
- `TypeConflict { path, existing, incoming }` - Non-table value at intermediate path would be replaced by table
- `EmptyPathSegment { var }` - Environment variable produces empty path segment (consecutive separators)

---

## Module: `error`

### `Error`

Top-level error type for the dragon-fnd library.

Variants:
- `Config(ConfigError)` - Configuration error
- `Logging(LoggingError)` - Logging error (feature: `logging`)

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

- `builder() -> AppContextBuilder<NoConfig>` - Creates a new builder for constructing an `AppContext`.

### `AppContextBuilder<Cfg, Async>`

Type-state builder for constructing an `AppContext`.

The builder tracks two type-level dimensions:
- `Cfg`: `NoConfig` → `Configured<C>` (via `with_config()`)
- `Async`: `SyncBuild` (future async subsystems will add `AsyncBuild`)

**Methods:**

- `with_config<C>(self, config: C) -> AppContextBuilder<Configured<C>, A>` - Provides the application configuration. Only available when `Cfg = NoConfig`. Preserves the async type parameter.

- `with_logging(self, builder: LoggingBuilder) -> Self` - Registers a logging configuration to be initialized at build time. Available on all builder states (before or after `with_config()`). Feature: `logging`.

- `build_sync(self) -> Result<AppContext<C>, Error>` - Builds the `AppContext`, initializing all registered subsystems. Only available when config is provided (`Cfg = Configured<C>`) and no async subsystems are registered (`Async = SyncBuild`). Returns an error if any subsystem fails to initialize.

---

## Module: `logging` (feature: `logging`)

Structured logging via `tracing` with console and file outputs.

### `LoggingConfig`

Serde-deserializable logging configuration. Top-level fields:

- `enabled: bool` (default: `true`) — master switch
- `filter: String` (default: `"info"`) — base EnvFilter directive
- `modules: BTreeMap<String, String>` (default: `{}`) — per-module overrides
- `console: ConsoleConfig` — console output settings
- `file: FileConfig` — file output settings

### `ConsoleConfig`

- `enabled: bool` (default: `true`)
- `format: LogFormat` (default: `Pretty`)
- `filter: Option<String>` — optional per-layer filter override

### `FileConfig`

- `enabled: bool` (default: `false`)
- `dir: PathBuf` (default: `"./logs"`)
- `prefix: String` (default: `"app"`)
- `format: LogFormat` (default: `Json`)
- `rotation: Rotation` (default: `Daily`)
- `filter: Option<String>` — optional per-layer filter override
- `max_bytes: Option<u64>` — size-based rotation threshold (minimum 4096); cannot combine with time-based rotation
- `compress: bool` (default: `false`) — gzip rotated files in background thread; requires `max_bytes` (time-based rotation compression is not yet supported)
- `retain_days: Option<u32>` — delete files older than N days
- `retain_files: Option<u32>` — keep only N most recent files

`retain_days` and `retain_files` are mutually exclusive.

### `LogFormat`

`Pretty` | `Json` | `Compact`

### `Rotation`

`Daily` | `Hourly` | `Never`

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
- `rotation(self, rotation: Rotation) -> Self`
- `max_bytes(self, bytes: u64) -> Self` — size-based rotation threshold (minimum 4096)
- `compress(self, compress: bool) -> Self` — gzip rotated files in background thread
- `retain_days(self, days: u32) -> Self` — clears retain_files
- `retain_files(self, count: u32) -> Self` — clears retain_days

### `LoggingError`

Errors that can occur when initializing the logging subsystem.

Variants:
- `InvalidFilter(String)` — malformed EnvFilter directive
- `InvalidRotation(String)` — invalid rotation config (max_bytes too small, combining max_bytes with time rotation, compress without rotation)
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
rotation = "never"
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
use dragon_fnd::logging::{LoggingBuilder, ConsoleBuilder, FileBuilder, LogFormat, Rotation};

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
                    .rotation(Rotation::Daily)
                    .retain_days(14),
            ),
    )
    .with_config(config)
    .build_sync()?;
```
