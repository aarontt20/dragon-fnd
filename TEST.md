# Test Coverage Documentation

Tests are inline (`#[cfg(test)]` modules in source files) plus integration tests in `tests/`.

**Without logging feature: 58 unit tests + 8 integration tests + 2 doc-tests = 68 tests**
**With logging feature: 113 unit tests + 19 integration tests + 2 doc-tests = 134 tests**

---

## Module: `config::source` (6 tests)

Tests for `merge_at_path()` and `ConfigEntry` constructors.

### `merge_at_path`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `merge_at_empty_path_deep_merges` | Deep merge at root | Empty path with table value performs recursive merge; existing keys preserved, new keys added, nested tables merged (not replaced) |
| `merge_at_empty_path_non_table_returns_error` | Root non-table error | Empty path with non-table value (e.g., integer) returns `ConfigError::RootNotTable` |
| `merge_at_path_creates_intermediates` | Path navigation | Non-existent intermediate tables are created automatically when merging at deep paths like `["a", "b", "c"]` |
| `merge_at_path_replaces_leaf` | Scalar replacement | Non-table values at leaf positions are replaced entirely |
| `merge_at_path_merges_tables_at_leaf` | Table merge at leaf | When both existing and new values are tables at the target path, they are deep-merged |

### `ConfigEntry`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `config_entry_constructors` | Constructor methods | `ConfigEntry::root()` creates entry with empty path; `ConfigEntry::at_path()` creates entry with specified path segments |

---

## Module: `config::file` (3 tests)

Tests for `FileSource` TOML file loading.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `file_source_loads_valid_file` | Happy path | Valid TOML file is parsed and returned as single root-level `ConfigEntry` with parsed table |
| `file_source_required_missing` | Required file error | Missing file with `required=true` returns `ConfigError::FileNotFound` |
| `file_source_optional_missing` | Optional file skip | Missing file with `required=false` returns empty entries vector (no error) |

### Coverage Gaps

- `ConfigError::ReadError` (I/O errors other than NotFound) - not tested
- `ConfigError::ParseError` (invalid TOML syntax) - not tested

---

## Module: `config::env` (13 tests)

Tests for `EnvSource` environment variable loading and `coerce_value()` type coercion.

### `coerce_value` (6 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `coerce_integer` | Integer parsing | Positive, negative, and zero integers parsed correctly |
| `coerce_float` | Float parsing | Positive, negative, and zero floats (containing `.`) parsed correctly |
| `coerce_boolean` | Boolean parsing | `true`/`false` recognized case-insensitively (`TRUE`, `False`, etc.) |
| `coerce_string` | String fallback | Non-numeric/boolean strings kept as-is; `"007"` kept as string (leading zeros rejected) |
| `coerce_edge_cases` | Edge cases | Empty string → string; lone `-` → string; `"1.2.3"` (invalid float) → string |
| `coerce_leading_zero_stays_string` | Leading zero rejection | `"007"`, `"01"`, `"-01"` stay as strings; `"0"` still parses as integer 0 |

### `EnvSource` (7 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `env_source_basic` | Basic loading | Env vars with prefix are captured; string and integer coercion works |
| `env_source_nested` | Nested paths | `PREFIX__A__B` maps to path `["a", "b"]`; multiple nested vars work together |
| `env_source_case_conversion` | Case normalization | Path segments converted to lowercase (`UPPER_CASE` → `upper_case`) |
| `env_source_ignores_unrelated` | Prefix isolation | Only vars matching exact prefix+separator are captured; `APP3EXTRA__` not matched by `APP3__` |
| `env_source_empty_path_ignored` | Empty path skip | `PREFIX__` (no path after separator) is silently ignored |
| `env_source_custom_separator` | Separator config | Custom separator (e.g., `_` instead of `__`) works correctly |
| `env_source_empty_separator_returns_error` | Validation | Empty separator returns `ConfigError::InvalidSeparator` from `entries()` |

### Test Helpers

- `EnvGuard`: RAII helper that sets env vars and removes them on drop to prevent test pollution
- All env-mutating tests use `#[serial]` from `serial_test` to prevent data races

---

## Module: `config::resolve` (35 tests)

Tests for `${path.to.field}` variable reference resolution, including string interpolation,
full value substitution, graph-based dependency resolution, and error handling.

### Basic String Interpolation (4 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `simple_reference` | Basic substitution | `${name}` replaced with value of `name` key |
| `multiple_references_in_one_string` | Multiple refs | `${host}:${port}` both resolved in single string |
| `nested_path_reference` | Dotted paths | `${database.host}` navigates nested tables |
| `no_references_unchanged` | No-op case | Strings without `${...}` are not modified |

