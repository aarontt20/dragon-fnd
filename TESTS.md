# TESTS.md - Test Coverage Documentation

This file documents all unit tests that were in the codebase. Total: **28 tests** across 4 test modules.

---

## src/config/source.rs (5 tests)

### test_merge_at_empty_path_deep_merges

**Purpose:** Verifies that merging at an empty path performs deep merge at root level.

**What it tests:**
- Existing keys are preserved after merge
- New keys are added from overlay
- Nested tables are merged recursively (not replaced entirely)

**Test logic:**
```rust
let mut base = Table::new();
base.insert("existing".into(), Value::String("keep".into()));

let mut nested = Table::new();
nested.insert("inner".into(), Value::Integer(42));
base.insert("nested".into(), Value::Table(nested));

let mut overlay = Table::new();
overlay.insert("new".into(), Value::String("added".into()));

let mut overlay_nested = Table::new();
overlay_nested.insert("another".into(), Value::Boolean(true));
overlay.insert("nested".into(), Value::Table(overlay_nested));

merge_at_path(&mut base, &[], Value::Table(overlay));

// Existing key preserved
assert_eq!(base.get("existing"), Some(&Value::String("keep".into())));
// New key added
assert_eq!(base.get("new"), Some(&Value::String("added".into())));
// Nested tables merged, not replaced
let nested = base.get("nested").unwrap().as_table().unwrap();
assert_eq!(nested.get("inner"), Some(&Value::Integer(42)));
assert_eq!(nested.get("another"), Some(&Value::Boolean(true)));
```

---

### test_merge_at_path_creates_intermediates

**Purpose:** Verifies that merging at a nested path creates intermediate tables.

**What it tests:**
- Non-existent intermediate tables are created automatically
- Value is placed at correct nested location

**Test logic:**
```rust
let mut table = Table::new();

merge_at_path(
    &mut table,
    &["a".into(), "b".into(), "c".into()],
    Value::Integer(123),
);

let a = table.get("a").unwrap().as_table().unwrap();
let b = a.get("b").unwrap().as_table().unwrap();
assert_eq!(b.get("c"), Some(&Value::Integer(123)));
```

---

### test_merge_at_path_replaces_leaf

**Purpose:** Verifies that merging at an existing key replaces the value.

**What it tests:**
- Scalar values are replaced entirely

**Test logic:**
```rust
let mut table = Table::new();
table.insert("key".into(), Value::String("old".into()));

merge_at_path(&mut table, &["key".into()], Value::String("new".into()));

assert_eq!(table.get("key"), Some(&Value::String("new".into())));
```

---

### test_merge_at_path_merges_tables_at_leaf

**Purpose:** Verifies that merging tables at a leaf path performs deep merge.

**What it tests:**
- When both existing and overlay are tables, they are merged
- Existing keys in nested table are preserved

**Test logic:**
```rust
let mut table = Table::new();
let mut existing = Table::new();
existing.insert("a".into(), Value::Integer(1));
table.insert("config".into(), Value::Table(existing));

let mut overlay = Table::new();
overlay.insert("b".into(), Value::Integer(2));

merge_at_path(&mut table, &["config".into()], Value::Table(overlay));

let config = table.get("config").unwrap().as_table().unwrap();
assert_eq!(config.get("a"), Some(&Value::Integer(1)));
assert_eq!(config.get("b"), Some(&Value::Integer(2)));
```

---

### test_config_entry_constructors

**Purpose:** Verifies ConfigEntry constructor methods work correctly.

**What it tests:**
- `ConfigEntry::root()` creates entry with empty path
- `ConfigEntry::at_path()` creates entry with specified path

**Test logic:**
```rust
let root = ConfigEntry::root(Table::new());
assert!(root.path.is_empty());

let at_path = ConfigEntry::at_path(vec!["a".into(), "b".into()], Value::Integer(42));
assert_eq!(at_path.path, vec!["a", "b"]);
```

---

## src/config/file.rs (3 tests)

### test_file_source_loads_valid_file

**Purpose:** Verifies FileSource correctly loads and parses a valid TOML file.

**What it tests:**
- FileSource returns one entry for a valid file
- Entry has empty path (root-level)
- Content is correctly parsed

