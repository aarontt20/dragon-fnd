# Size-Based File Rotation, Compression, and Inline Retention

## Context

dragon-fnd's logging subsystem currently rotates files only on time boundaries (daily/hourly) via `tracing_appender::rolling`. Retention cleanup runs only at startup. This leaves gaps: no size-based rotation (a bursty app can produce unbounded files within a single time window), no compression of old logs, and no inline retention (long-running servers accumulate files between restarts).

We're adding two orthogonal fields to `FileConfig`: `max_bytes: Option<u64>` and `compress: bool`. Tonight's scope implements the size-only and time-only paths. Composing both (time + size) is deferred as a validation error.

## Files to Modify

| File | Change |
|---|---|
| `Cargo.toml` | Add `flate2` and `time` as optional deps behind `logging` |
| `src/logging/error.rs` | Add `InvalidRotation(String)` variant |
| `src/logging/config.rs` | Add `max_bytes`, `compress` fields to `FileConfig` + defaults + tests |
| `src/logging/builder.rs` | Add `.max_bytes()`, `.compress()` to `FileBuilder` + tests |
| `src/logging/writer.rs` | **New file**: `SizeRotatingWriter` implementing `std::io::Write` |
| `src/logging/retain.rs` | **New file**: extract `cleanup_old_logs` from `init.rs` to avoid backward dependency |
| `src/logging/init.rs` | Validation updates, `build_file_layer` branching, call `retain::cleanup_old_logs` |
| `src/logging/mod.rs` | Register `mod writer` and `mod retain` |
| `tests/logging_init.rs` | Integration tests for size rotation through `AppContext` |

## Implementation Order

### 1. `Cargo.toml` — add dependencies

```toml
flate2 = { version = "1", optional = true }
time = { version = "0.3", features = ["formatting", "macros", "std"], optional = true }
```

Update feature: `logging = ["dep:tracing", "dep:tracing-subscriber", "dep:tracing-appender", "dep:flate2", "dep:time"]`

`flate2` defaults use the pure-Rust `miniz_oxide` backend (`rust_backend` feature) — no need to override. `time` requires the `std` feature for `OffsetDateTime::now_utc()` — this works without it today only because Cargo feature unification inherits `std` from the default features, but stating it explicitly prevents breakage if someone adds `default-features = false` later. Verify with `cargo tree --features logging` that `time` is already a transitive dep of `tracing-subscriber` (it is via `tracing-appender`).

### 2. `src/logging/error.rs` — add `InvalidRotation`

Add one variant:

```rust
#[error("invalid rotation config: {0}")]
InvalidRotation(String),
```

No `IoError(#[from])` — the writer's `new()` maps `io::Error` explicitly to `FileSetupFailed` for context. The `Write` impl returns `io::Result` natively.

### 3. `src/logging/config.rs` — add fields

Add to `FileConfig`:
- `max_bytes: Option<u64>` — defaults to `None`
- `compress: bool` — defaults to `false`

Add serde tests for new fields. Update `full_toml_round_trip` test.

### 4. `src/logging/builder.rs` — add fluent methods

Add to `FileBuilder`:
- `.max_bytes(bytes: u64) -> Self` — sets `max_bytes = Some(bytes)`
- `.compress(compress: bool) -> Self` — sets `compress`

Add unit tests for the new methods and a full-chain test.

### 5. `src/logging/retain.rs` — extract retention logic

Extract `cleanup_old_logs` from `init.rs` into a new `retain.rs` module. This avoids a backward dependency (`writer` → `init`) — both `init.rs` and `writer.rs` now depend on the neutral `retain` module instead.

Function signature and behavior are unchanged. Visibility becomes `pub(crate)`.

**Also migrate the 4 existing `cleanup_old_logs_*` unit tests** from `init.rs` to `retain.rs`'s `#[cfg(test)] mod tests`. After extraction, `use super::*` in `init.rs` tests no longer brings `cleanup_old_logs` into scope, so these tests must move:
- `cleanup_old_logs_days_retention`
- `cleanup_old_logs_files_retention`
- `cleanup_old_logs_nonexistent_dir`
- `cleanup_old_logs_non_matching_prefix_ignored`

**Register `mod retain;` in `mod.rs` now** (not at step 8) — step 6 (`writer.rs`) calls `retain::cleanup_old_logs` and won't compile without the module registration.

### 6. `src/logging/writer.rs` — the custom writer (core)