### Full Value Substitution (7 tests)

Pure references (`"${path}"` with nothing else) preserve the original type.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `pure_reference_preserves_integer` | Integer type | `"${default_port}"` becomes integer, not string |
| `pure_reference_preserves_boolean` | Boolean type | `"${debug}"` becomes boolean |
| `pure_reference_preserves_float` | Float type | `"${rate}"` becomes float |
| `pure_reference_copies_array` | Array copy | `"${tags}"` copies entire array |
| `pure_reference_copies_table` | Table copy | `"${defaults}"` copies entire table |
| `trailing_brace_is_interpolation_not_pure_reference` | Trailing `}` guard | `"${name}}"` is string interpolation producing `"world}"`, not a pure reference with path `name}` |
| `whitespace_around_reference_is_interpolation` | Whitespace significant | `"  ${value}  "` treated as string interpolation (result includes spaces), not pure substitution |

### Chained References (3 tests)

Graph-based resolution handles dependencies correctly.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `chained_references` | String chain | `a→b→c` resolved in correct order |
| `chained_pure_references` | Type-preserving chain | Pure refs through multiple levels preserve type |
| `deep_chain` | 5-level chain | `v1→v2→v3→v4→v5` resolves correctly |

### Arrays (2 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `references_in_array_elements` | String refs in array | Each array element's refs resolved |
| `pure_reference_in_array` | Type-preserving in array | Array elements can be pure refs |

### Escape Sequences (4 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `escaped_dollar_sign_with_reference` | Escape with ref | `$$` becomes `$` when string has refs |
| `mixed_escaped_and_reference` | Combined | `$$${amount}` → `$50` |
| `escape_without_references_is_processed` | Escape-only | `$$` without refs is processed to `$` |
| `escape_only_in_array` | Escape in array | `$$` in array elements processed to `$` |

### Type Coercion in Interpolation (2 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `integer_to_string_in_interpolation` | Int→string | `"port ${port}"` converts int to string |
| `boolean_to_string_in_interpolation` | Bool→string | `"debug: ${enabled}"` converts bool to string |

### Internal Functions (1 test)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `get_value_empty_path_returns_error` | Empty path guard | `get_value` with empty path returns `ReferenceNotFound` instead of panicking |

### Error Cases (12 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `circular_reference_detected` | 2-way cycle | `a="${b}", b="${a}"` → `CircularReference` |
| `self_reference_detected` | Self cycle | `a="${a}"` → `CircularReference` |
| `three_way_cycle_detected` | 3-way cycle | `a→b→c→a` → `CircularReference` |
| `reference_not_found` | Missing ref | `${nonexistent}` → `ReferenceNotFound` |
| `nested_reference_not_found` | Missing nested | `${database.port}` when port missing → `ReferenceNotFound` |
| `unclosed_reference_with_valid_reference` | Mixed valid/unclosed | String with valid + unclosed ref → `UnclosedReference` |
| `unclosed_reference_alone` | Only unclosed | `${unclosed` → `UnclosedReference` |
| `unclosed_reference_in_nested_table` | Nested unclosed | Unclosed ref in `[server]` section detected |
| `unclosed_reference_in_array` | Array unclosed | Unclosed ref in array element detected |
| `non_scalar_in_interpolation` | Table in string | `"text ${table}"` → `NonScalarReference` |
| `array_in_interpolation` | Array in string | `"items: ${array}"` → `NonScalarReference` |
| `invalid_path_empty_segment` | Bad path | `${a..b}` → `InvalidReferencePath` |

### Coverage Notes

- All `ConfigError` variants for resolve are tested
- Graph-based cycle detection catches all cycle types immediately
- Full value substitution tested for all TOML types (int, float, bool, array, table)
- Escape sequences processed in all strings containing `$$` (both with and without references)

---

## Module: `logging::config` (10 tests, feature: `logging`)

Tests for serde deserialization of logging config types.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `logging_config_defaults` | Default values | All defaults correct: enabled, info filter, console on, file off |
| `full_toml_round_trip` | Complete config | Full TOML with all fields deserializes correctly |
| `format_variants_parse` | LogFormat | `"pretty"`, `"json"`, `"compact"` all parse |
| `rotation_variants_parse` | Rotation | `"daily"`, `"hourly"`, `"never"` all parse |
| `retain_days_only` | Retention | `retain_days` without `retain_files` parses |
| `retain_files_only` | Retention | `retain_files` without `retain_days` parses |
| `console_filter_override_parses` | Per-layer filter | Console filter override parses correctly |
| `file_filter_override_parses` | Per-layer filter | File filter override parses correctly |
| `max_bytes_parses` | Size rotation | `max_bytes` field deserializes as `Option<u64>` |
| `compress_parses` | Compression | `compress` field deserializes as `bool` |

