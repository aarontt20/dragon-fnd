# Test Coverage Documentation

Tests are inline (`#[cfg(test)]` modules in source files) plus integration tests in `tests/`.

**No features: 95 unit + 25 integration + 5 doc-tests = 125 tests**
**With `http`: 150 unit + 49 integration + 6 doc-tests = 205 tests**
**With `shutdown`: 124 unit + 37 integration + 5 doc-tests = 166 tests**
**With `sqlite`: 125 unit + 40 integration + 5 doc-tests = 170 tests**
**With `logging`: 149 unit + 35 integration + 5 doc-tests = 189 tests**
**With `http,sqlite,logging`: 234 unit + 74 integration + 6 doc-tests = 314 tests**

---

## Module: `config::source` (27 tests)

Tests for `merge_at_path()`, `ConfigEntry` constructors, `ConfigValue`/`ConfigTable` types, and value conversions.

### `merge_at_path`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `merge_at_empty_path_deep_merges` | Deep merge at root | Empty path with table value performs recursive merge; existing keys preserved, new keys added, nested tables merged (not replaced) |
| `merge_at_empty_path_non_table_returns_error` | Root non-table error | Empty path with non-table value (e.g., integer) returns `ConfigError::RootNotTable` |
| `merge_at_empty_path_overlay_replaces_scalar` | Scalar overlay | Empty path with table replaces existing scalar at same key |
| `merge_at_path_creates_intermediates` | Path navigation | Non-existent intermediate tables are created automatically when merging at deep paths like `["a", "b", "c"]` |
| `merge_at_path_replaces_leaf` | Scalar replacement | Non-table values at leaf positions are replaced entirely |
| `merge_at_path_merges_tables_at_leaf` | Table merge at leaf | When both existing and new values are tables at the target path, they are deep-merged |
| `merge_at_path_scalar_replaces_table_at_leaf` | Scalar replaces table | Scalar at leaf replaces existing table |
| `merge_at_path_type_conflict_at_intermediate` | Type conflict | Existing non-table value at an intermediate path segment returns `ConfigError::TypeConflict` |
| `merge_at_path_type_conflict_preserves_full_path` | Conflict path | `TypeConflict` error includes the full path to the conflict point |
| `merge_at_path_type_conflict_three_levels_deep` | Deep conflict | Type conflict detected 3 levels deep in path navigation |

### `ConfigEntry`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `config_entry_constructors` | Constructor methods | `ConfigEntry::root()` creates entry with empty path; `ConfigEntry::at_path()` creates entry with specified path segments |

### `ConfigValue` and `ConfigTable`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `config_value_string_constructor` | String constructor | `ConfigValue::string("hello")` creates `String` variant |
| `config_value_integer_constructor` | Integer constructor | `ConfigValue::integer(42)` creates `Integer` variant |
| `config_value_float_constructor` | Float constructor | `ConfigValue::float(3.14)` creates `Float` variant |
| `config_value_boolean_constructor` | Boolean constructor | `ConfigValue::boolean(true)` creates `Boolean` variant |
| `config_value_datetime_constructor` | Datetime constructor | `ConfigValue::datetime("2024-01-01T00:00:00Z")` returns `Ok(Datetime)` variant |
| `config_value_datetime_constructor_rejects_invalid` | Datetime validation | `ConfigValue::datetime("not-a-datetime")` returns `Err(InvalidDatetime)` |
| `config_table_new_and_insert` | Table constructor | `ConfigTable::new()` + `insert()` builds tables with chaining |
| `config_value_datetime_round_trip` | Datetime conversion | Valid datetime string survives `ConfigValue → toml::Value → ConfigValue` round-trip |
| `config_value_datetime_invalid_returns_error` | Datetime rejection | Invalid datetime string returns `ConfigError::InvalidDatetime` from both constructor and `into_toml_value()` |

