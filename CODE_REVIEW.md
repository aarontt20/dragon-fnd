# Code Review: dragon-fnd

Full project review covering architecture, correctness, safety, testing, and areas for improvement.

**Verdict: This is a well-structured, carefully designed library.** The code is clean, the architecture is sound, and the design constraints from CLAUDE.md are consistently followed. The issues below are improvements, not blockers.

---

## Strengths

### Architecture & Design
- **Trait-based extensibility** is implemented correctly. `ConfigSource` is minimal, composable, and the `ConfigEntry` abstraction elegantly unifies root-level (file) and path-targeted (env) sources.
- **Type-state builder** for `AppContext` is a genuine compile-time safety win. The `compile_fail` doc-test proving it works is a nice touch.
- **Error handling** follows the stated "no panicking" constraint rigorously. Every fallible operation returns `Result`. The `#[non_exhaustive]` on all error enums is forward-thinking.
- **Feature gating** is clean. The `#[cfg(feature = "logging")]` annotations are precisely placed and don't leak into non-logging code paths.
- **Separation of config vs. builder** in the logging subsystem is well done — the config types are pure serde structs, while the builders provide ergonomic construction.

### Code Quality
- Zero clippy warnings (verified with `--features logging`).
- 139 tests pass (115 unit + 22 integration + 2 doc-tests).
- Test coverage is thorough, especially for the config resolution module (35 tests covering all edge cases).
- The `SizeRotatingWriter` correctly handles cascading rotation prevention by resetting `bytes_written` immediately.
- Soft error handling in the writer via `eprintln!` is the right call — logging recursion would be far worse.

---

## Issues

### 1. Race condition in `SizeRotatingWriter` background compression thread (Medium)

**File:** `src/logging/writer.rs:144-158`

When compression is enabled, a background thread runs `compress_file` then `cleanup_old_logs`. The retention cleanup uses prefix matching to find files. If two rotations happen in rapid succession:

1. Rotation A spawns thread A to compress `app.20240101T120000.000`
2. Rotation B spawns thread B to compress `app.20240101T120000.001`
3. Thread A finishes compression, runs retention (keep 2 files), deletes thread B's uncompressed source file before thread B reads it

The `compress_file` function does handle `NotFound` on the final `remove_file`, but the `File::open` at the start of compression will fail with a confusing error. This is a narrow race window but worth documenting or addressing.

**Recommendation:** Either serialize background compression (e.g., via a channel to a single compression worker), or document this as a known limitation. At minimum, make `compress_file` tolerate `NotFound` on the input file and return `Ok(())` instead of an error.

### 2. `EnvSource` iteration order is non-deterministic (Low)

**File:** `src/config/env.rs:30`

`std::env::vars()` returns environment variables in an unspecified order. If two env vars map to the same config path (e.g., through case folding — `APP__Foo` and `APP__FOO` both become `["foo"]`), the last one wins based on iteration order, which is platform-dependent and non-reproducible.

**Recommendation:** After collecting entries, sort them by the original env var key so the behavior is deterministic across runs. Alternatively, detect the duplicate and return an error.

### 3. `resolve_string` handles `$$` escapes during interpolation but `resolve_at_path` doesn't strip them (Low)

**File:** `src/config/resolve.rs:247-277` and `src/config/resolve.rs:180-198`

In `resolve_string`, the `$$` -> `$` escape is handled inline during interpolation (line 261-263). But `resolve_at_path` calls `resolve_string` which handles it. Meanwhile, escape-only strings (strings with `$$` but no `${...}`) are handled in a separate Phase 4. This works correctly as implemented, but the dual handling of `$$` in two different code paths is fragile — if `resolve_string` is ever refactored to not handle `$$`, Phase 4 won't cover strings that also had references.

This is not a bug today, but worth a comment noting the coupling.

### 4. `retain_files` count may be off-by-one during rotation (Low)

**File:** `src/logging/writer.rs:159-172`

When rotation happens without compression, `cleanup_old_logs` runs immediately after the new rotated file is created. The rotated file is counted in the retention list. So `retain_files = 5` keeps 5 rotated files. But the *active* file (which will itself become a rotated file later) is not counted. This means at any point there are `retain_files + 1` files containing log data (5 rotated + 1 active). This is probably fine for most users, but the semantics should be documented.

### 5. `value_kind` is `pub(crate)` but could be private (Nit)

**File:** `src/config/source.rs:84`

`value_kind` is marked `pub(crate)` and used in `resolve.rs`, but it's a simple display helper. This is fine as-is, but if you want to minimize the internal API surface, it could be moved to a shared internal utility or inlined.

### 6. `LoggingError` has no `#[source]` on `InvalidFilter` (Nit)

**File:** `src/logging/error.rs:8`

`InvalidFilter(String)` stores the stringified error from `EnvFilter::try_new()`. The original error is lost. This means `LoggingError::source()` returns `None` for filter errors, breaking the error chain. Consider storing the original error or at least noting this is intentional.

### 7. `FileConfig` default `dir` is `"./logs"` (relative path) (Nit)

**File:** `src/logging/config.rs:85`

The default file config uses a relative path `./logs`. This is reasonable as a default, but relative paths are resolved from the process's working directory, which can be surprising in daemon contexts. Consider documenting this in the struct doc comment.

### 8. No `Serialize` implementation on config types (Design consideration)

The logging config types derive `Deserialize` but not `Serialize`. This means users can't round-trip configs (e.g., for debugging, writing defaults to a file, or testing). This is a deliberate choice (keeps the dependency surface smaller), but adding `Serialize` behind the `logging` feature would cost nothing and enable debugging workflows.

---

## Testing Observations

### What's well-tested
- Config merge semantics: 7 tests covering all merge scenarios
- Variable resolution: 35 tests covering interpolation, pure references, chains, arrays, escapes, errors
- Env source: 14 tests with proper `#[serial]` and `EnvGuard` for environment isolation
- Logging validation: 19 init tests covering all validation rule combinations
- Writer rotation: 10 tests including compression, retention, and rapid rotation

### Gaps worth filling
- **No test for `EnvSource` with duplicate paths from case folding** (relates to issue #2)
- **No test for `FileBuilder::new()` with a `rotation` other than `Never` combined with `max_bytes`** at the builder level (only validated at init time — but a builder-level test documenting that the builder allows it would be useful)
- **No negative test for `is_pure_reference`** with nested `${` (e.g., `"${a${b}}"`) — while the current implementation handles this correctly, a test would document the expected behavior
- **No test for `compress_file` when the input file doesn't exist** — the function will return an `Err` from `File::open`, but this path isn't tested
- **Integration test for `EnvSource` through `ConfigBuilder`** — the env source is only tested in isolation; no integration test verifies it works through the full build pipeline

---

## Summary

| Category | Rating |
|----------|--------|
| Architecture | Excellent |
| Error handling | Excellent |
| Code clarity | Excellent |
| Test coverage | Very good |
| Safety (no panics) | Excellent |
| Documentation | Good |

The codebase is mature for its stage. The design constraints are consistently enforced, the error hierarchy is clean, and the test suite is comprehensive. The issues identified are minor refinements, not structural problems.
