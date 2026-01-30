# Test Coverage Documentation

This document describes the test coverage for dragon-fnd. Tests were removed from source files to keep implementation code focused.

**Total: 28 unit tests**

---

## Module: `config::source` (5 tests)

Tests for `merge_at_path()` and `ConfigEntry` constructors.

### `merge_at_path`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_merge_at_empty_path_deep_merges` | Deep merge at root | Empty path with table value performs recursive merge; existing keys preserved, new keys added, nested tables merged (not replaced) |
| `test_merge_at_path_creates_intermediates` | Path navigation | Non-existent intermediate tables are created automatically when merging at deep paths like `["a", "b", "c"]` |
| `test_merge_at_path_replaces_leaf` | Scalar replacement | Non-table values at leaf positions are replaced entirely |
| `test_merge_at_path_merges_tables_at_leaf` | Table merge at leaf | When both existing and new values are tables at the target path, they are deep-merged |

### `ConfigEntry`

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_config_entry_constructors` | Constructor methods | `ConfigEntry::root()` creates entry with empty path; `ConfigEntry::at_path()` creates entry with specified path segments |

---

## Module: `config::file` (3 tests)

Tests for `FileSource` TOML file loading.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_file_source_loads_valid_file` | Happy path | Valid TOML file is parsed and returned as single root-level `ConfigEntry` with parsed table |
| `test_file_source_required_missing` | Required file error | Missing file with `required=true` returns `ConfigError::FileNotFound` |
| `test_file_source_optional_missing` | Optional file skip | Missing file with `required=false` returns empty entries vector (no error) |

### Coverage Gaps

- `ConfigError::ReadError` (I/O errors other than NotFound) - not tested
- `ConfigError::ParseError` (invalid TOML syntax) - not tested

---

## Module: `config::env` (12 tests)

Tests for `EnvSource` environment variable loading and `coerce_value()` type coercion.

### `coerce_value` (6 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_coerce_integer` | Integer parsing | Positive, negative, and zero integers parsed correctly |
| `test_coerce_float` | Float parsing | Positive, negative, and zero floats (containing `.`) parsed correctly |
| `test_coerce_boolean` | Boolean parsing | `true`/`false` recognized case-insensitively (`TRUE`, `False`, etc.) |
| `test_coerce_string` | String fallback | Non-numeric/boolean strings kept as-is; note: `"007"` parses as integer 7 |
| `test_coerce_edge_cases` | Edge cases | Empty string → string; lone `-` → string; `"1.2.3"` (invalid float) → string |

### `EnvSource` (6 tests)

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_env_source_basic` | Basic loading | Env vars with prefix are captured; string and integer coercion works |
| `test_env_source_nested` | Nested paths | `PREFIX__A__B` maps to path `["a", "b"]`; multiple nested vars work together |
| `test_env_source_case_conversion` | Case normalization | Path segments converted to lowercase (`UPPER_CASE` → `upper_case`) |
| `test_env_source_ignores_unrelated` | Prefix isolation | Only vars matching exact prefix+separator are captured; `APP3EXTRA__` not matched by `APP3__` |
| `test_env_source_empty_path_ignored` | Empty path skip | `PREFIX__` (no path after separator) is silently ignored |
| `test_env_source_custom_separator` | Separator config | Custom separator (e.g., `_` instead of `__`) works correctly |
| `test_env_source_empty_separator_panics` | Validation | Empty separator panics with message "separator must not be empty" |

### Test Helpers

- `EnvGuard`: RAII helper that sets env vars and removes them on drop to prevent test pollution

---

## Module: `config::resolve` (8 tests)

Tests for `${path.to.field}` variable reference resolution.

| Test | Coverage | Behavior Verified |
|------|----------|-------------------|
| `test_simple_reference` | Basic substitution | `${host}` in string replaced with value of `host` key |
| `test_nested_path` | Dotted paths | `${server.host}` navigates nested tables correctly |
| `test_chained_references` | Multi-pass resolution | References that resolve to strings containing more references are iteratively resolved |
| `test_escape_sequence` | Escape handling | `$$` produces literal `$`; `$${VAR}` becomes `${VAR}` |
| `test_integer_coercion` | Non-string values | Integer values converted to string when referenced |
| `test_circular_reference` | Cycle detection | Circular references (`a="${b}"`, `b="${a}"`) return `ConfigError::CircularReference` after max iterations |
| `test_missing_reference` | Not found error | Reference to non-existent path returns `ConfigError::ReferenceNotFound` |
| `test_array_values` | Array traversal | References inside array elements are resolved |

### Coverage Gaps

- `ConfigError::InvalidReferencePath` (empty path segments like `${a..b}`) - not tested
- `ConfigError::NonScalarReference` (referencing table/array values) - not tested
- `ConfigError::UnclosedReference` (missing `}`) - not tested
- Float, boolean, datetime value coercion in references - not directly tested
- Lone `$` without `{` or second `$` - not tested (falls through to literal `$`)

---

## Module: `config::builder`

No unit tests. Tested indirectly through integration of source modules.

### Coverage Gaps

- `Config::builder()` / `with_file()` / `with_env()` / `with_source()` builder chain
- `Config::build()` end-to-end deserialization
- Error propagation from sources to build result

---

## Module: `context`

No unit tests.

### Coverage Gaps

- `AppContext::builder()` / `with_config()` / `build()` builder chain
- `AppContext::config()` accessor
- `Error::MissingConfig` when building without config

---

## Module: `error`

No unit tests. Error types tested indirectly via other module tests.

---

## Summary by Coverage Level

### Well Covered
- `merge_at_path()` - all major code paths
- `coerce_value()` - all type coercion branches
- `EnvSource::entries()` - prefix matching, path parsing, edge cases
- `resolve_references()` - substitution, chaining, cycles, errors

### Partially Covered
- `FileSource` - happy path and not-found; missing I/O error and parse error cases
- `resolve.rs` error variants - only CircularReference, ReferenceNotFound tested

### Not Covered
- `Config` builder (no direct tests)
- `AppContext` and `AppContextBuilder` (no tests)
- Several `ConfigError` variants never triggered in tests
