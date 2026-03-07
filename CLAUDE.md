# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Documentation

- `docs/DESIGN.md` — How the built system works (architecture, current state)
- `docs/VISION.md` — Where the project is going (planned subsystems, design philosophy)
- `docs/DOC.md` — API documentation
- `docs/TESTS.md` — Test coverage documentation (208 unit + 62 integration + 5 doc-tests with all features)

## Build Commands

```bash
cargo build              # Build the library
cargo test               # Run base tests (95 unit + 25 integration + 4 doc-tests)
cargo test --features logging  # Include logging (149 unit + 35 integration + 4 doc-tests)
cargo test --features sqlite   # Include sqlite (121 unit + 40 integration + 4 doc-tests)
cargo test --features shutdown # Include shutdown (113 unit + 34 integration + 5 doc-tests)
cargo test --features shutdown,sqlite,logging  # Run all tests (208 unit + 62 integration + 5 doc-tests)
cargo test resolve       # Run tests matching "resolve"
cargo clippy             # Run linter
cargo doc --open         # Generate and view documentation
```

## Architecture

**dragon-fnd** is a foundation library providing typed configuration loading and application context management for Rust applications.

### Module Structure

```
src/
├── lib.rs              # Crate root, re-exports public API
├── error.rs            # Top-level Error enum
├── config/
│   ├── mod.rs          # Public exports: ConfigBuilder, ConfigError, ConfigSource, ConfigEntry, ConfigValue, ConfigTable, SerdeSource
│   ├── source.rs       # Core abstractions: ConfigSource trait, ConfigEntry, ConfigValue, ConfigTable, merge_at_path
│   ├── builder.rs      # ConfigBuilder orchestrating sources
│   ├── file.rs         # FileSource: loads TOML files
│   ├── env.rs          # EnvSource: loads environment variables
│   ├── serde_source.rs # SerdeSource: serializes any Serialize type into config
│   ├── resolve.rs      # Variable reference resolution (${path.to.field})
│   └── error.rs        # ConfigError enum
├── logging/            # Feature: "logging"
│   ├── mod.rs          # Re-exports, pub(crate) init_logging
│   ├── config.rs       # LoggingConfig, ConsoleConfig, FileConfig, LogFormat, RotationStrategy (serde types, private fields)
│   ├── builder.rs      # LoggingBuilder, ConsoleBuilder, FileBuilder (fluent API)
│   ├── error.rs        # LoggingError enum
│   ├── init.rs         # Subscriber initialization, validation
│   ├── retain.rs       # Retention cleanup for rotated log files
│   └── writer.rs       # SizeRotatingWriter with compression
├── sqlite/             # Feature: "sqlite"
│   ├── mod.rs          # Re-exports, pub(crate) init_pool, pub use SqlitePool
│   ├── config.rs       # SqliteConfig, JournalMode (serde types, private fields)
│   ├── builder.rs      # SqliteBuilder (fluent API)
│   ├── error.rs        # SqliteError enum
│   └── init.rs         # Pool creation, connectivity test, migrations
├── shutdown/           # Feature: "shutdown"
│   ├── mod.rs          # Re-exports, pub(crate) init_shutdown
│   ├── builder.rs      # ShutdownBuilder (fluent API)
│   ├── init.rs         # Shutdown struct, init_shutdown(), cleanup orchestration
│   ├── signal.rs       # Platform-aware signal handling (SIGTERM/SIGINT)
│   └── error.rs        # ShutdownError enum (Clone)
└── context/
    └── mod.rs          # AppContext with type-state AppContextBuilder, extension slot
```

### Core Abstractions

**ConfigSource trait** (`src/config/source.rs`):
- All config sources implement `ConfigSource: Send + Sync + Debug`
- Sources produce `Vec<ConfigEntry>` where each entry has a path and value
- Unified `merge_at_path()` handles both root-level deep merges and path-targeted inserts

**ConfigValue** (`src/config/source.rs`):
- Library-owned value type replacing `toml::Value` in the public API
- Variants: `String`, `Integer`, `Float`, `Boolean`, `Datetime`, `Array`, `Table(ConfigTable)`
- `ConfigTable` is a newtype over `BTreeMap<String, ConfigValue>` with `new()`, `insert()`, `get()`, `iter()`, `len()`, `is_empty()`, and `IntoIterator`
- Converted to `toml::Value` at the single merge boundary in `ConfigBuilder::build()`

**ConfigEntry**:
- `path: Vec<String>` - empty for root-level (files), non-empty for specific paths (env vars)
- `value: ConfigValue` - the value to merge