**Test logic:**
```rust
let mut file = NamedTempFile::new().unwrap();
writeln!(file, "key = \"value\"").unwrap();

let source = FileSource::new(file.path(), true);
let entries = source.entries().unwrap();

assert_eq!(entries.len(), 1);
assert!(entries[0].path.is_empty());
let table = entries[0].value.as_table().unwrap();
assert_eq!(
    table.get("key"),
    Some(&toml::Value::String("value".into()))
);
```

---

### test_file_source_required_missing

**Purpose:** Verifies FileSource returns error when required file is missing.

**What it tests:**
- Returns `ConfigError::FileNotFound` for missing required file

**Test logic:**
```rust
let source = FileSource::new("/nonexistent/path/config.toml", true);
let result = source.entries();

assert!(matches!(result, Err(ConfigError::FileNotFound(_))));
```

---

### test_file_source_optional_missing

**Purpose:** Verifies FileSource gracefully handles missing optional file.

**What it tests:**
- Returns empty entries for missing optional file (no error)

**Test logic:**
```rust
let source = FileSource::new("/nonexistent/path/config.toml", false);
let entries = source.entries().unwrap();

assert!(entries.is_empty());
```

---

## src/config/env.rs (12 tests)

### Test Helper: EnvGuard

A RAII helper struct that sets environment variables for tests and cleans them up on drop.

```rust
struct EnvGuard {
    keys: Vec<String>,
}

impl EnvGuard {
    fn new() -> Self { Self { keys: Vec::new() } }
    fn set(&mut self, key: &str, value: &str) {
        env::set_var(key, value);
        self.keys.push(key.to_string());
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for key in &self.keys {
            env::remove_var(key);
        }
    }
}
```

---

### test_coerce_integer

**Purpose:** Verifies integer coercion from strings.

**What it tests:**
- Positive integers: "42" -> Integer(42)
- Negative integers: "-123" -> Integer(-123)
- Zero: "0" -> Integer(0)

**Test logic:**
```rust
assert_eq!(coerce_value("42"), Value::Integer(42));
assert_eq!(coerce_value("-123"), Value::Integer(-123));
assert_eq!(coerce_value("0"), Value::Integer(0));
```

---

### test_coerce_float

**Purpose:** Verifies float coercion from strings.

**What it tests:**
- Positive floats: "3.14" -> Float(3.14)
- Negative floats: "-2.5" -> Float(-2.5)
- Zero float: "0.0" -> Float(0.0)

**Test logic:**
```rust
assert_eq!(coerce_value("3.14"), Value::Float(3.14));
assert_eq!(coerce_value("-2.5"), Value::Float(-2.5));
assert_eq!(coerce_value("0.0"), Value::Float(0.0));
```

---

### test_coerce_boolean

**Purpose:** Verifies boolean coercion from strings (case-insensitive).

**What it tests:**
- "true" -> Boolean(true)
- "false" -> Boolean(false)
- "TRUE" -> Boolean(true)
- "False" -> Boolean(false)

**Test logic:**
```rust
assert_eq!(coerce_value("true"), Value::Boolean(true));
assert_eq!(coerce_value("false"), Value::Boolean(false));
assert_eq!(coerce_value("TRUE"), Value::Boolean(true));
assert_eq!(coerce_value("False"), Value::Boolean(false));
```

---

### test_coerce_string

**Purpose:** Verifies string fallback and edge cases.

**What it tests:**
- Plain text stays as string: "hello" -> String("hello")
- Text with spaces: "hello world" -> String("hello world")
- Leading zeros parsed as integer: "007" -> Integer(7)

**Test logic:**
```rust
assert_eq!(coerce_value("hello"), Value::String("hello".to_string()));
assert_eq!(coerce_value("hello world"), Value::String("hello world".to_string()));
assert_eq!(coerce_value("007"), Value::Integer(7));
```

---

### test_coerce_edge_cases

**Purpose:** Verifies edge cases in value coercion.

**What it tests:**
- Empty string stays as string
- Just a minus sign stays as string
- Invalid float format stays as string

**Test logic:**
```rust
assert_eq!(coerce_value(""), Value::String("".to_string()));
assert_eq!(coerce_value("-"), Value::String("-".to_string()));
assert_eq!(coerce_value("1.2.3"), Value::String("1.2.3".to_string()));
```

---

### test_env_source_basic

