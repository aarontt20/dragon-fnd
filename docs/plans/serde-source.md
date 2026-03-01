# SerdeSource — Struct-to-Config Adapter

## Context

dragon-fnd's config system accepts multiple sources (TOML files, env vars, custom `ConfigSource` impls) and merges them into a typed config struct. VISION.md identifies CLI args as "just another config source" — the library should provide an adapter that makes it trivial to feed parsed args into the config pipeline, without owning the parser or depending on clap.

The old version (`dragon-fnd-old`) had a `with_args::<A>()` that called `A::parse()` internally, depended on clap, used fragile empty-table detection for `Option::None`, and silently dropped path navigation errors. This adapter replaces all of that with a clean `ConfigSource` implementation that serializes any `Serialize` type.

## Design

**`SerdeSource`** — a `ConfigSource` that takes any `T: Serialize`, serializes it to a `toml::Table` at construction time, and emits a single root-level `ConfigEntry`. Always-on (no feature gate — uses only `serde` + `toml`, both already always-on deps).

The primary use case is feeding parsed CLI args into the config pipeline, but `SerdeSource` is a general-purpose adapter — any serializable struct (hardcoded defaults, remote config responses, test fixtures) can be fed through it.

### Naming rationale

The name `SerdeSource` communicates the trait bound (`T: Serialize`) directly. An alternative considered was `StructSource` (describes what it accepts rather than the mechanism). `SerdeSource` was chosen because `Serialize` is the actual contract — the name signals to Rust developers exactly what the type parameter requires, even though "serde" is technically the mechanism. The file is named `serde_source.rs` (not `serde.rs`) to avoid shadowing the `serde` extern crate in `config/mod.rs`.

### VISION.md departure

VISION.md planned this as a `cli` feature-gated subsystem named `ClapSource`. This plan departs in three ways:

1. **Name**: `SerdeSource` instead of `ClapSource` — the adapter accepts any `Serialize` type, not just clap structs
2. **Scope**: general-purpose adapter, not CLI-specific
3. **Feature gate**: always-on instead of behind `cli` — the adapter adds zero new dependencies (uses only `serde` + `toml`, both already always-on) and minimal API surface (one struct + one error variant). A feature gate would add friction without reducing compile time or dependency weight.

VISION.md's subsystem table and CLI args section should be updated when this lands.

### How it works

1. User parses args with whatever parser they want (clap, pico-args, manual)
2. `SerdeSource::new(&args)?` serializes the struct to a `toml::Table`
3. User passes it to `ConfigBuilder::with_source(...)` like any other source
4. Existing merge semantics handle layering — later sources override earlier ones

**Priority note:** Sources are merged in registration order — later sources override earlier ones. For CLI-args-override-everything behavior, register `SerdeSource` last:

```rust
ConfigBuilder::new()
    .with_file("config/default.toml", true)  // lowest priority
    .with_env("MYAPP", "__")                 // medium priority
    .with_source(SerdeSource::new(&args)?)   // highest priority — registered last
    .build()?;
```

### Eager validation

`SerdeSource::new()` returns `Result<Self, ConfigError>` — serialization happens at construction time. This differs from `FileSource` and `EnvSource`, which defer validation to `entries()`.

The asymmetry is intentional: `FileSource` defers because the file is read at `entries()` time. `EnvSource` defers because environment variables are read at `entries()` time. `SerdeSource` has nothing to defer — the only work is serialization, and the input value is available at construction. Reporting errors immediately, at the call site where the user created the source, gives the most actionable feedback. Deferring would mean storing the original value (requiring ownership or boxing) only to fail later at a point further from the mistake.

### None handling

`toml::Table::try_from()` omits `Option::None` fields — the toml crate's `SerializeMap` catches the `UnsupportedNone` error internally and skips the field. Nested structs with all-None fields produce empty tables, which are harmless under `deep_merge` (merging nothing = no-op).

**Important:** This behavior is an implementation detail of the `toml` crate, not a documented public API guarantee. It is structurally grounded — TOML has no null type, so omitting None is the only sensible behavior — and it is unlikely to change. The `none_fields_omitted` unit test and `file_then_serde_override` integration test together form the regression safety net: if a future toml version changes this behavior, these tests will catch it.