**Built-in sources**:
- `FileSource` - reads TOML files, returns single root entry
- `EnvSource` - reads env vars with prefix/separator, returns entries per variable

### Key Design Decisions

1. **Trait-based extensibility**: New sources (CLI args, remote config) can be added via `with_source()` without modifying library code

2. **Unified merge semantics**: `merge_at_path()` deep-merges tables, replaces scalars/arrays. Later sources override earlier ones.

3. **Deserialization at build time**: Config is parsed once into the target type `T`, making subsequent access zero-cost

4. **Error hierarchy**: `ConfigError` for config-specific errors, `LoggingError` for logging errors, `SqliteError` for sqlite errors, `ShutdownError` for shutdown errors, all wrapped by top-level `Error`

5. **Type-state AppContext builder**: `build_sync()` only exists when config is provided — compile-time enforcement, no runtime `MissingConfig` errors. Returns `Result` since subsystems can fail to initialize.

### Variable Resolution

String values can reference other config values using `${path.to.field}` syntax. Resolution happens after all sources are merged:
- Graph-based topological sort resolves dependencies in correct order
- Circular dependency detection via DFS cycle detection
- Pure references (`"${path}"`) preserve the target's type; embedded references interpolate as strings
- Escape with `$$` for literal `$`

### Extension Point

```rust
impl ConfigSource for MyCustomSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        Ok(vec![ConfigEntry::at_path(
            vec!["my".into(), "key".into()],
            ConfigValue::string("value"),
        )])
    }
}

let config: T = ConfigBuilder::new()
    .with_file("defaults.toml", true)
    .with_source(MyCustomSource::new())
    .build()?;
```

## Prior Art

`../dragon-fnd-old/` contains a previous, more complete implementation of this project. It has working implementations of all planned subsystems (config, CLI args, logging with tracing + file rotation, database with sqlx, HTTP with axum + graceful shutdown). **Consult it as a reference when implementing new subsystems** — but do not copy code directly. The rewrite exists because of structural problems in the old version.

Key files in dragon-fnd-old worth consulting:
- `src/builder.rs` — Type-state builder (NoAsync/RequiresAsync) pattern
- `src/logging.rs` — Tracing setup with file rotation and retention policies
- `src/database.rs` — sqlx pool init with migrations
- `src/http.rs` — Axum server with graceful shutdown
- `src/cli.rs` — CLI args as config overlay (the approach being replaced)
- `NOTES.md` — Edge cases and gotchas discovered during development
- `PLAN-code-review-fixes.md` — Known issues from code review

## Design Constraints

These are hard constraints learned from the previous attempt. Do not violate them:

1. **No panicking in library code.** Every accessor returns `Result` or `Option`. The old version had `ctx.database()` that panicked if the subsystem wasn't enabled — this was the single most criticized design flaw.
2. **No silent failures.** Every error must be surfaced. Missing config files, malformed references, failed path navigation — all must produce explicit errors, not silent fallbacks.
3. **Library does not own CLI args.** The library provides a `ConfigSource` adapter for feeding parsed args into config. It does not depend on clap, does not call `parse()`, and does not prescribe argument structure.
4. **Trait boundary for every subsystem.** Each subsystem follows three layers: trait (always available) → default implementation (feature-gated) → custom implementation (user-provided). The library says "satisfy this contract" not "use this crate." **Exceptions:** Logging uses `tracing` directly — the crate itself is the trait boundary. SQLite uses `sqlx` directly — there is no useful database abstraction in the Rust ecosystem that sqlx implements and others could substitute. Shutdown defers a trait boundary until a second trigger source materializes. Users who want a custom pool skip the `sqlite` feature and use `with_extension()`.
5. **Feature-gated opt-in.** Each subsystem lives behind a Cargo feature flag. Downstream projects only pay for what they enable.
6. **Own it or bridge to it.** For each subsystem, ask: "does it make sense for the library to own this?" Own it (behind a feature) when it's pure boilerplate nobody wants to write twice — logging setup, database pool init, shutdown signal handling, server lifecycle. Bridge to it when it's too application-specific — argument parsing, HTTP routing, middleware, query logic, file content decisions. This is the decision framework for where the library's responsibility ends.

## Known Limitations

- **Resolution operates on TOML intermediate**: Variable references target paths in the merged TOML table, not the final typed struct
- **User-written references cannot traverse arrays**: The internal `get_value`/`get_value_mut` functions support position-indexed array traversal (used for resolving references inside array elements), but user-written reference paths like `${arr.0}` go through `lookup_value`, which only navigates tables
