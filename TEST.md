# Test Coverage Documentation

This document describes the test coverage for dragon-fnd. Tests were removed from source files to keep implementation code focused.

**Total: 32 unit tests**

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
| `pure_reference_with_whitespace` | Whitespace trim | `"  ${value}  "` still treated as pure reference |

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

- All `ConfigError` variants for resolve are now tested
- Graph-based cycle detection catches all cycle types immediately
- Full value substitution tested for all TOML types (int, float, bool, array, table)
- Escape sequences only processed in strings containing actual references (documented limitation)

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
- `resolve_references()` - comprehensive coverage including:
  - String interpolation and full value substitution
  - Graph-based dependency resolution
  - All error variants (CircularReference, ReferenceNotFound, UnclosedReference, NonScalarReference, InvalidReferencePath)
  - Type preservation for pure references
  - Chained references at multiple depths

### Partially Covered
- `FileSource` - happy path and not-found; missing I/O error and parse error cases

### Not Covered
- `Config` builder (no direct tests)
- `AppContext` and `AppContextBuilder` (no tests)