### `ConfigValue ↔ toml::Value` conversion

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `config_value_to_toml_value_scalars` | Scalar conversion | String, Integer, Float, Boolean all convert correctly to `toml::Value` |
| `config_value_to_toml_value_array` | Array conversion | `ConfigValue::Array` converts to `toml::Value::Array` |
| `config_value_to_toml_value_table` | Table conversion | `ConfigValue::Table` converts to `toml::Value::Table` |
| `toml_value_to_config_value_scalars` | Reverse scalars | `toml::Value` scalars (String, Integer, Float, Boolean) convert to `ConfigValue` |
| `toml_value_to_config_value_array` | Reverse array | `toml::Value::Array` converts to `ConfigValue::Array` |
| `toml_value_to_config_value_nested_table` | Reverse table | Nested `toml::Value::Table` converts to nested `ConfigValue::Table` |

---

## Module: `config::file` (4 tests)

Tests for `FileSource` TOML file loading.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `file_source_loads_valid_file` | Happy path | Valid TOML file is parsed and returned as single root-level `ConfigEntry` with parsed table |
| `file_source_required_missing` | Required file error | Missing file with `required=true` returns `ConfigError::FileNotFound` |
| `file_source_optional_missing` | Optional file skip | Missing file with `required=false` returns empty entries vector (no error) |
| `file_source_parse_error` | Parse error | Invalid TOML syntax returns `ConfigError::ParseError` |

### Coverage Gaps

- `ConfigError::ReadError` (I/O errors other than NotFound) - not tested (platform-specific: requires permission denied)

---

## Module: `config::env` (15 tests)

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

### `EnvSource` (8 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `env_source_basic` | Basic loading | Env vars with prefix are captured; string and integer coercion works |
| `env_source_nested` | Nested paths | `PREFIX__A__B` maps to path `["a", "b"]`; multiple nested vars work together |
| `env_source_case_conversion` | Case normalization | Path segments converted to lowercase (`UPPER_CASE` → `upper_case`) |
| `env_source_ignores_unrelated` | Prefix isolation | Only vars matching exact prefix+separator are captured; `APP3EXTRA__` not matched by `APP3__` |
| `env_source_empty_path_ignored` | Empty path skip | `PREFIX__` (no path after separator) is silently ignored |
| `env_source_custom_separator` | Separator config | Custom separator (e.g., `_` instead of `__`) works correctly |
| `env_source_empty_separator_returns_error` | Validation | Empty separator returns `ConfigError::InvalidSeparator` from `entries()` |
| `env_source_empty_prefix_returns_error` | Validation | Empty prefix returns `ConfigError::InvalidPrefix` from `entries()` |
| `env_source_empty_segment_error` | Empty segment | Consecutive separators (e.g., `APP__A____B`) return `ConfigError::EmptyPathSegment` |

### Test Helpers

- `EnvGuard`: RAII helper that sets env vars and removes them on drop to prevent test pollution
- All env-mutating tests use `#[serial]` from `serial_test` to prevent data races

---

## Module: `config::serde_source` (14 tests)

Tests for `SerdeSource` — serializing `T: Serialize` structs into the config pipeline.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `basic_struct` | Happy path | Simple struct serializes correctly, table contains expected keys/values |
| `nested_struct` | Nested structs | Nested structs map to nested TOML tables |
| `none_fields_omitted` | None omission | `Option::None` fields absent from table (regression guard for toml crate behavior) |
| `some_fields_present` | Some values | `Option::Some(v)` fields present with unwrapped value |
| `mixed_none_and_some` | Mixed options | Only Some values appear in output; None values absent |
| `all_none_nested` | All-None inner struct | Inner struct with all-None produces empty sub-table |
| `empty_struct` | Empty struct | Struct with no fields produces empty table (no-op merge) |
| `vec_fields` | Vec fields | `Vec<T>` fields serialize as TOML arrays |
| `non_struct_errors` | Non-struct rejection | Bare scalar/vec returns `ConfigError::SerializeError` |
| `u64_overflow_errors` | u64 overflow | `u64::MAX` returns `ConfigError::SerializeError` (TOML integers are i64) |
| `enum_with_data_serializes_as_nested_table` | Enum with data | Externally-tagged enum with data serializes as nested tables |
| `unit_enum_variants` | Unit enums | Unit enum variants serialize as strings |
| `nested_option_some_none_omitted` | Nested option | `Option<Option<T>>` with `Some(None)` is omitted (UnsupportedNone catch handles nested Options) |
| `entries_returns_single_root` | ConfigSource impl | Exactly one entry with empty path returned |

---

