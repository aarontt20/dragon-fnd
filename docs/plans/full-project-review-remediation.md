# Plan: Full Project Review Remediation

## Context

The full project review identified 16 findings across architecture, code quality, Rust idioms, and simplification. This plan addresses 13 of them (3 were "keep private / no change needed"). The fixes are grouped into 7 commits ordered by dependency to maintain a green test suite at every step.

User decisions confirmed:
- Leading zeros in integer coercion: **reject** (match TOML rules)
- `from_config` signature: **take ownership**
- `$$` escape: **fix the inconsistency**
- `FileSource`/`EnvSource` exports: **keep private**

---

## Commit 1: Safety — Guard path indexing + `merge_at_path` returns Result

**Fixes:** #1 (constraint violation: panic risk) + #2 (constraint violation: silent drop)

### `src/config/resolve.rs`

**Lines 172-205:** Change `get_value` and `get_value_mut` to use `split_first()` instead of `path[0]`:

```rust
let (first, rest) = path.split_first().ok_or_else(err)?;
let mut current = table.get(first).ok_or_else(err)?;
for segment in rest { ... }
```

Add test: `get_value_empty_path_returns_error`

### `src/config/error.rs`

Add variant after `DeserializeError`:

```rust
#[error("root-level config entry must be a table, got {0}")]
RootNotTable(String),
```

### `src/config/source.rs`

**Lines 28-58:** Change `merge_at_path` signature to `-> Result<(), ConfigError>`. Add a `value_kind()` helper. Return `Err(ConfigError::RootNotTable(...))` instead of silent `return`.

Add test: `merge_at_empty_path_non_table_returns_error`. Update existing tests to add `.unwrap()`.

### `src/config/builder.rs`

**Line 41:** Add `?` to `merge_at_path(...)` call.

---

## Commit 2: Error quality — Cycle path in `CircularReference`

**Fix:** #3 (CircularReference has no path info)

### `src/config/error.rs`

Change variant:
```rust
#[error("circular reference detected: {}", .0.join(" -> "))]
CircularReference(Vec<String>),
```

### `src/config/resolve.rs`

**Lines 108-155:** Change `in_progress` from `HashSet` to `Vec` (stack) to preserve DFS ordering. On cycle detection, reconstruct the cycle from the stack position where the node first appeared:

```rust
if let Some(pos) = stack.iter().position(|n| *n == node) {
    let cycle: Vec<String> = stack[pos..]
        .iter()
        .chain(std::iter::once(&node))
        .map(|p| p.join("."))
        .collect();
    return Err(ConfigError::CircularReference(cycle));
}
```

Update 3 test assertions (`circular_reference_detected`, `self_reference_detected`, `three_way_cycle_detected`) to match on `CircularReference(cycle)` and verify cycle contents.

---

## Commit 3: Behavioral fixes — Leading zeros + `$$` escape

**Fixes:** #4 (leading zeros) + #5 (`$$` inconsistency)

### `src/config/env.rs`

**Lines 77-80:** Add leading-zero rejection to `looks_like_integer`:

```rust
if s.len() > 1 && s.starts_with('0') { return false; }
```

Update test at line 138: `"007"` now returns `String("007")`. Add test `coerce_leading_zero_stays_string`.

### `src/config/resolve.rs`

**Escape fix strategy:** Extend `collect_from_value` to also track "escape-only" paths (strings with `$$` but no `${...}`). Process those with a simple `replace("$$", "$")` after graph-based resolution.

Changes:
1. Add `escape_only: &mut Vec<ConfigPath>` param to `collect_from_table`, `collect_from_value`
2. In `collect_from_value`: when `parse_references` returns empty but string contains `$$`, push path to `escape_only`
3. Update `collect_references` return type to `(Vec<(ConfigPath, ConfigPath)>, Vec<ConfigPath>)`
4. Add `resolve_escapes_at_path(table, path)` — gets the string, replaces `$$` with `$`, writes it back
5. In `resolve_references`: after the topological sort loop, iterate `escape_only` paths and call `resolve_escapes_at_path`