**Defensive recommendation:** For maximum robustness, users should annotate Option fields with `#[serde(skip_serializing_if = "Option::is_none")]`. This makes the omission behavior explicit in the user's code, independent of toml crate internals:

```rust
#[derive(Parser, Serialize)]
struct Args {
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}
```

Without the annotation, the behavior is identical today — but the annotation protects against hypothetical future toml crate changes.

### Usage

```rust
#[derive(Parser, Serialize)]
struct Args {
    #[command(flatten)]
    server: ServerArgs,
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    verbose: Option<bool>,
}

#[derive(Args, Serialize)]
struct ServerArgs {
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
}

let args = Args::parse();  // user's parser, user's code
let config: AppConfig = ConfigBuilder::new()
    .with_file("config/default.toml", true)
    .with_env("MYAPP", "__")
    .with_source(SerdeSource::new(&args)?)  // registered last = highest priority
    .build()?;
```

### Limitations

`SerdeSource` serializes through `toml::Table`, which means the value takes a roundtrip through TOML's type system: `T → toml::Table → merge → toml::Value → T'`. Some Rust types do not survive this roundtrip:

| Type | Behavior | Why |
|------|----------|-----|
| Externally-tagged enums with data (serde default) | Serialize as nested tables | Works — `File { path }` becomes `{"File": {"path": "..."}}` |
| Unit enum variants | Serialize as strings | Works — `Level::Info` becomes `"Info"` |
| `u64` values > `i64::MAX` | `SerializeError` at `new()` | TOML integers are `i64` |
| `Option<Option<T>>` with `Some(None)` | Field omitted | The `UnsupportedNone` catch handles nested Options — field is absent from table |
| Newtype wrappers (`struct Port(u16)`) | Transparent — serializes as the inner type | Works correctly for roundtrip |
| `Vec<T>` | Serializes as TOML array | Works — common for multi-value CLI args |
| `HashMap<String, V>` | Serializes as TOML table | Works, but `HashMap<String, Option<T>>` with None values follows the same omission behavior as struct fields |

Types that cannot be represented as TOML tables (bare scalars, bare arrays at root level) produce `ConfigError::SerializeError` at `SerdeSource::new()` — they are caught eagerly, not silently dropped.

**Serde attributes:** Field names in the serialized TOML must match the keys in config files for merge/override to work. `#[serde(rename = "camelCase")]` changes the serialized key — if the TOML file uses `snake_case`, the keys won't match and the override won't apply. This is expected behavior but can be surprising.

## Implementation Steps

### Step 1: Add `SerializeError` variant to `ConfigError`

**File:** `src/config/error.rs`

Add after `DeserializeError`:
```rust
#[error("failed to serialize value to config: {0}")]
SerializeError(toml::ser::Error),
```

No `#[from]` — matches `DeserializeError` pattern. Enum is `#[non_exhaustive]`, so adding a variant is non-breaking.

### Step 2: Create `SerdeSource` implementation

**File:** `src/config/serde_source.rs` (new)

```rust
pub struct SerdeSource {
    table: toml::Table,
}
```

- **Constructor:** `pub fn new<T: Serialize>(value: &T) -> Result<Self, ConfigError>` — takes `&T` (caller retains ownership), serializes eagerly via `toml::Table::try_from()`, maps errors to `ConfigError::SerializeError`
- **`ConfigSource` impl:** `entries()` returns `vec![ConfigEntry::root(self.table.clone())]` — single root entry, existing merge handles everything. The clone is necessary because `entries()` takes `&self` per the `ConfigSource` trait contract. For config-sized data (tens to hundreds of keys) this cost is negligible and happens once at build time.
- **Derives:** `Debug`, `Clone`
- **`ConfigEntry::root()`** is `pub(crate)` — accessible because `serde_source.rs` is inside the crate

**Unit tests** (inline `#[cfg(test)]` module):

