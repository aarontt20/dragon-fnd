# Plan: Pre-Logging Foundation — Tests, AppContext Rework, Feature Flags

## Context

dragon-fnd's config subsystem is built, but before adding the first feature-gated subsystem (logging), several prerequisites must land:

1. **Tests are not in-tree.** 52 tests are documented in TEST.md but none are runnable. Building on untested code is risky.
2. **AppContext only holds config.** It needs to support feature-gated subsystem handles (log guard, db pool, etc.) with no `Option` wrapping and no panicking.
3. **No feature flag infrastructure exists.** Cargo.toml has no `[features]` section.
4. **Code fixes identified by design review.** Several issues found during review should be addressed in this foundation pass rather than deferred.

The agreed design for AppContext is a **hybrid type-state** approach:
- **Builder** has type-state (compile-time enforcement of config presence and async requirements)
- **AppContext** is concrete (`AppContext<C>`) with `#[cfg(feature)]`-gated fields and methods
- The builder guarantees all feature-gated fields are initialized before build returns
- No `Option`, no panicking — fields are unconditional within their feature gate

## Design Review Fixes

The following issues were identified by a three-agent design review and are incorporated into this plan. Each fix is tagged with its issue number for traceability.

### Fix 1: Remove `assert!` from `EnvSource::new`

**Problem:** `assert!(!separator.is_empty())` in `env.rs:15` panics in library code, violating Constraint #1 (no panicking in library code).

**Fix:** Remove the `assert!`. Add `ConfigError::InvalidSeparator` variant. Validate in `entries()` — consistent with how `FileSource` defers validation to its `entries()` call. The error surfaces at `build()` time through the existing error channel.

```rust
// src/config/env.rs — constructor becomes infallible
pub fn new(prefix: impl Into<String>, separator: impl Into<String>) -> Self {
    Self {
        prefix: prefix.into(),
        separator: separator.into(),
    }
}

// Validation moves to entries()
fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
    if self.separator.is_empty() {
        return Err(ConfigError::InvalidSeparator);
    }
    // ... existing logic
}
```

```rust
// src/config/error.rs — new variant
#[error("env source separator must not be empty")]
InvalidSeparator,
```

### Fix 9: Rename `Config` to `ConfigBuilder`

**Problem:** `Config` is a builder type, not a config value. `Config::builder()` returns `Self`, which is confusing — the type IS the builder.

**Fix:** Rename to `ConfigBuilder` with constructor `new()`:

```rust
// src/config/builder.rs
#[derive(Default)]
#[must_use = "builders do nothing until .build() is called"]
pub struct ConfigBuilder { ... }

impl ConfigBuilder {
    pub fn new() -> Self { Self::default() }
    // ... remaining methods unchanged
}
```

Update re-exports in `src/config/mod.rs` and `src/lib.rs`.

### Fix 13: Restructure `merge_at_path`

**Problem:** `source.rs:38` uses `.expect("path is non-empty")` after an `is_empty()` guard — a panic site on a logically unreachable path.

**Fix:** Restructure with `let-else`:

```rust
pub fn merge_at_path(table: &mut Table, path: &[String], value: Value) {
    let Some((first, rest)) = path.split_first() else {
        if let Value::Table(overlay) = value {
            deep_merge(table, overlay);
        }
        return;
    };
    // ... rest unchanged, using first/rest directly
}
```

### Fix 15: Remove trim from `is_pure_reference`

**Problem:** `is_pure_reference` trims whitespace before checking, but `resolve_string` does not. `"  ${foo}  "` silently becomes a type-preserving pure reference (no spaces), creating inconsistent semantics.

**Fix:** Remove the `.trim()`. Whitespace is significant — `"  ${foo}  "` is string interpolation producing `"  <value>  "`, not pure substitution.

### Fix 16: Re-export `ConfigSource` and `ConfigEntry` at crate root

**Problem:** The primary extension point requires `dragon_fnd::config::{ConfigSource, ConfigEntry}`. These are already exported from `config/mod.rs`, but not re-exported at the crate root — users must reach into the submodule.

**Fix:** Update `src/lib.rs` to re-export them at the crate root alongside `ConfigBuilder` and `ConfigError`:
```rust
pub use config::{ConfigBuilder, ConfigEntry, ConfigError, ConfigSource};
```

### Fix 14: Document `get_value`/`get_value_mut` duplication

**Problem:** Two near-identical functions in `resolve.rs` differ only in `&` vs `&mut`.