`SizeRotatingWriter` struct:
- Fields: `file: File`, `bytes_written: u64`, `max_bytes: u64`, `dir: PathBuf`, `prefix: String`, `compress: bool`, `retain_days: Option<u32>`, `retain_files: Option<u32>`
- `new()` → opens/creates `{dir}/{prefix}` via `OpenOptions::new().create(true).append(true).open()`, recovers byte count from existing file via `metadata().len()`
- `Write::write()` → delegates to inner `File::write()`, increments counter by the **actual bytes written** (the return value, not `buf.len()`), if `bytes_written >= max_bytes` → calls `rotate()`
- `Write::flush()` → delegates to inner file
- No custom `Drop` needed — `File` close on drop suffices. No rotation is triggered on drop because a partial file below threshold is a valid active file, and the byte count is recovered on next `new()`.

Rotation sequence:
1. Flush current file
2. Rename `{prefix}` → `{prefix}.{YYYYMMDDTHHmmss.SSS}` (UTC via `time::OffsetDateTime::now_utc()`, format via `format_description!("[year][month][day]T[hour][minute][second].[subsecond digits:3]")`)
3. Handle timestamp collision: append `.1`, `.2` etc. if target exists. This is a TOCTOU check, but since `non_blocking` enforces a single-writer model, there is no concurrent rotation. The only collision source is rapid rotation within the same millisecond.
4. Open new `{prefix}`, reset counter
5. If `compress`: spawn a background thread to gzip the rotated file, then delete original
6. Run retention via `retain::cleanup_old_logs()` (best-effort, `eprintln!` on failure)

**Background compression** (step 5):
```rust
if self.compress {
    let path = rotated_path.clone();
    std::thread::spawn(move || {
        if let Err(e) = compress_file(&path) {
            eprintln!("dragon-fnd: failed to compress {}: {e}", path.display());
        }
    });
}
```

The rotated file is fully independent from the writer after rename — no shared state, no synchronization needed. The spawned thread owns a `PathBuf`, reads the file, writes `{path}.gz`, deletes the original. Edge cases:
- **Rotate again before compression finishes**: each spawn gets a different file path — no conflict.
- **Retention runs while compression is in-flight**: retention counts the uncompressed file. Next rotation, retention sees the `.gz` instead. Self-correcting, off by at most 1 file temporarily.
- **Retention deletes the file being compressed**: POSIX handles this — `unlink` on an open fd removes the directory entry, compression thread keeps reading through its handle. Its `remove_file` on the original gets `NotFound`, which is silently ignored.
- **Process exits mid-compression**: detached thread dies, partial `.gz` may remain alongside the original. Retention cannot detect that a `.gz` is partial — it matches the prefix and has a recent mtime, so it will be kept as a "newest" file. This is a known limitation (see below), not self-correcting.

Design notes:
- **Write-then-check**: complete the write, then rotate if over threshold. File may slightly exceed `max_bytes` by one write call. This avoids splitting log lines across files.
- **Track actual bytes written**: `Write::write()` returns the number of bytes the inner `File` accepted. The counter tracks this return value, not `buf.len()`, so the byte count stays accurate even on short writes.
- **`eprintln!` with `"dragon-fnd: "` prefix for all writer errors**: the writer IS the tracing writer — using `tracing::warn!` would recurse. Matches existing `eprintln!` convention in `init.rs`.
- **All errors from `rotate()` are soft**: `eprintln!` warning, then continue writing to the current (over-threshold) file. `non_blocking` silently swallows `io::Error` from `write()`, so a "hard error" would just mean silent log loss with no diagnostic. `eprintln!` is strictly more observable.

### 7. `src/logging/init.rs` — validation and integration

**New validation rules** (inside `if config.file.enabled` block). All new rules run **before** the existing retention rules, so rotation-related misconfigurations are caught first:

```rust
// --- new rules (in this order) ---
1. max_bytes.is_some_and(|b| b < 4096) → InvalidRotation("max_bytes must be at least 4096")
2. max_bytes.is_some() && rotation != Never → InvalidRotation("...not yet supported")
3. compress && rotation != Never → InvalidRotation("...not yet supported")
4. compress && max_bytes.is_none() && rotation == Never → InvalidRotation("compress requires rotation to be enabled")

// --- existing retention rules (updated) ---
5. retain_days == Some(0) → InvalidRetention (unchanged)
6. retain_files == Some(0) → InvalidRetention (unchanged)
7. rotation == Never && max_bytes.is_none() && (retain_days || retain_files) → InvalidRetention (updated)
```

The ordering matters: `compress=true, max_bytes=None, rotation=Never, retain_files=5` hits rule 4 (`InvalidRotation`) before reaching rule 7 (`InvalidRetention`). This is the correct error — the compress misconfiguration is the root cause.

**Update existing retention rule** (rule 7): `Rotation::Never` with retention is only invalid when `max_bytes` is also `None`. With `max_bytes` set, rotation produces files so retention is valid.

**Update `build_file_layer()`**: branch on `max_bytes` for both the startup retention cleanup and the appender/writer creation. Filter construction, fmt layer, and format match remain shared below:

```rust
// Startup retention: only for time-based rotation (size-based handles it inline)
let cleanup_errors = if config.max_bytes.is_none()
    && config.rotation != Rotation::Never
    && (config.retain_days.is_some() || config.retain_files.is_some())
{
    retain::cleanup_old_logs(&config.dir, &config.prefix, config.retain_days, config.retain_files)
} else {
    Vec::new()
};

// Writer/appender creation
let (non_blocking, guard) = if let Some(max_bytes) = config.max_bytes {
    let writer = SizeRotatingWriter::new(...)?;
    tracing_appender::non_blocking(writer)
} else {
    let appender = match config.rotation { ... };
    tracing_appender::non_blocking(appender)
};
// filter construction, fmt layer, format match all remain below, shared by both paths
```

**Replace `cleanup_old_logs` call** with `retain::cleanup_old_logs` (function has moved to `retain.rs`). Delete the old private function from this file.

### 8. `src/logging/mod.rs` — register `mod writer`

Add `mod writer;`. (`mod retain;` was already added in step 5 as a prerequisite for step 6.) No new public exports — `SizeRotatingWriter` and `cleanup_old_logs` are `pub(crate)`.

### 9. Tests

**Writer unit tests** (`writer.rs`):
- `new` creates active file, recovers byte count from existing file
- Write increments counter by actual bytes written (return value)
- Rotation triggered at threshold, produces timestamped rotated file
- Rotation with compression produces `.gz`, deletes original (poll/sleep for async compression)
- Multiple rapid rotations produce unique filenames (verify `.1`, `.2` suffixes appear)
- Retention runs inline after rotation
- `compress_file` produces valid gzip (decompress and verify)
- Timestamp format is correct shape
- Rotation failure (read-only dir) → `eprintln!` warning, subsequent writes continue to over-threshold file
- Compressed `.gz` files are counted by `retain_files` retention

**Validation tests** (`init.rs`):
- `max_bytes` too small → `InvalidRotation`
- `max_bytes` with `Daily` → `InvalidRotation`
- `compress` with `Hourly` → `InvalidRotation`
- `compress` with `Never` and no `max_bytes` → `InvalidRotation` (silent no-op prevention)
- `Never` + `max_bytes` + `retain_files` → allowed (no error)
- `Never` + `max_bytes` + `retain_days` → allowed (no error — `retain_days` works for size-rotated files too)
- `Never` + no `max_bytes` + `retain_files` → `InvalidRetention` (unchanged)

**Existing test update** (`init.rs`):
- `build_file_layer_creates_directory` — add `max_bytes: None, compress: false` to the `FileConfig` struct literal (adding new fields breaks this test without the update)

**Integration tests** (`tests/logging_init.rs`):
- `build_sync` with size rotation creates dir and active file
- `build_sync` with size rotation + compress + retain_files succeeds, assert old files cleaned up and remaining files are `.gz`
- Invalid `max_bytes` + time rotation errors through `AppContext`

## Verification

```bash
cargo test --features logging           # all tests pass
cargo clippy --features logging         # no warnings
cargo build                             # builds without logging feature (no compile errors)
cargo doc --features logging            # docs generate cleanly
```

## Not in Scope (deferred)

- Composing time + size rotation (both triggers active simultaneously)
- Compression with time-based rotation (requires hooking `tracing_appender` rotation events)
- Count-based suffix cascade (`.1` → `.2` → `.3`)
- `local-offset` for `time` crate — use UTC timestamps to avoid the multi-threaded Linux soundness issue

## Known Limitations

- **Compression is async**: background thread means `.gz` may not exist immediately after rotation. Retention count may be off by ±1 file temporarily (self-correcting on next rotation).
- **Partial `.gz` after process kill**: if the process is killed mid-compression, a partial `.gz` file may remain. Retention cannot detect it as corrupt — it matches the prefix and has a recent mtime, so it is kept as a "newest" file. This is **not self-correcting**. The partial `.gz` persists until it ages out via `retain_days` or is displaced by enough new rotations to push it past the `retain_files` limit. Detecting corrupt `.gz` files (e.g., by attempting to read the gzip footer) is deferred as out of scope.
- **`non_blocking` drops records silently**: if the writer is slow (e.g., slow filesystem), the bounded channel fills and log records are dropped with no indication. This is inherent to `tracing_appender::non_blocking`, not specific to size rotation. Compression is deferred to a background thread specifically to avoid exacerbating this.
- **Collision check is TOCTOU**: the timestamp collision fallback (`.1`, `.2`) checks existence then renames. Safe in practice because `non_blocking` enforces single-writer, but not safe against external processes writing to the same log directory with the same prefix.
- **`Rotation` enum name**: currently represents time-based rotation only. When time+size composition is eventually added, the field name `rotation` may need to become `time_rotation` — a breaking API change. Accepted for now since composition is deferred.