**Purpose:** Verifies basic environment variable loading.

**What it tests:**
- Variables with correct prefix are loaded
- Path is extracted correctly
- Values are coerced to appropriate types

**Test logic:**
```rust
guard.set("TESTAPP2__HOST", "localhost");
guard.set("TESTAPP2__PORT", "8080");

let source = EnvSource::new("TESTAPP2", "__");
let entries = source.entries().unwrap();

let host_entry = entries.iter().find(|e| e.path == vec!["host"]);
let port_entry = entries.iter().find(|e| e.path == vec!["port"]);

assert_eq!(host_entry.map(|e| &e.value), Some(&Value::String("localhost".to_string())));
assert_eq!(port_entry.map(|e| &e.value), Some(&Value::Integer(8080)));
```

---

### test_env_source_nested

**Purpose:** Verifies nested path handling with multiple separators.

**What it tests:**
- Multiple separators create nested paths
- Different nested prefixes are correctly parsed

**Test logic:**
```rust
guard.set("MYAPP2__DATABASE__HOST", "db.example.com");
guard.set("MYAPP2__DATABASE__PORT", "5432");
guard.set("MYAPP2__SERVER__ENABLED", "true");

let source = EnvSource::new("MYAPP2", "__");
let entries = source.entries().unwrap();

let db_host = entries.iter().find(|e| e.path == vec!["database", "host"]);
let db_port = entries.iter().find(|e| e.path == vec!["database", "port"]);
let srv_enabled = entries.iter().find(|e| e.path == vec!["server", "enabled"]);

assert_eq!(db_host.map(|e| &e.value), Some(&Value::String("db.example.com".to_string())));
assert_eq!(db_port.map(|e| &e.value), Some(&Value::Integer(5432)));
assert_eq!(srv_enabled.map(|e| &e.value), Some(&Value::Boolean(true)));
```

---

### test_env_source_case_conversion

**Purpose:** Verifies uppercase to lowercase conversion in paths.

**What it tests:**
- Path segments are converted to lowercase

**Test logic:**
```rust
guard.set("APP2__UPPER_CASE__NESTED_KEY", "value");

let source = EnvSource::new("APP2", "__");
let entries = source.entries().unwrap();

let entry = entries.iter().find(|e| e.path == vec!["upper_case", "nested_key"]);

assert_eq!(entry.map(|e| &e.value), Some(&Value::String("value".to_string())));
```

---

### test_env_source_ignores_unrelated

**Purpose:** Verifies only variables with correct prefix are loaded.

**What it tests:**
- Variables without matching prefix are ignored
- Partial prefix matches are ignored

**Test logic:**
```rust
guard.set("APP3__KEY", "value");
guard.set("OTHER3__KEY", "ignored");
guard.set("APP3EXTRA__KEY", "also_ignored");

let source = EnvSource::new("APP3", "__");
let entries = source.entries().unwrap();

assert_eq!(entries.len(), 1);
assert_eq!(entries[0].path, vec!["key"]);
```

---

### test_env_source_empty_path_ignored

**Purpose:** Verifies empty paths are ignored.

**What it tests:**
- Variable with only prefix and separator (no path) is ignored

**Test logic:**
```rust
guard.set("APP4__", "value");

let source = EnvSource::new("APP4", "__");
let entries = source.entries().unwrap();

assert!(entries.is_empty());
```

---

### test_env_source_custom_separator

**Purpose:** Verifies custom separator support.

**What it tests:**
- Single underscore separator works correctly

**Test logic:**
```rust
guard.set("APP5_DB_HOST", "localhost");

let source = EnvSource::new("APP5", "_");
let entries = source.entries().unwrap();

let entry = entries.iter().find(|e| e.path == vec!["db", "host"]);
assert_eq!(entry.map(|e| &e.value), Some(&Value::String("localhost".to_string())));
```

---

### test_env_source_empty_separator_panics

**Purpose:** Verifies panic on empty separator.

**What it tests:**
- Creating EnvSource with empty separator panics

**Attributes:** `#[should_panic(expected = "separator must not be empty")]`

**Test logic:**
```rust
let _ = EnvSource::new("APP", "");
```

---

## src/config/resolve.rs (8 tests)

### Test Helper: make_table

Parses a TOML string into a Table for testing.

