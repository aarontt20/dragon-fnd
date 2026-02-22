# DESIGN.md — How the Built System Works

This document describes dragon-fnd as it exists in code today. For future plans and the full vision, see [VISION.md](VISION.md).

---

## What It Does

dragon-fnd is a foundation library for Rust applications that provides typed configuration loading from multiple sources (TOML files, environment variables, custom sources) with deep merge semantics, graph-based variable reference resolution, and a type-state application context builder. It ships as a single crate with three dependencies (serde, toml, thiserror) and a `[features]` section for future subsystem opt-in (no features are currently active). The config subsystem is the only built component. The library does not own `main()` and does not prescribe application structure.

---

## Module Structure

```
src/
├── lib.rs                 # Crate root, re-exports public API
├── error.rs               # Top-level Error enum (1 variant)
├── config/
│   ├── mod.rs             # Public exports: ConfigBuilder, ConfigError, ConfigSource, ConfigEntry
│   ├── source.rs          # ConfigSource trait, ConfigEntry, merge_at_path, deep_merge
│   ├── builder.rs         # ConfigBuilder: fluent API, generic build::<T>()
│   ├── file.rs            # FileSource: loads TOML files
│   ├── env.rs             # EnvSource: loads environment variables
│   ├── resolve.rs         # Graph-based ${path.to.field} variable resolution
│   └── error.rs           # ConfigError enum (10 variants)
└── context/
    └── mod.rs             # AppContext<C> with type-state AppContextBuilder
```

10 source files. Tests are inline (`#[cfg(test)]` modules) plus integration tests in `tests/`.

---

## Config System

### ConfigSource Trait and ConfigEntry

All configuration sources implement one trait (`src/config/source.rs`):

```rust
pub trait ConfigSource: Send + Sync + std::fmt::Debug {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>;
}
```

Sources produce `ConfigEntry` values — each pairing a path with a TOML value:

```rust
pub struct ConfigEntry {
    pub path: Vec<String>,
    pub value: toml::Value,
}
```

Two constructors serve different use cases:
- `ConfigEntry::root(table)` — empty path, for sources that produce a complete table (files)
- `ConfigEntry::at_path(path, value)` — specific path segments, for sources that produce individual values (env vars)

`merge_at_path()` handles all merge scenarios with a `let-else` pattern for the empty-path case:
1. **Empty path + table value**: deep merge at root level — nested tables merge recursively, non-table values replace
2. **Non-empty path, final key**: if both existing and incoming are tables, deep merge; otherwise replace entirely
3. **Non-empty path, intermediate segments**: navigate to the target, auto-creating missing intermediate tables

`deep_merge()` is the recursive helper: for each key in the overlay, if both sides have tables at that key, recurse; otherwise the overlay value replaces the base value.

### FileSource

`src/config/file.rs` — a TOML file loader. Wraps a `PathBuf` and a `required: bool` flag.

- Produces a single `ConfigEntry::root(table)` by reading and parsing the file
- When `required` is false and the file is missing, returns an empty entries vec (no error)
- When `required` is true and the file is missing, returns `ConfigError::FileNotFound`
- I/O failures return `ConfigError::ReadError`; invalid TOML returns `ConfigError::ParseError`

### EnvSource

`src/config/env.rs` — loads configuration from environment variables. Configured with a `prefix` and `separator`.

The constructor is infallible — validation is deferred to `entries()`, consistent with how `FileSource` validates at `entries()` time. An empty separator produces `ConfigError::InvalidSeparator`.

Scanning logic:
1. For each env var, check if the key starts with `{prefix}{separator}`
2. Strip the prefix and separator
3. Split the remainder on the separator to produce path segments
4. Lowercase each segment

Produces one `ConfigEntry::at_path(path, coerced_value)` per matching variable.

Value coercion applies in order:
1. **Boolean** — case-insensitive `true`/`false`
2. **Integer** — optional leading `-` followed by ASCII digits only (no scientific notation, no hex)
3. **Float** — must contain `.` and parse as `f64`
4. **String** — fallback for everything else

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

`src/config/resolve.rs` — the largest module (~275 lines). Uses a graph-based approach with three phases:

**Phase 1 — Collection.** Walk the entire merged table. For each string value, parse `${path.to.field}` references. Collect them as `(source_path, target_path)` edge pairs. `$$` is recognized as an escape (skipped during collection). Unclosed `${` produces `ConfigError::UnclosedReference`. References inside array elements are collected with position-indexed paths (e.g., `["arr", "0"]`), so a string at `arr[0]` that contains `${some.ref}` is tracked and resolved correctly. Note: user-written reference paths like `${arr.0}` cannot traverse into arrays — `lookup_value` only navigates tables.

**Phase 2 — Topological sort.** Build a dependency graph from the collected edges. Sort via DFS with an `in_progress` set for cycle detection. If a node is visited while already in-progress, the reference chain is circular → `ConfigError::CircularReference`. The result is a resolution order where dependencies come before dependents.

**Phase 3 — Resolution.** Process each path in topological order. Two modes:

- **Pure reference** — the string is exactly `${path}` (no whitespace, single reference, nothing else). The entire value is replaced with the target value's type — integers, booleans, arrays, and tables pass through intact. This enables type-preserving references. Whitespace around the reference (e.g., `"  ${path}  "`) is treated as string interpolation, not pure substitution.
- **String interpolation** — the string contains `${path}` embedded among other text. Each reference is replaced with the string representation of the target value. Scalars (string, integer, float, boolean, datetime) convert naturally. Non-scalar targets (arrays, tables) produce `ConfigError::NonScalarReference`.