| Test | Verifies |
|------|----------|
| `basic_struct` | Simple struct serializes correctly, produces one root entry |
| `nested_struct` | Nested structs map to nested TOML tables |
| `none_fields_omitted` | `Option::None` fields absent from table (regression guard for toml crate behavior) |
| `some_fields_present` | `Option::Some(v)` fields present with unwrapped value |
| `mixed_none_and_some` | Only Some values appear in output |
| `all_none_nested` | Inner struct with all-None produces empty sub-table (does not wipe parent values) |
| `empty_struct` | Struct with no fields produces empty table (no-op merge) |
| `vec_fields` | `Vec<T>` fields serialize as TOML arrays |
| `non_struct_errors` | Bare scalar/vec returns `SerializeError` |
| `enum_with_data_serializes_as_nested_table` | Externally-tagged enum with data variant serializes as nested tables |
| `unit_enum_variants` | Unit enum variants serialize as strings |
| `nested_option_some_none_omitted` | `Option<Option<T>>` with `Some(None)` is omitted (UnsupportedNone catch handles nested Options) |
| `entries_returns_single_root` | Exactly one entry with empty path |

### Step 3: Wire up module and re-exports

**File:** `src/config/mod.rs`

- Add `mod serde_source;` after existing module declarations
- Add `pub use serde_source::SerdeSource;`

**File:** `src/lib.rs`

- Add `SerdeSource` to the existing `pub use config::{...}` line

### Step 4: Integration tests

**File:** `tests/config_serde_source.rs` (new)

Follow the pattern in `tests/config_builder.rs` (imports from `dragon_fnd`, test config structs with `Deserialize`, `tempfile` for TOML files).

| Test | Verifies |
|------|----------|
| `serde_source_standalone` | SerdeSource as sole source through full pipeline |
| `file_then_serde_override` | File defaults + SerdeSource overrides; None fields keep file values |
| `serde_source_with_nested_structs` | Nested structs deep-merge correctly against file |
| `serde_source_with_variable_references` | Sanity check: existing `${...}` resolution pipeline works when SerdeSource participates in the source stack (not SerdeSource-specific — verifies no regression in the merge→resolve→deserialize pipeline) |
| `serde_source_serialize_error` | Non-struct type produces `ConfigError::SerializeError` at `new()`, before `build()` |
| `three_way_merge` | File + env + serde layering (full priority stack, verifying registration-order priority) |
| `serde_source_with_vec_fields` | Vec fields survive the full pipeline roundtrip |

### Step 5: Documentation updates

- **`docs/DESIGN.md`**: Add SerdeSource to built-in sources section, add `SerializeError` to error table
- **`docs/VISION.md`**: Update CLI args subsystem entry — note that it landed as always-on `SerdeSource` rather than feature-gated `ClapSource`
- **`DOC.md`**: Add `config::serde_source` module section with usage example, limitations, and defensive `skip_serializing_if` recommendation
- **`TEST.md`**: Add unit + integration test entries, update counts

## Files Modified

| File | Change |
|------|--------|
| `src/config/error.rs` | Add `SerializeError` variant |
| `src/config/serde_source.rs` | **New** — `SerdeSource` + unit tests |
| `src/config/mod.rs` | Add `mod serde_source;` + `pub use serde_source::SerdeSource;` |
| `src/lib.rs` | Add `SerdeSource` to re-exports |
| `tests/config_serde_source.rs` | **New** — integration tests |
| `docs/DESIGN.md` | Document SerdeSource |
| `docs/VISION.md` | Update CLI args subsystem entry |
| `DOC.md` | API documentation |
| `TEST.md` | Test coverage documentation |

## Key patterns to reuse

- `ConfigEntry::root(table)` — `src/config/source.rs:152` — root-level entry constructor
- `merge_at_path()` / `deep_merge()` — `src/config/source.rs:168,248` — existing merge semantics
- `EnvSource` — `src/config/env.rs` — structural pattern for a config source
- `StaticSource` test helper — `tests/config_builder.rs:13` — integration test pattern

## Design notes

**`ConfigEntry::root()` is `pub(crate)`** — this is the third built-in source using it (after `FileSource` and the test helper `StaticSource`). External `ConfigSource` implementors who want to emit a root-level table can use `ConfigEntry::at_path(vec![], ConfigValue::Table(...))` as the public equivalent — `merge_at_path` treats empty path as root. Consider making `root()` public in a future change if this pattern becomes common.

## Verification

```bash
cargo test                        # All tests pass (unit + integration + doc-tests)
cargo test serde                  # SerdeSource-specific tests
cargo test --features logging     # Full suite including logging
cargo clippy                      # No warnings
cargo doc --open                  # Documentation renders correctly
```