---

## Module: `logging::builder` (12 tests, feature: `logging`)

Tests for fluent builder API.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `new_matches_config_defaults` | Default alignment | `LoggingBuilder::new()` matches `LoggingConfig::default()` |
| `from_config_round_trips` | Config bridge | `from_config()` + `into_config()` round-trips all fields |
| `fluent_overrides` | Builder methods | `filter()`, `module()`, `enabled()` modify config correctly |
| `console_builder_overrides` | Console builder | `format()`, `filter()`, `enabled()` on ConsoleBuilder |
| `file_builder_enables_by_default` | Auto-enable | `FileBuilder::new(dir)` sets `enabled = true` |
| `file_builder_overrides` | File builder | All FileBuilder methods set correct fields |
| `file_builder_max_bytes` | Size rotation | `max_bytes()` sets the field correctly |
| `file_builder_compress` | Compression | `compress()` sets the field correctly |
| `file_builder_size_rotation_full_chain` | Full chain | `max_bytes + compress + retain_files` compose correctly |
| `retain_days_clears_retain_files` | Mutual exclusion | `retain_days()` after `retain_files()` clears files |
| `retain_files_clears_retain_days` | Mutual exclusion | `retain_files()` after `retain_days()` clears days |
| `console_and_file_compose_into_logging_builder` | Composition | Full builder chain with console + file produces correct config |

---

## Module: `logging::init` (19 tests, feature: `logging`)

Tests for subscriber initialization, validation rules, and layer composition.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `build_env_filter_valid_base` | Filter building | Valid base directive produces EnvFilter |
| `build_env_filter_with_modules` | Module overrides | Per-module directives appended correctly |
| `build_env_filter_invalid_directive` | Invalid filter | Malformed directive returns `InvalidFilter` |
| `build_env_filter_invalid_module` | Invalid module | Invalid module level returns error |
| `disabled_logging_returns_none` | Master switch | `enabled = false` returns `Ok(None)` |
| `retention_validation_both_set` | Validation | Both `retain_days` and `retain_files` returns `InvalidRetention` |
| `retention_validation_zero_days` | Validation | `retain_days = 0` returns `InvalidRetention` |
| `retention_validation_zero_files` | Validation | `retain_files = 0` returns `InvalidRetention` |
| `retention_validation_never_rotation_with_retention` | Validation | `Rotation::Never` without `max_bytes` + retention returns `InvalidRetention` |
| `rotation_validation_max_bytes_too_small` | Validation | `max_bytes < 4096` returns `InvalidRotation` |
| `rotation_validation_max_bytes_with_daily` | Validation | `max_bytes` + time-based rotation returns `InvalidRotation` |
| `rotation_validation_compress_with_hourly` | Validation | `compress` + time-based rotation returns `InvalidRotation` |
| `rotation_validation_compress_without_rotation` | Validation | `compress` without any rotation returns `InvalidRotation` |
| `rotation_validation_never_with_max_bytes_and_retain_files_allowed` | Validation | `max_bytes` + `retain_files` is valid (max_bytes provides rotation) |
| `rotation_validation_never_with_max_bytes_and_retain_days_allowed` | Validation | `max_bytes` + `retain_days` is valid |
| `rotation_validation_compress_with_max_bytes_allowed` | Validation | `compress` + `max_bytes` is valid |
| `rotation_validation_max_bytes_boundary_pass` | Boundary | `max_bytes = 4096` (exact minimum) succeeds |
| `rotation_validation_max_bytes_boundary_fail` | Boundary | `max_bytes = 4095` (below minimum) returns `InvalidRotation` |
| `build_file_layer_creates_directory` | Dir creation | Nested log directory created automatically |

---

## Module: `logging::retain` (4 tests, feature: `logging`)

Tests for retention cleanup of rotated log files.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `cleanup_old_logs_days_retention` | Days retention | Files older than N days are deleted |
| `cleanup_old_logs_files_retention` | File count | Only N newest files kept, oldest deleted |
| `cleanup_old_logs_nonexistent_dir` | Missing dir | Nonexistent directory returns empty errors |
| `cleanup_old_logs_non_matching_prefix_ignored` | Prefix filter | Files not matching prefix are untouched |

---

## Module: `logging::writer` (10 tests, feature: `logging`)