## Integration Tests: `config_serde_source` (7 tests)

End-to-end tests for `SerdeSource` through the full config pipeline (`tests/config_serde_source.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `serde_source_standalone` | Standalone | SerdeSource as sole source through full pipeline |
| `file_then_serde_override` | File + override | File defaults + SerdeSource overrides; None fields keep file values |
| `serde_source_with_nested_structs` | Nested deep merge | Nested structs deep-merge correctly against file; None fields in inner structs preserve file values |
| `serde_source_with_variable_references` | Pipeline sanity | Existing `${...}` resolution works when SerdeSource participates in source stack |
| `serde_source_serialize_error` | Error at new() | Non-struct type produces `ConfigError::SerializeError` at `new()`, before `build()` |
| `three_way_merge` | Multi-layer | Three source layers merge correctly with registration-order priority |
| `serde_source_with_vec_fields` | Vec roundtrip | Vec fields survive the full `T → toml::Table → merge → T'` roundtrip |

---

## Module: `config::resolve` (36 tests)

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

### Error Cases (13 tests)

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
| `empty_reference_rejected` | Empty ref | `${}` → `InvalidReferencePath` |

### Coverage Notes

- All `ConfigError` variants for resolve are tested
- Graph-based cycle detection catches all cycle types immediately
- Full value substitution tested for all TOML types (int, float, bool, array, table)
- Escape sequences processed in all strings containing `$$` (both with and without references)

---

## Module: `logging::config` (12 tests, feature: `logging`)

Tests for serde deserialization of logging config types including `RotationStrategy`.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `logging_config_defaults` | Default values | All defaults correct: enabled, info filter, console on, file off |
| `full_toml_round_trip` | Complete config | Full TOML with all fields deserializes correctly |
| `format_variants_parse` | LogFormat | `"pretty"`, `"json"`, `"compact"` all parse |
| `rotation_variants_parse` | RotationStrategy | `"daily"`, `"hourly"`, `"never"` all parse as RotationStrategy variants |
| `size_rotation_parses` | Size rotation | `rotation = "size"` with `max_bytes` deserializes to `RotationStrategy::SizeBased { max_bytes }` |
| `retain_days_only` | Retention | `retain_days` without `retain_files` parses |
| `retain_files_only` | Retention | `retain_files` without `retain_days` parses |
| `console_filter_override_parses` | Per-layer filter | Console filter override parses correctly |
| `file_filter_override_parses` | Per-layer filter | File filter override parses correctly |
| `compress_parses` | Compression | `compress` field deserializes as `bool` |
| `size_rotation_without_max_bytes_errors` | Validation | `rotation = "size"` without `max_bytes` returns deserialize error |
| `unknown_rotation_strategy_errors` | Validation | Unknown rotation string returns deserialize error |

---

## Module: `logging::builder` (11 tests, feature: `logging`)

Tests for fluent builder API.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `new_matches_config_defaults` | Default alignment | `LoggingBuilder::new()` matches `LoggingConfig::default()` |
| `from_config_round_trips` | Config bridge | `from_config()` + `into_config()` round-trips all fields |
| `fluent_overrides` | Builder methods | `filter()`, `module()`, `enabled()` modify config correctly |
| `console_builder_overrides` | Console builder | `format()`, `filter()`, `enabled()` on ConsoleBuilder |
| `file_builder_enables_by_default` | Auto-enable | `FileBuilder::new(dir)` sets `enabled = true` |
| `file_builder_overrides` | File builder | All FileBuilder methods set correct fields |
| `file_builder_compress` | Compression | `compress()` sets the field correctly |
| `file_builder_size_rotation_full_chain` | Full chain | `SizeBased + compress + retain_files` compose correctly |
| `retain_days_clears_retain_files` | Mutual exclusion | `retain_days()` after `retain_files()` clears files |
| `retain_files_clears_retain_days` | Mutual exclusion | `retain_files()` after `retain_days()` clears days |
| `console_and_file_compose_into_logging_builder` | Composition | Full builder chain with console + file produces correct config |

---

## Module: `logging::init` (17 tests, feature: `logging`)

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
| `retention_validation_never_rotation_with_retention` | Validation | `RotationStrategy::Never` + retention returns `InvalidRetention` |
| `rotation_validation_max_bytes_too_small` | Validation | `SizeBased { max_bytes < 4096 }` returns `InvalidRotation` |
| `rotation_validation_compress_without_rotation` | Validation | `compress` with `Never` rotation returns `InvalidRotation` |
| `rotation_validation_size_based_with_retain_files_allowed` | Validation | `SizeBased` + `retain_files` is valid |
| `rotation_validation_size_based_with_retain_days_allowed` | Validation | `SizeBased` + `retain_days` is valid |
| `rotation_validation_compress_with_size_based_allowed` | Validation | `compress` + `SizeBased` is valid |
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

## Integration Tests: `logging_init` (10 tests, feature: `logging`)

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
| `build_sync_with_size_rotation_creates_dir_and_active_file` | Size rotation | `RotationStrategy::SizeBased` creates directory and active log file |
| `build_sync_with_size_rotation_and_compress_and_retain` | Full chain | `SizeBased` + compress + retain_files compose through AppContext |

---

## Module: `http::config` (7 tests, feature: `http`)

Tests for `HttpConfig` defaults, TOML deserialization, and address formatting.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `defaults` | Default values | Host is `"0.0.0.0"`, port is `8080` |
| `custom_values` | Custom config | Custom host and port set correctly |
| `addr_string` | Address formatting | `addr_string()` returns `"{host}:{port}"` |
| `deserialize_defaults` | TOML defaults | Empty TOML produces default config |
| `deserialize_full` | Full TOML | Both fields specified in TOML; parsed correctly |
| `clone` | Clone trait | Cloned config matches original |
| `debug` | Debug trait | Debug output contains `"HttpConfig"` |

---

## Module: `http::builder` (7 tests, feature: `http`)

Tests for `HttpBuilder` fluent API.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `new_defaults` | Default alignment | `HttpBuilder::new()` matches default config (0.0.0.0:8080) |
| `from_config_preserves_values` | Config bridge | `from_config()` + `into_config()` round-trips all fields |
| `fluent_chain` | Builder methods | `host()` and `port()` modify config correctly |
| `host_accepts_string_types` | Generic host | `host()` accepts both `&str` and `String` |
| `clone` | Clone trait | Cloned builder produces matching config |
| `debug` | Debug trait | Debug output contains `"HttpBuilder"` |
| `default_trait` | Default impl | `HttpBuilder::default()` matches `HttpBuilder::new()` |

---

## Module: `http::error` (7 tests, feature: `http`)

Tests for `HttpError` display output and error source chains.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `bind_failed_display` | Display output | `BindFailed` displays `"failed to bind to {addr}"`; has source |
| `bind_failed_source_chain` | Source chain | `BindFailed` source returns the inner I/O error |
| `already_serving_display` | Display output | `AlreadyServing` displays correct message; no source |
| `shutdown_required_display` | Display output | `ShutdownRequired` displays correct message; no source |
| `serve_failed_display` | Display output | `ServeFailed` displays `"HTTP server error"`; has source |
| `debug` | Debug trait | Debug output contains variant name |
| `top_level_error_from_http` | Error conversion | `HttpError` converts to top-level `Error` via `From` with `"http error:"` prefix |

---

## Module: `http::init` (5 tests, feature: `http`)

Tests for `init_http()` and the `Http` handle.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `init_binds_to_port_0` | Bind to ephemeral | Port 0 binds successfully, assigned port is non-zero |
| `local_addr_available_before_serve` | Address access | `local_addr()` returns correct IP and non-zero port before `serve()` |
| `serve_twice_returns_already_serving` | Double serve | Second `serve()` call returns `HttpError::AlreadyServing` |
| `debug_output` | Debug trait | Debug output contains `"Http"`, `"local_addr"`, and `"listener: true"` |
| `debug_after_serve` | Post-serve debug | After `serve()`, debug shows `"listener: false"` |

---

## Integration Tests: `http` (12 tests, feature: `http`)

End-to-end tests for HTTP subsystem through the `AppContext` builder (`tests/http.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `appcontext_with_http` | Build with HTTP | `ctx.http()` returns `Some`, `ctx.shutdown()` returns `Some` |
| `appcontext_without_http` | Build without HTTP | Feature enabled but `with_http()` not called — `ctx.http()` returns `None` |
| `bind_to_port_0_assigns_port` | Ephemeral port | Port 0 assigns a real port; IP matches configured host |
| `bind_failure` | Bind collision | Binding to an already-used port returns `HttpError::BindFailed` |
| `missing_shutdown_returns_error` | Missing dependency | `with_http()` without `with_shutdown()` returns `HttpError::ShutdownRequired` |
| `serve_and_shutdown` | Serve lifecycle | Trigger shutdown then serve — `serve()` returns `Ok(())` |
| `serve_twice_returns_already_serving` | Double serve | Second `serve()` call returns `HttpError::AlreadyServing` |
| `config_deserialization` | TOML config | `HttpConfig` deserializes from TOML and bridges to builder |
| `context_debug_with_http` | Context debug | `AppContext` debug includes `"http: true"` and `"shutdown: true"` |
| `builder_debug_with_http` | Builder debug | `AppContextBuilder` debug includes `"http: true"` and `"shutdown: true"` |
| `serve_with_router_and_programmatic_shutdown` | Full e2e | Spawn serve, connect via TCP, trigger shutdown, verify serve returns Ok |
| `local_addr_stable_after_serve` | Address stability | `local_addr()` returns same address before and after `serve()` |

---

## Module: `sqlite::error` (4 tests, feature: `sqlite`)

Tests for `SqliteError` display output and conversion to top-level `Error`.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `empty_path_display` | Display output | `EmptyPath` displays "database path cannot be empty"; has no source |
| `migrations_dir_not_found_display` | Display output | `MigrationsDirNotFound` includes the path in message |
| `directory_creation_failed_display` | Display + source | `DirectoryCreationFailed` includes dir path; `source()` returns the I/O error |
| `top_level_error_from_sqlite` | Error conversion | `SqliteError` converts to top-level `Error` via `From` with "sqlite error:" prefix |

---

## Module: `sqlite::config` (9 tests, feature: `sqlite`)

Tests for `SqliteConfig` defaults, TOML deserialization, and `JournalMode` enum.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `defaults` | Default values | All 10 fields have correct defaults: empty path, 5 max/1 min connections, WAL, foreign_keys on, 5s busy timeout |
| `deserialize_minimal_toml` | Minimal config | Only `path` specified; all other fields use defaults |
| `deserialize_full_toml` | Full config | All 10 fields specified in TOML; all parsed correctly |
| `deserialize_empty_table` | Empty table | Empty TOML string produces `SqliteConfig::default()` |
| `journal_mode_wal` | JournalMode variant | `"wal"` deserializes to `JournalMode::Wal` |
| `journal_mode_delete` | JournalMode variant | `"delete"` deserializes to `JournalMode::Delete` |
| `journal_mode_memory` | JournalMode variant | `"memory"` deserializes to `JournalMode::Memory` |
| `journal_mode_invalid` | JournalMode rejection | `"truncate"` returns deserialization error |
| `memory_path` | Memory path | `":memory:"` parsed as valid path string |

---

## Module: `sqlite::builder` (5 tests, feature: `sqlite`)

Tests for `SqliteBuilder` fluent API.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `new_sets_path_with_defaults` | Constructor | `new("test.db")` sets path; all other fields are defaults |
| `from_config_preserves_values` | Config bridge | `from_config()` + `into_config()` round-trips all fields unchanged |
| `fluent_chain` | Full chain | All 9 builder methods set correct fields in fluent chain |
| `new_accepts_string_types` | Generic path | `new()` accepts both `&str` and `String` |
| `memory_database` | Memory path | `new(":memory:")` sets path to `:memory:` |

---

## Module: `sqlite::init` (8 tests, feature: `sqlite`)

Tests for `init_pool()` — pool creation, PRAGMA verification, directory creation, and migrations.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `init_memory_database` | Happy path | In-memory pool created; `SELECT 1` succeeds |
| `empty_path_rejected` | Validation | Empty path returns `SqliteError::EmptyPath` |
| `foreign_keys_enabled` | PRAGMA | `foreign_keys: true` → `PRAGMA foreign_keys` returns 1 |
| `foreign_keys_disabled` | PRAGMA | `foreign_keys: false` → `PRAGMA foreign_keys` returns 0 |
| `busy_timeout_is_set` | PRAGMA | `busy_timeout_secs: 10` → `PRAGMA busy_timeout` returns 10000 (milliseconds) |
| `migrations_dir_missing` | Error path | Missing migrations directory returns `SqliteError::MigrationsDirNotFound` |
| `file_based_creates_directory` | Dir creation | Non-existent parent directory created automatically for file-based database |
| `migrations_run_successfully` | Migrations | SQL migration file creates table; `SELECT COUNT(*)` succeeds on migrated table |

---

## Integration Tests: `sqlite` (9 tests, feature: `sqlite`)

End-to-end tests for SQLite subsystem through the `AppContext` builder (`tests/sqlite.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `memory_pool_creation` | Memory pool | In-memory pool via `AppContext::builder()` → `with_sqlite()` → `build().await` |
| `file_pool_creation` | File pool | File-based pool creates database file on disk |
| `pragma_foreign_keys_enabled` | PRAGMA e2e | Foreign keys enabled through full builder chain |
| `pragma_foreign_keys_disabled` | PRAGMA e2e | Foreign keys disabled through full builder chain |
| `pragma_busy_timeout` | PRAGMA e2e | Busy timeout (7s → 7000ms) verified through full builder chain |
| `pragma_journal_mode_wal_requires_file` | WAL mode | WAL journal mode verified on file-based database |
| `migrations_from_directory` | Migrations e2e | Migration creates table; insert + count verifies schema |
| `from_config_via_toml` | Config bridge | TOML → `SqliteConfig` → `SqliteBuilder::from_config()` → working pool |
| `sqlite_pool_reexport_is_usable` | Re-export | `dragon_fnd::sqlite::SqlitePool` type alias works without direct sqlx dependency |

---

## Integration Tests: `context_async` (6 tests, feature: `sqlite`)

Tests for `AppContextBuilder` async type-state with SQLite (`tests/context_async.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `async_builder_with_config_and_sqlite` | Full chain | `with_config()` → `with_sqlite()` → `build().await` produces context with both |
| `sqlite_before_config` | Builder ordering | `with_sqlite()` before `with_config()` works — registration order doesn't matter |
| `async_build_without_sqlite` | Pool presence | Registered sqlite pool is `Some` via `ctx.sqlite()` |
| `async_build_with_extensions` | Extensions + async | Extensions survive async build alongside sqlite |
| `async_context_debug` | Debug impl | `AppContext` debug output includes `sqlite_pool: true` |
| `builder_debug_with_sqlite` | Builder debug | `AppContextBuilder` debug output includes `sqlite: true` |

---

## Integration Tests: `config_builder` (10 tests)

End-to-end tests for `ConfigBuilder` (`tests/config_builder.rs`). Custom sources use `ConfigValue` constructors.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_with_file_deserializes` | File → typed config | TOML file loaded and deserialized into struct |
| `builder_multiple_sources_override` | Source ordering | Later sources override earlier sources |
| `builder_with_custom_source` | Custom `ConfigSource` | User-defined source with `ConfigValue` integrates via `with_source()` |
| `builder_resolves_references` | Variable resolution | `${path}` references resolved during `build()` |
| `builder_resolves_cross_source_references` | Cross-source refs | File source + custom source references resolve across boundaries |
| `builder_error_propagates_from_source` | Error propagation | Source errors surface through `build()` |
| `builder_propagates_custom_source_error` | Custom source error | Custom `ConfigSource` returning error propagates through `build()` |
| `builder_deserialize_error_missing_field` | Deserialize error | Missing required field returns `ConfigError::DeserializeError` |
| `builder_deserialize_error_wrong_type` | Deserialize error | Wrong value type returns `ConfigError::DeserializeError` |
| `builder_no_sources` | Zero sources | Building with no sources returns `ConfigError::DeserializeError` |

---

## Integration Tests: `context` (8 tests)

Tests for the type-state `AppContext` builder and extension slot (`tests/context.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_chain_with_config_and_build_sync` | Full chain | `builder() → with_config() → build_sync()` produces working context with config accessor |
| `build_sync_returns_result` | Return type | `build_sync()` returns `Result<AppContext<C>, Error>` |
| `app_context_debug_output` | Debug impl | Manual `Debug` impl produces meaningful output |
| `extension_store_and_retrieve` | Extension basics | `with_extension()` + `extension::<T>()` stores and retrieves typed values |
| `extension_missing_returns_none` | Extension miss | `extension::<T>()` returns `None` for unregistered types |
| `extension_last_writer_wins` | Extension override | Registering same type twice keeps the last value |
| `extension_before_config` | Extension ordering | `with_extension()` works before `with_config()` |
| `extension_debug_shows_count` | Extension debug | Debug output includes extension count |

---

## Doc-Tests (6 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `AppContext` usage example | `no_run` compile check | Usage example compiles correctly |
| `AppContext` compile_fail (no config) | Type-state enforcement | `AppContext::builder().build_sync()` without `with_config()` does not compile |
| `AppContext` compile_fail (sqlite async) | Type-state enforcement | `with_sqlite().build_sync()` does not compile — must use `build().await` |
| `AppContext` compile_fail (shutdown async) | Type-state enforcement | `with_shutdown().build_sync()` does not compile — must use `build().await` |
| `Http` usage example | `no_run` compile check | HTTP usage example with serve and shutdown compiles correctly |
| `SerdeSource` priority example | `no_run` compile check | Usage example with multi-source priority compiles correctly |

---

## Summary by Coverage Level

### Well Covered
- `merge_at_path()` - all major code paths including type conflicts
- `ConfigValue`/`ConfigTable` - constructors, bidirectional conversion, datetime round-trip
- `coerce_value()` - all type coercion branches
- `EnvSource::entries()` - prefix matching, path parsing, edge cases, validation
- `resolve_references()` - comprehensive coverage including all error variants, type preservation, chaining
- `SerdeSource` - struct serialization, None omission, nested structs, enums, Vec fields, error cases, full pipeline roundtrip
- `ConfigBuilder` - file loading, source ordering, custom sources, cross-source references, error propagation
- `AppContext` - type-state builder, fallible `build_sync()`, async `build()`, compile-time enforcement, extension slot (store, retrieve, override, ordering, debug)
- `LoggingConfig` - defaults, round-trip, all format/rotation variants, retention fields, per-layer filters
- `LoggingBuilder` / `ConsoleBuilder` / `FileBuilder` - defaults, overrides, composition, mutual exclusion
- `init_logging()` - filter building, disabled path, retention validation, rotation validation, boundary tests, directory creation
- `cleanup_old_logs()` - days retention, file count retention, missing dir, prefix filtering
- `SizeRotatingWriter` - creation, byte recovery, write tracking, rotation threshold, compression, rapid rotations, timestamp format, inline retention, gz retention
- `SqliteConfig` / `JournalMode` - defaults, TOML deserialization (minimal, full, empty), all journal mode variants, invalid mode, memory path
- `SqliteBuilder` - constructor, config round-trip, full fluent chain, generic path types, memory database
- `init_pool()` - memory and file pools, PRAGMA verification (foreign_keys, busy_timeout), directory creation, migrations, empty path rejection
- `SqliteError` - display output for all testable variants, source chain, top-level Error conversion
- `HttpConfig` - defaults, TOML deserialization, address formatting, clone, debug
- `HttpBuilder` - defaults, config round-trip, fluent chain, generic host types, clone, debug, Default trait
- `HttpError` - display output for all variants, source chains, top-level Error conversion
- `Http` handle - bind to ephemeral port, local_addr before serve, double-serve rejection, debug output
- `AppContext` HTTP path - build with/without http, bind failure, missing shutdown, serve lifecycle, config deserialization, debug, full e2e with TCP connect, address stability
- `AppContext` async path - config+sqlite builder chain, registration ordering, extensions with async, debug output, compile-time `build_sync()` prevention

### Partially Covered
- `FileSource` - happy path, not-found, and parse error; missing I/O error case (platform-specific)
- `SqliteError` - `PoolCreationFailed`, `ConnectivityTestFailed`, `MigrationFailed` display/source tested indirectly via integration tests but not unit-tested (require real sqlx errors)

### Not Covered
- Error display strings (tested indirectly via `matches!` but not asserted on text) — exception: `SqliteError` display strings are explicitly asserted
