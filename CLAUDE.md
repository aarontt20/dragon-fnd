# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build              # Build the library
cargo test               # Run all tests (28 tests)
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
│   ├── mod.rs          # Public exports: Config, ConfigError, ConfigSource, ConfigEntry
│   ├── source.rs       # Core abstractions: ConfigSource trait, ConfigEntry, merge_at_path
│   ├── builder.rs      # Config builder orchestrating sources
│   ├── file.rs         # FileSource: loads TOML files
│   ├── env.rs          # EnvSource: loads environment variables
│   ├── resolve.rs      # Variable reference resolution (${path.to.field})
│   └── error.rs        # ConfigError enum
└── context/
    └── mod.rs          # AppContext and AppContextBuilder
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

4. **Error hierarchy**: `ConfigError` for config-specific errors, wrapped by top-level `Error`

### Variable Resolution

String values can reference other config values using `${path.to.field}` syntax. Resolution happens after all sources are merged:
- Iterative resolution handles chained references
- Circular dependency detection (max 100 iterations)
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

let config: T = Config::builder()
    .with_file("defaults.toml", true)
    .with_source(MyCustomSource::new())
    .build()?;
```

## Known Limitations

- **AppContext type-state**: The builder pattern uses runtime validation (`Err(MissingConfig)`) rather than compile-time enforcement
- **Resolution cloning**: Each resolution pass clones the config table (up to 100 times for deeply chained references)