**Fix:** Accept with a comment. This is a standard Rust borrow-checker limitation. Add:
```rust
// Structurally identical to get_value; duplicated because Rust's borrow
// checker requires separate shared and mutable traversal paths.
// If the navigation logic changes, update both functions.
```

## Step 1: Feature Flag Infrastructure + Dev Dependencies

Add to `Cargo.toml`:

```toml
[features]
default = []
# Subsystem features (empty for now — deps added when subsystems are built)
# logging = ["dep:tracing", "dep:tracing-subscriber", "dep:tracing-appender"]
# sqlite = ["dep:sqlx", "dep:tokio"]
# http = ["dep:axum", "dep:tokio"]
# shutdown = ["dep:tokio"]
# fs = []

[dev-dependencies]
tempfile = "3"
serial_test = "3"
```

`tempfile` for filesystem tests (`file.rs`). `serial_test` for env-var tests that mutate global state (`env.rs`).

## Step 2: Apply Code Fixes

Before writing tests, apply Fixes 1, 9, 13, 14, 15, 16 from the Design Review Fixes section above. This ensures tests are written against the corrected API from the start.

**Files modified:**
- `src/config/env.rs` — remove `assert!`, constructor becomes infallible
- `src/config/error.rs` — add `InvalidSeparator` variant
- `src/config/builder.rs` — rename `Config` to `ConfigBuilder`, `builder()` to `new()`
- `src/config/mod.rs` — update re-export from `Config` to `ConfigBuilder`
- `src/config/source.rs` — restructure `merge_at_path` with `let-else`
- `src/config/resolve.rs` — remove `.trim()` from `is_pure_reference`, add comment to `get_value_mut`
- `src/lib.rs` — update re-exports: `ConfigBuilder`, add `ConfigSource`, `ConfigEntry`

## Step 3: Bring Tests Back In-Tree

Source: `TEST.md` documents all 52 tests with behavior descriptions.

**Unit tests** — inline `#[cfg(test)] mod tests` at the bottom of each source file:

| File | Tests | Notes |
|------|-------|-------|
| `src/config/source.rs` | 5 | merge_at_path (4) + ConfigEntry constructors (1) |
| `src/config/file.rs` | 3 | Valid file, required missing, optional missing |
| `src/config/env.rs` | 12 | coerce_value (5) + EnvSource (7) |
| `src/config/resolve.rs` | 32 | Basic refs, pure refs, chains, arrays, escapes, interpolation, errors |

**Total: 52 unit tests.**

Unit tests for `resolve.rs` need access to private functions (`resolve_references`, `is_pure_reference`, etc.) — inline modules have this access.

**Integration tests** — `tests/` directory for public API:

| File | Tests | Notes |
|------|-------|-------|
| `tests/config_builder.rs` | ~4 | End-to-end: builder chain, multiple sources, deserialization, error propagation |

Context integration tests (`tests/context.rs`) are written in Step 6 after the AppContext rework — no old-API tests.

### Test helpers and conventions

**`EnvGuard`** — RAII struct for setting/removing env vars safely in tests. All env-mutating tests use `#[serial]` from the `serial_test` crate to prevent data races (`std::env::set_var` is not thread-safe; `unsafe fn` since Rust 1.81).

**File tests** — use `tempfile::tempdir()` for RAII-managed temp directories.

**EnvSource empty separator test** — tests `EnvSource::new("APP", "").entries()` returns `Err(ConfigError::InvalidSeparator)`. No `#[should_panic]` — the constructor no longer panics.

**`is_pure_reference` whitespace test** — renamed to `whitespace_around_reference_is_interpolation` and updated to verify that `"  ${foo}  "` is treated as string interpolation (result includes spaces), not pure substitution.

## Step 4: Rework AppContext + Builder

### AppContext struct

```rust
pub struct AppContext<C> {
    config: C,
    // Future feature-gated fields:
    // #[cfg(feature = "logging")]
    // log_guard: WorkerGuard,
    // #[cfg(feature = "sqlite")]
    // db_pool: Pool,
}
```

Manual `Debug` impl instead of `#[derive(Debug)]` — `WorkerGuard` (arriving with the logging subsystem) does not implement `Debug`, so the derive will break. Writing the manual impl now prevents a guaranteed compile failure in the next task:

```rust
impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppContext")
            .field("config", &self.config)
            // Future: #[cfg(feature = "logging")]
            // .field("log_guard", &"<WorkerGuard>")
            .finish()
    }
}
```