Tests for `SizeRotatingWriter` — size-based log file rotation with compression.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `new_creates_active_file` | Creation | Active file created, byte counter starts at 0 |
| `new_recovers_byte_count_from_existing_file` | Recovery | Existing file's size used as initial byte counter |
| `write_increments_counter` | Tracking | Each write increments `bytes_written` by actual bytes written |
| `rotation_triggered_at_threshold` | Rotation | Rotation triggered when `bytes_written >= max_bytes`; active file refreshed, rotated file contains original data |
| `multiple_rapid_rotations_produce_unique_filenames` | Collision | 5 rapid rotations produce 5 unique rotated files + 1 active |
| `timestamp_format_is_correct_shape` | Format | Rotated filename has `YYYYMMDDTHHmmss.SSS` format (19 chars) |
| `rotation_with_compression_produces_gz` | Compression | Rotated file compressed to `.gz`, original deleted |
| `compress_file_produces_valid_gzip` | Gzip | Compressed file decompresses to original content |
| `retention_runs_inline_after_rotation` | Retention | Retention cleanup runs after each rotation, keeping only N files |
| `compressed_gz_files_counted_by_retention` | Gz retention | `.gz` files are matched by prefix and counted in retention |

---

## Integration Tests: `logging_init` (11 tests, feature: `logging`)

End-to-end tests for logging integration with AppContext (`tests/logging_init.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `build_sync_with_console_logging` | Console init | Console-only logging via AppContext builder |
| `build_sync_with_logging_disabled` | Disabled | `enabled(false)` still produces valid AppContext |
| `build_sync_with_file_logging` | File init | File logging creates log directory via AppContext |
| `build_sync_without_logging_still_works` | No logging | No `with_logging()` call produces valid AppContext |
| `with_logging_before_config` | Builder ordering | `with_logging()` before `with_config()` works |
| `with_logging_after_config` | Builder ordering | `with_logging()` after `with_config()` works |
| `invalid_retention_skipped_when_file_disabled` | Validation gating | Conflicting retention values don't error when file logging is disabled |
| `invalid_retention_config_errors` | Error propagation | Retention validation errors surface through `build_sync()` |
| `build_sync_with_size_rotation_creates_dir_and_active_file` | Size rotation | Size-based rotation creates directory and active log file |
| `build_sync_with_size_rotation_and_compress_and_retain` | Full chain | Size rotation + compress + retain_files compose through AppContext |
| `invalid_max_bytes_with_time_rotation_errors_through_app_context` | Error propagation | Combining max_bytes with Daily rotation errors through `build_sync()` |

---

## Integration Tests: `config_builder` (5 tests)

End-to-end tests for `ConfigBuilder` (`tests/config_builder.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_with_file_deserializes` | File → typed config | TOML file loaded and deserialized into struct |
| `builder_multiple_sources_override` | Source ordering | Later sources override earlier sources |
| `builder_with_custom_source` | Custom `ConfigSource` | User-defined source integrates via `with_source()` |
| `builder_resolves_references` | Variable resolution | `${path}` references resolved during `build()` |
| `builder_error_propagates_from_source` | Error propagation | Source errors surface through `build()` |

---

## Integration Tests: `context` (3 tests)

Tests for the type-state `AppContext` builder (`tests/context.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_chain_with_config_and_build_sync` | Full chain | `builder() → with_config() → build_sync()` produces working context with config accessor |
| `build_sync_returns_result` | Return type | `build_sync()` returns `Result<AppContext<C>, Error>` |
| `app_context_debug_output` | Debug impl | Manual `Debug` impl produces meaningful output |

---

## Doc-Tests (2 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `AppContext` usage example | `no_run` compile check | Usage example compiles correctly |
| `AppContext` compile_fail | Type-state enforcement | `AppContext::builder().build_sync()` without `with_config()` does not compile |

---

## Summary by Coverage Level

### Well Covered
- `merge_at_path()` - all major code paths
- `coerce_value()` - all type coercion branches
- `EnvSource::entries()` - prefix matching, path parsing, edge cases, validation
- `resolve_references()` - comprehensive coverage including all error variants, type preservation, chaining
- `ConfigBuilder` - file loading, source ordering, custom sources, error propagation
- `AppContext` - type-state builder, fallible `build_sync()`, compile-time enforcement
- `LoggingConfig` - defaults, round-trip, all format/rotation variants, retention fields, per-layer filters
- `LoggingBuilder` / `ConsoleBuilder` / `FileBuilder` - defaults, overrides, composition, mutual exclusion
- `init_logging()` - filter building, disabled path, retention validation, rotation validation, boundary tests, directory creation
- `cleanup_old_logs()` - days retention, file count retention, missing dir, prefix filtering
- `SizeRotatingWriter` - creation, byte recovery, write tracking, rotation threshold, compression, rapid rotations, timestamp format, inline retention, gz retention

### Partially Covered
- `FileSource` - happy path and not-found; missing I/O error and parse error cases

### Not Covered
- Error display strings (tested indirectly via `matches!` but not asserted on text)