Update test `string_without_references_unchanged` → rename to `escape_without_references_is_processed`, assert `"$not_a_ref"`. Add test `escape_only_in_array`.

---

## Commit 4: API improvements — `from_config` ownership + `with_config` generic

**Fixes:** #6 (from_config takes &T then clones) + #7 (with_config not generic over A)

### `src/logging/builder.rs`

**Lines 25-29:** Change to `pub fn from_config(config: LoggingConfig) -> Self { Self { config } }`.

Update test `from_config_round_trips` to pass owned value. Update `tests/logging_init.rs` lines 97, 112 to pass owned values.

### `src/context/mod.rs`

**Line 128:** Change `impl AppContextBuilder<NoConfig, SyncBuild>` to `impl<A> AppContextBuilder<NoConfig, A>`. Return type becomes `AppContextBuilder<Configured<C>, A>`. Zero behavioral change.

---

## Commit 5: Remove `#[from]` from `DeserializeError`

**Fix:** #8 (footgun: `#[from]` on shared error type)

### `src/config/error.rs`

**Line 23:** Remove `#[from]` from `DeserializeError(toml::de::Error)`. Both call sites already use explicit `map_err`. Add doc comment to `ParseError` explaining it's manually constructed with file path context.

---

## Commit 6: Code quality — PartialEq, display fix, inline filter

**Fixes:** #9 (derive PartialEq) + #10 (redundant `{source}`) + #11 (inline resolve_layer_filter)

### `src/logging/config.rs`

Add `PartialEq` to derive on `LoggingConfig`, `ConsoleConfig`, `FileConfig`. Simplify `new_matches_config_defaults` test to single `assert_eq!`.

### `src/logging/error.rs`

**Line 13:** Remove `: {source}` from `FileSetupFailed` display format.

### `src/logging/init.rs`

**Lines 117-125:** Delete `resolve_layer_filter`. Inline at both call sites (lines 59 and 186) — use `EnvFilter::try_new()` directly for overrides. Delete the two unit tests for the removed function.

---

## Commit 7: Cleanup — Inline `collect_references`, documentation

**Fixes:** #12 (thin wrapper) + #13 (doc accuracy)

### `src/config/resolve.rs`

Inline `collect_references` body into `resolve_references`. Delete the wrapper function. All three phases (collect, sort, resolve) plus escape processing now visible in one function.

### `docs/DESIGN.md`

- Known Limitations: Clarify `get_value` (internal, supports arrays) vs `lookup_value` (user-facing, tables only)
- ConfigError table: Update to reflect new variants (`RootNotTable`, `CircularReference(Vec<String>)`) — now 12 variants
- LoggingError table: Update `FileSetupFailed` display text (no `: {source}`)

### `src/config/error.rs`

Add doc comment to `ParseError` explaining distinction from `DeserializeError`.

---

## Documentation updates (across commits)

After all fixes, update:
- `CLAUDE.md`: test counts (will change with new tests)
- `TEST.md`: add new test entries, update counts
- `DOC.md`: update `ConfigError` variant list (add `RootNotTable`)

---

## Verification

After each commit:
```bash
cargo test --features logging    # all tests pass
cargo test                       # without logging, no regressions
cargo clippy --features logging  # zero warnings
cargo clippy                     # zero warnings
```

After all commits:
```bash
cargo doc --features logging     # documentation generates cleanly
```

---

## Files Modified (by commit)

| Commit | Files |
|--------|-------|
| 1 | `config/error.rs`, `config/source.rs`, `config/resolve.rs`, `config/builder.rs` |
| 2 | `config/error.rs`, `config/resolve.rs` |
| 3 | `config/env.rs`, `config/resolve.rs` |
| 4 | `logging/builder.rs`, `context/mod.rs`, `tests/logging_init.rs` |
| 5 | `config/error.rs` |
| 6 | `logging/config.rs`, `logging/error.rs`, `logging/init.rs`, `logging/builder.rs` |
| 7 | `config/resolve.rs`, `config/error.rs`, `docs/DESIGN.md`, `CLAUDE.md`, `TEST.md`, `DOC.md` |
