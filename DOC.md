# DOC.md - Extracted Documentation

This file contains all documentation comments extracted from the source files.

---

## src/lib.rs

```rust
//! Foundation library providing configuration management and application context.
//!
//! # Example
//!
//! ```no_run
//! use dragon_fnd::{AppContext, Config};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct MyConfig {
//!     name: String,
//!     port: u16,
//! }
//!
//! let ctx = AppContext::builder()
//!     .with_config(
//!         Config::builder()
//!             .with_file("config/default.toml", true)
//!             .with_file("config/local.toml", false)  // optional override
//!             .build::<MyConfig>()?,
//!     )
//!     .build()?;
//!
//! let config = ctx.config();  // &MyConfig, zero-cost
//! # Ok::<(), dragon_fnd::Error>(())
//! ```
//!
//! Configuration files support `${path.to.field}` variable references.
//! See [`Config`] for details.
```

---

## src/error.rs

```rust
/// Top-level error type for the dragon-fnd library.
```

---

## src/config/mod.rs

```rust
//! Configuration loading and management.
```

---

## src/config/source.rs

### Module-level documentation

```rust
//! Core abstractions for configuration sources.
//!
//! This module provides the trait and types that unify all configuration sources
//! (files, environment variables, CLI args, etc.) under a single abstraction.
```

### ConfigEntry struct

```rust
/// A single configuration entry to merge into the config table.
///
/// All configuration sources produce entries in this format, enabling
/// unified merge logic regardless of source type.
```

### ConfigEntry.path field

```rust
/// Path segments to the target location.
/// Empty path means root-level merge (for complete tables like files).
/// Non-empty path like `["database", "host"]` targets nested locations.
```

### ConfigEntry.value field

```rust
/// The value to merge at the target path.
```

### ConfigEntry::root()

```rust
/// Creates a root-level entry (for merging complete tables).
```

### ConfigEntry::at_path()

```rust
/// Creates an entry at a specific path.
```

### ConfigSource trait

```rust
/// A source of configuration entries.
///
/// Implement this trait to create custom configuration sources.
/// The builder collects entries from all sources and merges them
/// in registration order.
///
/// # Example
///
/// ```ignore
/// struct MySource { /* ... */ }
///
/// impl ConfigSource for MySource {
///     fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
///         Ok(vec![
///             ConfigEntry::at_path(
///                 vec!["my".into(), "key".into()],
///                 toml::Value::String("value".into()),
///             ),
///         ])
///     }
/// }
/// ```
```

### ConfigSource::entries()

```rust
/// Produces configuration entries to merge.
///
/// Returns a vector of entries, each specifying a path and value.
/// Entries are applied in order, so later entries override earlier ones.
```

### merge_at_path()

```rust
/// Merges a value at the given path into the table.
///
/// This is the unified merge function that handles all merge scenarios:
/// - Empty path with Table value: deep merge at root level
/// - Non-empty path: navigate/create intermediate tables, then merge or replace
///
/// Deep merging applies to nested tables: keys are merged recursively rather
/// than replaced entirely. Non-table values (including arrays) replace entirely.
```

### deep_merge()

```rust
/// Deep merges an overlay table into a base table.
///
/// For each key in overlay:
/// - If both base and overlay have tables at that key, merge recursively
/// - Otherwise, overlay value replaces base value
```

---

## src/config/builder.rs

### Config struct

```rust
/// Builder for loading configuration from multiple sources.
///
/// Sources are merged in registration order, with later sources overriding
/// earlier ones. Nested tables are merged recursively; other values
/// (including arrays) are replaced entirely.
///
/// ## Variable References
///
/// String values can reference other config values using `${path.to.field}` syntax:
///
/// ```toml
/// [server]
/// host = "localhost"
/// port = 8080
/// url = "http://${server.host}:${server.port}/api"
/// ```
///
/// Use `$$` to escape a literal `$` (e.g., `$${VAR}` becomes `${VAR}`).
///
/// ## Example
///
/// ```no_run
/// use dragon_fnd::Config;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     name: String,
///     port: u16,
/// }
///
/// let config: MyConfig = Config::builder()
///     .with_file("config/default.toml", true)
///     .with_file("config/local.toml", false)
///     .build()?;
/// # Ok::<(), dragon_fnd::ConfigError>(())
/// ```
```

### Config::builder()

```rust
/// Creates a new configuration builder.
```

### Config::with_file()

```rust
/// Adds a TOML file to be loaded.
///
/// If `required` is `true`, the build will fail if the file doesn't exist.
/// Optional files that are missing are silently skipped.
///
/// Sources are applied in registration order, so later sources override earlier ones.
```

### Config::with_env()

```rust
/// Loads configuration from environment variables with the given prefix.
///
/// Environment variables are mapped to config paths by:
/// 1. Removing the prefix and separator
/// 2. Splitting remaining segments on the separator
/// 3. Converting path segments to lowercase
///
/// Values are coerced from strings to the most specific type:
/// integer, float, boolean, or string (fallback).
///
/// Sources are applied in registration order. This allows flexible layering:
///
/// ```no_run
/// # use dragon_fnd::Config;
/// # use serde::Deserialize;
/// # #[derive(Deserialize)] struct MyConfig { }
/// // defaults -> env overrides -> local file overrides env
/// let config: MyConfig = Config::builder()
///     .with_file("config/default.toml", true)
///     .with_env("MYAPP", "__")
///     .with_file("config/local.toml", false)
///     .build()?;
/// # Ok::<(), dragon_fnd::ConfigError>(())
/// ```
///
/// ## Example
///
/// ```no_run
/// use dragon_fnd::Config;
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     database: Database,
/// }
///
/// #[derive(Deserialize)]
/// struct Database {
///     host: String,
///     port: u16,
/// }
///
/// // With MYAPP__DATABASE__HOST=localhost and MYAPP__DATABASE__PORT=5432
/// let config: MyConfig = Config::builder()
///     .with_file("config/default.toml", true)
///     .with_env("MYAPP", "__")
///     .build()?;
/// # Ok::<(), dragon_fnd::ConfigError>(())
/// ```
```

### Config::with_source()

```rust
/// Adds a custom configuration source.
///
/// This enables extension with custom source types (CLI args, remote config, etc.)
/// by implementing the [`ConfigSource`] trait.
///
/// ## Example
///
/// ```ignore
/// use dragon_fnd::config::{ConfigSource, ConfigEntry, ConfigError};
///
/// struct MyCustomSource { /* ... */ }
///
/// impl ConfigSource for MyCustomSource {
///     fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
///         // Return configuration entries
///         Ok(vec![])
///     }
/// }
///
/// let config: MyConfig = Config::builder()
///     .with_file("defaults.toml", true)
///     .with_source(MyCustomSource::new())
///     .build()?;
/// ```
```

### Config::build()

```rust
/// Builds the configuration by loading, merging, resolving, and deserializing.
///
/// This performs deserialization once at build time rather than on each access,
/// making subsequent config reads zero-cost.
```

---

## src/config/file.rs

### Module-level documentation

```rust
//! File-based configuration source.
```

### FileSource struct

```rust
/// A configuration source that loads from a TOML file.
///
/// Files can be marked as required or optional. Required files that don't exist
/// cause an error; optional files that don't exist are silently skipped.
```

### FileSource::new()

```rust
/// Creates a new file source.
///
/// If `required` is true, the build will fail if the file doesn't exist.
```

### load_config_file()

```rust
/// Loads and parses a TOML config file.
///
/// Returns `Ok(None)` if the file doesn't exist and `required` is false.
```

---

## src/config/env.rs

### Module-level documentation

```rust
//! Environment variable configuration source.
```

### EnvSource struct

```rust
/// A configuration source that loads from environment variables.
///
/// Environment variables are mapped to config paths by:
/// 1. Removing the prefix and separator
/// 2. Splitting remaining segments on the separator
/// 3. Converting path segments to lowercase
///
/// For example, with prefix `"APP"` and separator `"__"`:
/// - `APP__DATABASE__HOST=localhost` -> `["database", "host"]` = "localhost"
/// - `APP__SERVER__PORT=8080` -> `["server", "port"]` = 8080
///
/// Values are coerced from strings to the most specific type:
/// - Integer (if all digits with optional leading `-`)
/// - Float (if contains `.` and parses successfully)
/// - Boolean (`true`/`false`, case-insensitive)
/// - String (fallback)
```

### EnvSource::new()

```rust
/// Creates a new environment variable source.
///
/// # Arguments
///
/// * `prefix` - The prefix that identifies relevant env vars (e.g., "MYAPP")
/// * `separator` - The separator between path segments (e.g., "__"). Must not be empty.
///
/// # Panics
///
/// Panics if `separator` is empty.
```

### coerce_value()

```rust
/// Coerces a string value to the most specific TOML type.
```

### looks_like_integer()

```rust
/// Checks if a string looks like an integer (optional minus followed by digits).
```

---

## src/config/resolve.rs

### Module-level documentation

```rust
//! Variable reference resolution for configuration values.
//!
//! Supports `${section.field}` syntax for cross-referencing values within config.
//! Use `$${...}` to escape and produce a literal `${...}`.
```

### resolve_references()

```rust
/// Resolves all `${path.to.field}` references in the configuration table.
///
/// Iteratively resolves references until no more substitutions are made.
/// Returns an error if a circular reference is detected or a referenced path doesn't exist.
```

### resolve_pass()

```rust
/// Performs a single resolution pass over all string values.
/// Returns the number of substitutions made.
```

### resolve_value()

```rust
/// Resolves references in a single value (recursively for tables/arrays).
```

### resolve_string()

```rust
/// Resolves all `${...}` references in a string.
/// Handles `$$` escape sequences.
```

### consume_until()

```rust
/// Consumes characters until the delimiter, returning the collected string.
```

### lookup_path()

```rust
/// Looks up a dotted path in the TOML table and returns the value as a string.
```

### value_to_string()

```rust
/// Converts a TOML value to its string representation.
```

---

## src/config/error.rs

### ConfigError enum

```rust
/// Errors that can occur when loading or parsing configuration.
```

---

## src/context/mod.rs

### Module-level documentation

```rust
//! Application context for managing shared application state.
```

### AppContext struct

```rust
/// Central application context holding configuration and shared resources.
///
/// Generic over the configuration type `C`, which is deserialized once at build time.
/// Access configuration via [`config()`](Self::config) for zero-cost reads.
///
/// ## Example
///
/// ```no_run
/// use dragon_fnd::{AppContext, Config};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     name: String,
///     port: u16,
/// }
///
/// let ctx = AppContext::builder()
///     .with_config(
///         Config::builder()
///             .with_file("config.toml", true)
///             .build::<MyConfig>()?
///     )
///     .build()?;
///
/// let config = ctx.config();  // &MyConfig, zero-cost
/// # Ok::<(), dragon_fnd::Error>(())
/// ```
```

### AppContext::config()

```rust
/// Returns a reference to the configuration.
///
/// This is a zero-cost operation since the config was deserialized at build time.
```

### AppContextBuilder struct

```rust
/// Builder for constructing an [`AppContext`].
///
/// The builder starts with no config (`AppContextBuilder<()>`) and transitions
/// to `AppContextBuilder<C>` when [`with_config`](Self::with_config) is called.
```

### AppContextBuilder::with_config()

```rust
/// Attaches a configuration to the application context.
///
/// The configuration should be the result of [`Config::builder().build()`](crate::Config::build).
```

### AppContextBuilder::build()

```rust
/// Builds the `AppContext`.
///
/// Returns an error if no configuration was provided.
```