```rust
fn make_table(toml_str: &str) -> Table {
    toml::from_str(toml_str).unwrap()
}
```

---

### test_simple_reference

**Purpose:** Verifies basic `${path}` substitution.

**What it tests:**
- Simple variable reference is resolved
- Reference is replaced with actual value

**Test logic:**
```rust
let mut table = make_table(r#"
    host = "localhost"
    url = "http://${host}/api"
"#);
resolve_references(&mut table).unwrap();
assert_eq!(table["url"].as_str().unwrap(), "http://localhost/api");
```

---

### test_nested_path

**Purpose:** Verifies nested path references like `${server.host}`.

**What it tests:**
- Dotted path references work correctly
- Multiple references in same string are resolved

**Test logic:**
```rust
let mut table = make_table(r#"
    [server]
    host = "example.com"
    port = 8080

    [client]
    endpoint = "https://${server.host}:${server.port}"
"#);
resolve_references(&mut table).unwrap();
assert_eq!(
    table["client"]["endpoint"].as_str().unwrap(),
    "https://example.com:8080"
);
```

---

### test_chained_references

**Purpose:** Verifies multi-level reference chains.

**What it tests:**
- References that reference other references work
- Iterative resolution handles chains

**Test logic:**
```rust
let mut table = make_table(r#"
    a = "hello"
    b = "${a} world"
    c = "${b}!"
"#);
resolve_references(&mut table).unwrap();
assert_eq!(table["c"].as_str().unwrap(), "hello world!");
```

---

### test_escape_sequence

**Purpose:** Verifies `$$` escape sequence produces literal `$`.

**What it tests:**
- Double dollar sign is converted to single dollar sign
- Escaped references are not resolved

**Test logic:**
```rust
let mut table = make_table(r#"
    value = "use $${VAR} for env vars"
"#);
resolve_references(&mut table).unwrap();
assert_eq!(
    table["value"].as_str().unwrap(),
    "use ${VAR} for env vars"
);
```

---

### test_integer_coercion

**Purpose:** Verifies integer values are converted to string in references.

**What it tests:**
- Integer values can be referenced and become strings

**Test logic:**
```rust
let mut table = make_table(r#"
    port = 3000
    url = "http://localhost:${port}"
"#);
resolve_references(&mut table).unwrap();
assert_eq!(table["url"].as_str().unwrap(), "http://localhost:3000");
```

---

### test_circular_reference

**Purpose:** Verifies circular reference detection.

**What it tests:**
- Returns `ConfigError::CircularReference` for circular refs

**Test logic:**
```rust
let mut table = make_table(r#"
    a = "${b}"
    b = "${a}"
"#);
let result = resolve_references(&mut table);
assert!(matches!(result, Err(ConfigError::CircularReference)));
```

---

### test_missing_reference

**Purpose:** Verifies error on nonexistent path references.

**What it tests:**
- Returns `ConfigError::ReferenceNotFound` for missing paths

**Test logic:**
```rust
let mut table = make_table(r#"
    url = "${nonexistent.path}"
"#);
let result = resolve_references(&mut table);
assert!(matches!(result, Err(ConfigError::ReferenceNotFound(_))));
```

---

### test_array_values

**Purpose:** Verifies reference resolution in array elements.

**What it tests:**
- References inside arrays are resolved

**Test logic:**
```rust
let mut table = make_table(r#"
    base = "/api"
    endpoints = ["${base}/users", "${base}/posts"]
"#);
resolve_references(&mut table).unwrap();
let endpoints = table["endpoints"].as_array().unwrap();
assert_eq!(endpoints[0].as_str().unwrap(), "/api/users");
assert_eq!(endpoints[1].as_str().unwrap(), "/api/posts");
```

---

## Summary by Module

| Module | Tests | Coverage Areas |
|--------|-------|----------------|
| `source.rs` | 5 | Merge semantics, path handling, entry constructors |
| `file.rs` | 3 | File loading, required vs optional files |
| `env.rs` | 12 | Value coercion, path mapping, filtering, separators |
| `resolve.rs` | 8 | Variable resolution, escaping, error handling |
| **Total** | **28** | |

## Test Dependencies

- `tempfile` crate (for `file.rs` tests)
- Standard library `std::env` (for `env.rs` tests)