Accessors:
- `config(&self) -> &C` — always available
- Future: `#[cfg(feature = "logging")] log_guard(&self) -> &WorkerGuard` etc.

### Builder type-state

Two type-level dimensions on the builder:

```rust
// Config presence markers
#[doc(hidden)]
pub struct NoConfig;
#[doc(hidden)]
pub struct Configured<C>(C);

// Async requirement markers
#[doc(hidden)]
pub struct SyncBuild;
// AsyncBuild is NOT defined in this plan — deferred until first async subsystem

pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
}
```

Marker types are `pub` with `#[doc(hidden)]` — nameable for compiler error messages and power users, but hidden from generated docs. This is the standard Rust ecosystem convention for type-state markers.

**Transitions:**

| Method | Available when | Produces |
|--------|---------------|----------|
| `AppContext::builder()` | always | `AppContextBuilder<NoConfig, SyncBuild>` |
| `.with_config(config)` | `Cfg = NoConfig` | `AppContextBuilder<Configured<C>, SyncBuild>` |
| `.build_sync()` | `Cfg = Configured<C>`, `Async = SyncBuild` | `AppContext<C>` (infallible) |

**`build_sync()` is infallible** — returns `AppContext<C>`, not `Result`. The type-state design proves at compile time that config is present and no async subsystems are registered. If nothing can fail, the return type must say so. When a sync subsystem that can fail during initialization is added, the signature changes at that point — an expected minor version change at pre-1.0.

**`build()` (async) is deferred** — not implemented in this plan. There is no async subsystem to initialize. When `sqlite` lands, it introduces `AsyncBuild` and the `build()` method simultaneously:

```rust
// Future — NOT implemented now
// #[doc(hidden)]
// pub struct AsyncBuild;
//
// impl<C> AppContextBuilder<Configured<C>, AsyncBuild> {
//     pub async fn build(self) -> Result<AppContext<C>, Error> { ... }
// }
```

The `PhantomData<Async>` parameter exists now with `SyncBuild` as the default, so `AsyncBuild` can be added later without restructuring the builder.

Future subsystem methods (NOT implemented in this step, but the pattern):

| Method | Available when | Effect |
|--------|---------------|--------|
| `.with_logging()` | `Cfg = Configured<C>` | same Async state |
| `.with_database()` | `Cfg = Configured<C>` | transitions `Async → AsyncBuild` |

**Key constraint:** `build_sync()` only exists on `AppContextBuilder<Configured<C>, SyncBuild>`. Adding an async subsystem transitions to `AsyncBuild`, removing `build_sync()` from the type.

### Compile-time verification via doc-tests

Type-state correctness is verified via `compile_fail` doc-tests — zero dependencies, co-located with the code:

```rust
/// ```compile_fail
/// use dragon_fnd::context::AppContext;
/// // Fails: no config provided
/// let _ctx = AppContext::builder().build_sync();
/// ```
```

### Error type changes

Remove `Error::MissingConfig` — the type-state makes it impossible to build without config. `Error` temporarily has one variant (`Config(ConfigError)`). **Keep `Error` as-is** — logging arrives in the very next task, which will add `Error::Logging(LoggingError)`. Collapsing and re-expanding the enum would be pointless churn.

### Teardown contract (documentation only)

Document in DESIGN.md that `AppContext` follows two teardown strategies:
- **Sync handles** (e.g., `WorkerGuard` for logging): cleaned up via `Drop`. Rust drops struct fields in declaration order — declare fields so that dependent handles drop before their dependencies.
- **Async handles** (e.g., db pool, HTTP server): will require an explicit `ctx.shutdown().await` method, implemented when the shutdown subsystem lands. `Drop` cannot `await`.

This is documentation only — no `shutdown()` method is implemented in this plan. The contract is specified now so the logging and database tasks know what they are building toward.

### Migration from current API

This is a breaking change at 0.1.0 (semver-legal for pre-1.0, no external consumers).

Current:
```rust
Config::builder()
    .with_file("config.toml", true)
    .build::<MyConfig>()?;
AppContext::builder().with_config(config).build()?
```

New:
```rust
ConfigBuilder::new()
    .with_file("config.toml", true)
    .build::<MyConfig>()?;
