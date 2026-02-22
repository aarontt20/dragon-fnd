# Test Coverage Documentation

Tests are inline (`#[cfg(test)]` modules in source files) plus integration tests in `tests/`.

**Total: 52 unit tests + 7 integration tests + 2 doc-tests = 61 tests**

---

## Module: `config::source` (5 tests)

Tests for `merge_at_path()` and `ConfigEntry` constructors.

### `merge_at_path`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `merge_at_empty_path_deep_merges` | Deep merge at root | Empty path with table value performs recursive merge; existing keys preserved, new keys added, nested tables merged (not replaced) |
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

## Module: `config::env` (12 tests)

Tests for `EnvSource` environment variable loading and `coerce_value()` type coercion.

### `coerce_value` (5 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `coerce_integer` | Integer parsing | Positive, negative, and zero integers parsed correctly |
| `coerce_float` | Float parsing | Positive, negative, and zero floats (containing `.`) parsed correctly |
| `coerce_boolean` | Boolean parsing | `true`/`false` recognized case-insensitively (`TRUE`, `False`, etc.) |
| `coerce_string` | String fallback | Non-numeric/boolean strings kept as-is; note: `"007"` parses as integer 7 |
| `coerce_edge_cases` | Edge cases | Empty string → string; lone `-` → string; `"1.2.3"` (invalid float) → string |

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

## Module: `config::resolve` (32 tests)

Tests for `${path.to.field}` variable reference resolution, including string interpolation,
full value substitution, graph-based dependency resolution, and error handling.

### Basic String Interpolation (4 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `simple_reference` | Basic substitution | `${name}` replaced with value of `name` key |
| `multiple_references_in_one_string` | Multiple refs | `${host}:${port}` both resolved in single string |
| `nested_path_reference` | Dotted paths | `${database.host}` navigates nested tables |
| `no_references_unchanged` | No-op case | Strings without `${...}` are not modified |

### Full Value Substitution (6 tests)

Pure references (`"${path}"` with nothing else) preserve the original type.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `pure_reference_preserves_integer` | Integer type | `"${default_port}"` becomes integer, not string |
| `pure_reference_preserves_boolean` | Boolean type | `"${debug}"` becomes boolean |
| `pure_reference_preserves_float` | Float type | `"${rate}"` becomes float |
| `pure_reference_copies_array` | Array copy | `"${tags}"` copies entire array |
| `pure_reference_copies_table` | Table copy | `"${defaults}"` copies entire table |
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

### Escape Sequences (3 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `escaped_dollar_sign_with_reference` | Escape with ref | `$$` becomes `$` when string has refs |
| `mixed_escaped_and_reference` | Combined | `$$${amount}` → `$50` |
| `string_without_references_unchanged` | Limitation | `$$` without refs stays as `$$` |

### Type Coercion in Interpolation (2 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `integer_to_string_in_interpolation` | Int→string | `"port ${port}"` converts int to string |
| `boolean_to_string_in_interpolation` | Bool→string | `"debug: ${enabled}"` converts bool to string |

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
- Escape sequences only processed in strings containing actual references (documented limitation)

---

## Integration Tests: `config_builder` (4 tests)

End-to-end tests for `ConfigBuilder` (`tests/config_builder.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_with_file_deserializes` | File → typed config | TOML file loaded and deserialized into struct |
| `builder_multiple_sources_override` | Source ordering | Later sources override earlier sources |
| `builder_with_custom_source` | Custom `ConfigSource` | User-defined source integrates via `with_source()` |
| `builder_error_propagates_from_source` | Error propagation | Source errors surface through `build()` |

---

## Integration Tests: `context` (3 tests)

Tests for the type-state `AppContext` builder (`tests/context.rs`).

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `builder_chain_with_config_and_build_sync` | Full chain | `builder() → with_config() → build_sync()` produces working context with config accessor |
| `build_sync_is_infallible` | Return type | `build_sync()` returns `AppContext<C>` directly, not `Result` — no `?` or `unwrap()` needed |
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
- `AppContext` - type-state builder, infallible `build_sync()`, compile-time enforcement

### Partially Covered
- `FileSource` - happy path and not-found; missing I/O error and parse error cases

### Not Covered
- Error display strings (tested indirectly via `matches!` but not asserted on text)
