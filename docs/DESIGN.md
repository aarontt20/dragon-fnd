# DESIGN.md — How the Built System Works

This document describes dragon-fnd as it exists in code today. For future plans and the full vision, see [VISION.md](VISION.md).

---

## What It Does

dragon-fnd is a foundation library for Rust applications that provides typed configuration loading from multiple sources (TOML files, environment variables, custom sources) with deep merge semantics, graph-based variable reference resolution, and an application context skeleton. It ships as a single crate with three dependencies (serde, toml, thiserror) and no feature flags. The config subsystem is the only built component. The library does not own `main()` and does not prescribe application structure.

---

## Module Structure

```
src/
├── lib.rs                 # Crate root, re-exports public API
├── error.rs               # Top-level Error enum (2 variants)
├── config/
│   ├── mod.rs             # Public exports: Config, ConfigError, ConfigSource, ConfigEntry
│   ├── source.rs          # ConfigSource trait, ConfigEntry, merge_at_path, deep_merge
│   ├── builder.rs         # Config builder: fluent API, generic build::<T>()
│   ├── file.rs            # FileSource: loads TOML files
│   ├── env.rs             # EnvSource: loads environment variables
│   ├── resolve.rs         # Graph-based ${path.to.field} variable resolution
│   └── error.rs           # ConfigError enum (9 variants)
└── context/
    └── mod.rs             # AppContext<C> and AppContextBuilder<C>
```

10 source files, ~642 lines total.

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

`merge_at_path()` handles all merge scenarios with three cases:
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

The constructor asserts the separator is non-empty (`assert!`). This is the one panic point in the library — it guards against a programmer error at construction time, not a runtime failure.

### Config Builder

`src/config/builder.rs` — orchestrates sources into a typed config value.

```rust
pub struct Config {
    sources: Vec<Box<dyn ConfigSource>>,
}
```

Fluent API:
- `Config::builder()` — creates an empty builder
- `.with_file(path, required)` — adds a `FileSource`
- `.with_env(prefix, separator)` — adds an `EnvSource`
- `.with_source(impl ConfigSource + 'static)` — adds any custom source

`build::<T>()` executes in three steps:
1. Iterate all sources in registration order, collect entries, merge into a single `toml::Table` via `merge_at_path` (later sources override earlier ones)
2. Resolve `${...}` variable references in the merged table
3. Deserialize the table into `T` via `toml::Value::try_into()`

Deserialization happens once at build time. After `build()`, config access is a plain struct field reference with no parsing overhead.

### Variable Resolution

`src/config/resolve.rs` — the largest module (271 lines). Uses a graph-based approach with three phases:

**Phase 1 — Collection.** Walk the entire merged table. For each string value, parse `${path.to.field}` references. Collect them as `(source_path, target_path)` edge pairs. `$$` is recognized as an escape (skipped during collection). Unclosed `${` produces `ConfigError::UnclosedReference`. References inside array elements are collected with position-indexed paths (e.g., `["arr", "0"]`), so a string at `arr[0]` that contains `${some.ref}` is tracked and resolved correctly. Note: user-written reference paths like `${arr.0}` cannot traverse into arrays — `lookup_value` only navigates tables.

**Phase 2 — Topological sort.** Build a dependency graph from the collected edges. Sort via DFS with an `in_progress` set for cycle detection. If a node is visited while already in-progress, the reference chain is circular → `ConfigError::CircularReference`. The result is a resolution order where dependencies come before dependents.

**Phase 3 — Resolution.** Process each path in topological order. Two modes:

- **Pure reference** — the string is exactly `${path}` (trimmed, single reference, nothing else). The entire value is replaced with the target value's type — integers, booleans, arrays, and tables pass through intact. This enables type-preserving references.
- **String interpolation** — the string contains `${path}` embedded among other text. Each reference is replaced with the string representation of the target value. Scalars (string, integer, float, boolean, datetime) convert naturally. Non-scalar targets (arrays, tables) produce `ConfigError::NonScalarReference`.

Escape sequences: `$$` becomes literal `$` during resolution, but only in strings that also contain actual `${...}` references. Strings without references are not processed.

---

## Error Types

### ConfigError

`src/config/error.rs` — 9 variants, `#[non_exhaustive]`:

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

### Top-level Error

`src/error.rs` — 2 variants, `#[non_exhaustive]`:

| Variant | Meaning |
|---------|---------|
| `Config(ConfigError)` | Wraps any config error (with `#[from]` for `?` conversion) |
| `MissingConfig` | `AppContext` built without providing config |

---

## AppContext

`src/context/mod.rs` — a generic container for application state.

```rust
pub struct AppContext<C> {
    config: C,
}
```

`AppContext::config()` returns `&C` — zero-cost after build.

The builder uses a type-level transition:
- `AppContext::builder()` returns `AppContextBuilder<()>`
- `.with_config(config)` transitions to `AppContextBuilder<C>`
- `.build()` produces `AppContext<C>` or returns `Err(Error::MissingConfig)`

The config-is-present check is runtime (`Option::ok_or`), not compile-time. The builder's generic parameter changes from `()` to `C`, but both states expose `build()`, and the `()` state technically allows calling `build()` without `with_config()` — it fails at runtime.

---

## Dependencies

Three crates, no optional dependencies, no feature flags:

| Crate | Version | Role |
|-------|---------|------|
| `serde` | 1 (with `derive`) | `DeserializeOwned` bound on `build::<T>()` |
| `toml` | 0.8 | File parsing, `Value`/`Table` as intermediate representation |
| `thiserror` | 2 | Error derive macros |

No async runtime. No logging framework. No CLI parser.

---

## Design Principles (as implemented)

- **Trait-based extensibility** — `ConfigSource` is the single extension point. Custom sources implement one method and integrate via `with_source()` with zero changes to library code.
- **Unified merge semantics** — All sources produce `ConfigEntry`. Root-level entries deep-merge tables; path-targeted entries create intermediate structure. Registration order determines priority.
- **Deserialization at build time** — The merged TOML table is deserialized once into `T`. After `build()`, config access is a plain struct field reference.
- **Explicit error handling** — No panics in normal operation. All fallible operations return `Result`. The `EnvSource` constructor's `assert!` is the sole exception, guarding a programmer error (empty separator) at construction time.
- **Minimal dependency surface** — Three crates, all widely used and stable.

---

## Known Limitations

- **AppContext runtime validation** — The builder uses `Option::ok_or` to check for config, not compile-time type-state enforcement. Calling `.build()` without `.with_config()` compiles but fails at runtime.
- **EnvSource assert** — The empty separator check panics rather than returning a `Result`. This is a conscious trade-off for programmer error vs. runtime error.
- **Resolution operates on TOML intermediate** — Variable references can only target paths that exist in the merged TOML table, not in the final typed struct. References are resolved before deserialization.