AppContext::builder().with_config(config).build_sync()
```

## Step 5: Update `src/error.rs`

`src/error.rs`:
- Remove `MissingConfig` variant
- Keep `Error` enum with single `Config(ConfigError)` variant (logging will add the second variant)

Note: `src/lib.rs` re-exports (`ConfigBuilder` rename, adding `ConfigSource`/`ConfigEntry`) are handled in Step 2 as part of Fix 9 and Fix 16. This step only touches `error.rs`.

## Step 6: Create Context Integration Tests

Create `tests/context.rs` for the new type-state API (not the old API):
- Builder chain with `with_config()` and `build_sync()` — verify config accessor works
- Verify `build_sync()` is infallible (no `?` needed)
- `compile_fail` doc-tests on the builder verify that `build_sync()` without `with_config()` does not compile

## Step 7: Update Examples

Update `examples/example.rs` for the new API:
- `Config::builder()` → `ConfigBuilder::new()`
- `.build()?` → `.build_sync()` (no `?`)

## Step 8: Update Documentation

| File | Changes |
|------|---------|
| `DESIGN.md` | Update: AppContext section (type-state design, infallible `build_sync()`), EnvSource section (remove "sole exception" framing), module structure (ConfigBuilder rename), error types (MissingConfig removed), add teardown contract section |
| `DOC.md` | Update: AppContext/Builder docs for new API, ConfigBuilder rename, `build_sync()` return type, add `InvalidSeparator` to error table |
| `CLAUDE.md` | Update: test count (52), ConfigBuilder rename in architecture section, remove MissingConfig from error docs |
| `TEST.md` | Update: header count (52), note tests are now in-tree, update EnvSource test description |

## Critical Files

| File | Action |
|------|--------|
| `Cargo.toml` | Edit — add `[features]`, `[dev-dependencies]` |
| `src/config/env.rs` | Edit — remove `assert!`, infallible constructor |
| `src/config/error.rs` | Edit — add `InvalidSeparator` variant |
| `src/config/builder.rs` | Rewrite — rename to `ConfigBuilder`, `builder()` → `new()` |
| `src/config/mod.rs` | Edit — update re-export to `ConfigBuilder` |
| `src/config/source.rs` | Edit — restructure `merge_at_path`, add `#[cfg(test)]` module |
| `src/config/resolve.rs` | Edit — remove trim, add duplication comment, add `#[cfg(test)]` module |
| `src/config/file.rs` | Edit — add `#[cfg(test)]` module |
| `src/context/mod.rs` | Rewrite — type-state builder, manual Debug, infallible `build_sync()` |
| `src/error.rs` | Edit — remove `MissingConfig` |
| `src/lib.rs` | Edit — update re-exports (`ConfigBuilder`, `ConfigSource`, `ConfigEntry`) |
| `tests/config_builder.rs` | Create — integration tests for ConfigBuilder |
| `tests/context.rs` | Create — integration tests for new AppContext API |
| `examples/example.rs` | Edit — update to new API |
| `docs/DESIGN.md` | Edit — multiple sections |
| `DOC.md` | Edit — API docs |
| `CLAUDE.md` | Edit — architecture section, test count |
| `TEST.md` | Edit — header count, in-tree status |

Reference files (read-only):
- `TEST.md` — test descriptions to implement from
- `../dragon-fnd-old/src/builder.rs` — old type-state pattern for reference

## Verification

1. `cargo test` — all 52 unit tests + integration tests pass
2. `cargo clippy` — no warnings
3. `cargo build` — clean build
4. **Type-state compile test:** `compile_fail` doc-test verifies `AppContext::builder().build_sync()` does NOT compile (no config)
5. **Type-state compile test:** `compile_fail` doc-test verifies `AppContext::builder().with_config(x).build_sync()` DOES compile
6. **API smoke test:** `examples/example.rs` works with the new API
7. `cargo doc` — documentation generates cleanly

## Implementation Order

Execute in this sequence (each step depends on the previous):

1. Add `[features]` and `[dev-dependencies]` to `Cargo.toml`
2. Apply code fixes (EnvSource, ConfigBuilder rename, merge_at_path, is_pure_reference, re-exports, get_value comment)
3. Add unit tests to source files (52 tests, written against corrected API)
4. Add integration tests in `tests/config_builder.rs`
5. Rework `src/context/mod.rs` (type-state builder, manual Debug, infallible `build_sync()`)
6. Update `src/error.rs` (remove `MissingConfig`; `src/lib.rs` re-exports already handled in step 2)
7. Create `tests/context.rs` for new API
8. Update `examples/example.rs` for new API
9. Update docs (DESIGN.md, DOC.md, CLAUDE.md, TEST.md)
10. Run full verification suite