Escape sequences: `$$` becomes literal `$` during resolution, but only in strings that also contain actual `${...}` references. Strings without references are not processed.

The `get_value` and `get_value_mut` functions are structurally identical — duplicated because Rust's borrow checker requires separate shared and mutable traversal paths.

---

## Error Types

### ConfigError

`src/config/error.rs` — 10 variants, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `FileNotFound(PathBuf)` | Required config file missing |
| `ReadError { path, source }` | I/O failure reading a file |
| `ParseError { path, source }` | Invalid TOML syntax |
| `DeserializeError(toml::de::Error)` | Final deserialization into target type failed |
| `CircularReference` | Cycle detected in variable reference graph |
| `ReferenceNotFound(String)` | `${path}` points to a nonexistent key |
| `InvalidReferencePath(String)` | Empty or malformed reference path (e.g., `${a..b}`) |
| `NonScalarReference(String)` | Embedded reference targets a table or array |
| `UnclosedReference` | Missing closing `}` in `${...}` |
| `InvalidSeparator` | EnvSource separator is empty |

### Top-level Error

`src/error.rs` — 1 variant, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `Config(ConfigError)` | Wraps any config error (with `#[from]` for `?` conversion) |

The enum has a single variant because logging (the next subsystem) will add `Error::Logging(LoggingError)`. Keeping `Error` as an enum avoids collapsing and re-expanding when that lands.

---

## AppContext

`src/context/mod.rs` — a generic container for application state, built with a type-state pattern.

```rust
pub struct AppContext<C> {
    config: C,
    // Future feature-gated fields added here
}
```

`AppContext::config()` returns `&C` — zero-cost after build.

### Type-State Builder

The builder uses type-state to enforce correct construction at compile time:

```rust
pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
}
```

Two type-level dimensions:

**Config presence** — `NoConfig` vs `Configured<C>`:
- `AppContext::builder()` returns `AppContextBuilder<NoConfig, SyncBuild>`
- `.with_config(config)` transitions to `AppContextBuilder<Configured<C>, SyncBuild>`
- `build_sync()` only exists on `Configured<C>` — calling it without config is a compile error

**Async requirements** — `SyncBuild` (only marker defined currently):
- `build_sync()` only exists on `SyncBuild` — future async subsystems will transition to `AsyncBuild`, removing `build_sync()` and requiring `build().await` instead
- `PhantomData<Async>` reserves the parameter with no layout cost

`build_sync()` is infallible — it returns `AppContext<C>`, not `Result`. The type-state proves config is present and no async subsystems are registered. If nothing can fail, the return type says so.

Marker types (`NoConfig`, `Configured<C>`, `SyncBuild`) are `pub` with `#[doc(hidden)]` — nameable for compiler error messages, hidden from generated docs. Standard Rust ecosystem convention for type-state markers.

`AppContext` uses a manual `Debug` impl rather than `#[derive(Debug)]` — `WorkerGuard` (arriving with the logging subsystem) does not implement `Debug`, so the derive would break.

A `compile_fail` doc-test verifies that `AppContext::builder().build_sync()` does not compile (no config provided).

### Teardown Contract

`AppContext` follows two teardown strategies depending on the subsystem:

- **Sync handles** (e.g., `WorkerGuard` for logging): cleaned up via `Drop`. Rust drops struct fields in declaration order — fields are declared so that dependent handles drop before their dependencies.
- **Async handles** (e.g., db pool, HTTP server): will require an explicit `ctx.shutdown().await` method, implemented when the shutdown subsystem lands. `Drop` cannot `await`.

---

## Dependencies

Three crates, no optional dependencies. Feature flags defined but no features currently active:

| Crate | Version | Role |
|-------|---------|------|
| `serde` | 1 (with `derive`) | `DeserializeOwned` bound on `build::<T>()` |
| `toml` | 0.8 | File parsing, `Value`/`Table` as intermediate representation |
| `thiserror` | 2 | Error derive macros |

Dev dependencies: `tempfile` (filesystem tests), `serial_test` (env-var test serialization).

No async runtime. No logging framework. No CLI parser.

---

## Design Principles (as implemented)

- **Trait-based extensibility** — `ConfigSource` is the single extension point. Custom sources implement one method and integrate via `with_source()` with zero changes to library code.
- **Unified merge semantics** — All sources produce `ConfigEntry`. Root-level entries deep-merge tables; path-targeted entries create intermediate structure. Registration order determines priority.
- **Deserialization at build time** — The merged TOML table is deserialized once into `T`. After `build()`, config access is a plain struct field reference.
- **Explicit error handling** — No panics in library code. All fallible operations return `Result`. All validation deferred to `entries()` for consistent error flow through `build()`.
- **Compile-time safety** — The AppContext builder uses type-state to enforce that config is provided and async requirements are met. Invalid usage is rejected by the compiler, not at runtime.
- **Minimal dependency surface** — Three crates, all widely used and stable.

---

## Known Limitations

- **Resolution operates on TOML intermediate** — Variable references can only target paths that exist in the merged TOML table, not in the final typed struct. References are resolved before deserialization.
- **`lookup_value` does not support array traversal** — References inside array elements are collected and resolved correctly (via position-indexed paths), but user-written reference paths like `${arr.0}` cannot traverse into arrays — `lookup_value` only navigates tables.
