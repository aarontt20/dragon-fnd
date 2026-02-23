# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Documentation

- `docs/DESIGN.md` — How the built system works (architecture, current state)
- `docs/VISION.md` — Where the project is going (planned subsystems, design philosophy)
- `DOC.md` — API documentation
- `TEST.md` — Test coverage documentation (89 unit + 16 integration + 2 doc-tests with logging feature)

## Build Commands

```bash
cargo build              # Build the library
cargo test               # Run all tests (58 unit + 8 integration + 2 doc-tests)
cargo test --features logging  # Run all tests including logging (89 unit + 16 integration + 2 doc-tests)
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
│   ├── mod.rs          # Public exports: ConfigBuilder, ConfigError, ConfigSource, ConfigEntry
│   ├── source.rs       # Core abstractions: ConfigSource trait, ConfigEntry, merge_at_path
│   ├── builder.rs      # ConfigBuilder orchestrating sources
│   ├── file.rs         # FileSource: loads TOML files
│   ├── env.rs          # EnvSource: loads environment variables
│   ├── resolve.rs      # Variable reference resolution (${path.to.field})
│   └── error.rs        # ConfigError enum
├── logging/            # Feature: "logging"
│   ├── mod.rs          # Re-exports, pub(crate) init_logging
│   ├── config.rs       # LoggingConfig, ConsoleConfig, FileConfig (serde types)
│   ├── builder.rs      # LoggingBuilder, ConsoleBuilder, FileBuilder (fluent API)
│   ├── error.rs        # LoggingError enum
│   └── init.rs         # Subscriber initialization, retention cleanup
└── context/
    └── mod.rs          # AppContext with type-state AppContextBuilder
```

### Core Abstractions

**ConfigSource trait** (`src/config/source.rs`):
- All config sources implement `ConfigSource: Send + Sync + Debug`
- Sources produce `Vec<ConfigEntry>` where each entry has a path and value
- Unified `merge_at_path()` handles both root-level deep merges and path-targeted inserts

**ConfigEntry**:
- `path: Vec<String>` - empty for root-level (files), non-empty for specific paths (env vars)
- `value: toml::Value` - the value to merge

**Built-in sources**:
- `FileSource` - reads TOML files, returns single root entry
- `EnvSource` - reads env vars with prefix/separator, returns entries per variable

### Key Design Decisions

1. **Trait-based extensibility**: New sources (CLI args, remote config) can be added via `with_source()` without modifying library code

2. **Unified merge semantics**: `merge_at_path()` deep-merges tables, replaces scalars/arrays. Later sources override earlier ones.

3. **Deserialization at build time**: Config is parsed once into the target type `T`, making subsequent access zero-cost

4. **Error hierarchy**: `ConfigError` for config-specific errors, `LoggingError` for logging errors, both wrapped by top-level `Error`

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
            toml::Value::String("value".into()),
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
4. **Trait boundary for every subsystem.** Each subsystem follows three layers: trait (always available) → default implementation (feature-gated) → custom implementation (user-provided). The library says "satisfy this contract" not "use this crate." **Exception:** Logging uses `tracing` directly — the crate itself is the trait boundary. Adding a library-level trait on top would add indirection with no value.
5. **Feature-gated opt-in.** Each subsystem lives behind a Cargo feature flag. Downstream projects only pay for what they enable.
6. **Own it or bridge to it.** For each subsystem, ask: "does it make sense for the library to own this?" Own it (behind a feature) when it's pure boilerplate nobody wants to write twice — logging setup, database pool init, shutdown signal handling, server lifecycle. Bridge to it when it's too application-specific — argument parsing, HTTP routing, middleware, query logic, file content decisions. This is the decision framework for where the library's responsibility ends.

## Known Limitations

- **Resolution operates on TOML intermediate**: Variable references target paths in the merged TOML table, not the final typed struct
- **User-written references cannot traverse arrays**: The internal `get_value`/`get_value_mut` functions support position-indexed array traversal (used for resolving references inside array elements), but user-written reference paths like `${arr.0}` go through `lookup_value`, which only navigates tables
