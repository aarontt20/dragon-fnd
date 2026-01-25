//! Environment variable loading for configuration.

use toml::{Table, Value};

/// Loads environment variables with the given prefix and merges them into the config table.
///
/// Environment variables are mapped to config paths by:
/// 1. Removing the prefix
/// 2. Splitting on the separator
/// 3. Converting path segments to lowercase
///
/// For example, with prefix `"APP"` and separator `"__"`:
/// - `APP__DATABASE__HOST=localhost` → `database.host = "localhost"`
/// - `APP__SERVER__PORT=8080` → `server.port = 8080`
///
/// Values are coerced from strings to the most specific type:
/// - Integer (if all digits with optional leading `-`)
/// - Float (if contains `.` and parses successfully)
/// - Boolean (`true`/`false`, case-insensitive)
/// - String (fallback)
pub fn load_env_vars(table: &mut Table, prefix: &str, separator: &str) {
    let prefix_with_sep = format!("{prefix}{separator}");

    for (key, value) in std::env::vars() {
        if let Some(path_str) = key.strip_prefix(&prefix_with_sep) {
            if path_str.is_empty() {
                continue;
            }

            let path: Vec<&str> = path_str.split(separator).collect();
            let coerced_value = coerce_value(&value);

            insert_at_path(table, &path, coerced_value);
        }
    }
}

/// Inserts a value at the given path, creating intermediate tables as needed.
fn insert_at_path(table: &mut Table, path: &[&str], value: Value) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };

    let key = first.to_lowercase();

    if rest.is_empty() {
        table.insert(key, value);
        return;
    }

    // Ensure intermediate table exists (replace non-table values if needed)
    match table.get(&key) {
        Some(Value::Table(_)) => {}
        _ => {
            table.insert(key.clone(), Value::Table(Table::new()));
        }
    }

    if let Some(Value::Table(nested)) = table.get_mut(&key) {
        insert_at_path(nested, rest, value);
    }
}

/// Coerces a string value to the most specific TOML type.
fn coerce_value(s: &str) -> Value {
    // Try boolean first (case-insensitive)
    if s.eq_ignore_ascii_case("true") {
        return Value::Boolean(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return Value::Boolean(false);
    }

    // Try integer (only if it looks like an integer: optional minus, then digits)
    if looks_like_integer(s) {
        if let Ok(i) = s.parse::<i64>() {
            return Value::Integer(i);
        }
    }

    // Try float (if contains decimal point)
    if s.contains('.') {
        if let Ok(f) = s.parse::<f64>() {
            return Value::Float(f);
        }
    }

    // Fallback to string
    Value::String(s.to_string())
}

/// Checks if a string looks like an integer (optional minus followed by digits).
fn looks_like_integer(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Helper to set env vars for a test and clean them up after.
    struct EnvGuard {
        keys: Vec<String>,
    }

    impl EnvGuard {
        fn new() -> Self {
            Self { keys: Vec::new() }
        }

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

    #[test]
    fn test_coerce_integer() {
        assert_eq!(coerce_value("42"), Value::Integer(42));
        assert_eq!(coerce_value("-123"), Value::Integer(-123));
        assert_eq!(coerce_value("0"), Value::Integer(0));
    }

    #[test]
    fn test_coerce_float() {
        assert_eq!(coerce_value("3.14"), Value::Float(3.14));
        assert_eq!(coerce_value("-2.5"), Value::Float(-2.5));
        assert_eq!(coerce_value("0.0"), Value::Float(0.0));
    }

    #[test]
    fn test_coerce_boolean() {
        assert_eq!(coerce_value("true"), Value::Boolean(true));
        assert_eq!(coerce_value("false"), Value::Boolean(false));
        assert_eq!(coerce_value("TRUE"), Value::Boolean(true));
        assert_eq!(coerce_value("False"), Value::Boolean(false));
    }

    #[test]
    fn test_coerce_string() {
        assert_eq!(
            coerce_value("hello"),
            Value::String("hello".to_string())
        );
        assert_eq!(
            coerce_value("hello world"),
            Value::String("hello world".to_string())
        );
        // Leading zeros are allowed and parsed as decimal
        assert_eq!(coerce_value("007"), Value::Integer(7));
    }

    #[test]
    fn test_coerce_edge_cases() {
        // Empty string
        assert_eq!(coerce_value(""), Value::String("".to_string()));
        // Just a minus
        assert_eq!(coerce_value("-"), Value::String("-".to_string()));
        // Invalid float
        assert_eq!(coerce_value("1.2.3"), Value::String("1.2.3".to_string()));
    }

    #[test]
    fn test_insert_at_path_simple() {
        let mut table = Table::new();
        insert_at_path(&mut table, &["HOST"], Value::String("localhost".to_string()));

        assert_eq!(
            table.get("host"),
            Some(&Value::String("localhost".to_string()))
        );
    }

    #[test]
    fn test_insert_at_path_nested() {
        let mut table = Table::new();
        insert_at_path(
            &mut table,
            &["DATABASE", "HOST"],
            Value::String("localhost".to_string()),
        );

        let db = table.get("database").unwrap().as_table().unwrap();
        assert_eq!(db.get("host"), Some(&Value::String("localhost".to_string())));
    }

    #[test]
    fn test_insert_at_path_deeply_nested() {
        let mut table = Table::new();
        insert_at_path(
            &mut table,
            &["A", "B", "C", "D"],
            Value::Integer(42),
        );

        let a = table.get("a").unwrap().as_table().unwrap();
        let b = a.get("b").unwrap().as_table().unwrap();
        let c = b.get("c").unwrap().as_table().unwrap();
        assert_eq!(c.get("d"), Some(&Value::Integer(42)));
    }

    #[test]
    fn test_load_env_vars_basic() {
        let mut guard = EnvGuard::new();
        guard.set("TESTAPP__HOST", "localhost");
        guard.set("TESTAPP__PORT", "8080");

        let mut table = Table::new();
        load_env_vars(&mut table, "TESTAPP", "__");

        assert_eq!(
            table.get("host"),
            Some(&Value::String("localhost".to_string()))
        );
        assert_eq!(table.get("port"), Some(&Value::Integer(8080)));
    }

    #[test]
    fn test_load_env_vars_nested() {
        let mut guard = EnvGuard::new();
        guard.set("MYAPP__DATABASE__HOST", "db.example.com");
        guard.set("MYAPP__DATABASE__PORT", "5432");
        guard.set("MYAPP__SERVER__ENABLED", "true");

        let mut table = Table::new();
        load_env_vars(&mut table, "MYAPP", "__");

        let db = table.get("database").unwrap().as_table().unwrap();
        assert_eq!(
            db.get("host"),
            Some(&Value::String("db.example.com".to_string()))
        );
        assert_eq!(db.get("port"), Some(&Value::Integer(5432)));

        let server = table.get("server").unwrap().as_table().unwrap();
        assert_eq!(server.get("enabled"), Some(&Value::Boolean(true)));
    }

    #[test]
    fn test_load_env_vars_case_conversion() {
        let mut guard = EnvGuard::new();
        guard.set("APP__UPPER_CASE__NESTED_KEY", "value");

        let mut table = Table::new();
        load_env_vars(&mut table, "APP", "__");

        // Keys should be lowercase
        let upper = table.get("upper_case").unwrap().as_table().unwrap();
        assert_eq!(
            upper.get("nested_key"),
            Some(&Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_load_env_vars_ignores_unrelated() {
        let mut guard = EnvGuard::new();
        guard.set("APP__KEY", "value");
        guard.set("OTHER__KEY", "ignored");
        guard.set("APPEXTRA__KEY", "also_ignored");

        let mut table = Table::new();
        load_env_vars(&mut table, "APP", "__");

        assert_eq!(table.get("key"), Some(&Value::String("value".to_string())));
        assert!(table.get("other").is_none());
        // APPEXTRA doesn't match because prefix requires separator after it
    }

    #[test]
    fn test_load_env_vars_empty_path_ignored() {
        let mut guard = EnvGuard::new();
        // Just the prefix with separator but no path
        guard.set("APP__", "value");

        let mut table = Table::new();
        load_env_vars(&mut table, "APP", "__");

        // Should be empty - no valid path
        assert!(table.is_empty());
    }

    #[test]
    fn test_load_env_vars_overrides_existing() {
        let mut guard = EnvGuard::new();
        guard.set("CFG__PORT", "9000");

        let mut table = Table::new();
        table.insert("port".to_string(), Value::Integer(8080));

        load_env_vars(&mut table, "CFG", "__");

        // Env var should override
        assert_eq!(table.get("port"), Some(&Value::Integer(9000)));
    }

    #[test]
    fn test_load_env_vars_custom_separator() {
        let mut guard = EnvGuard::new();
        guard.set("APP_DB_HOST", "localhost");

        let mut table = Table::new();
        load_env_vars(&mut table, "APP", "_");

        let db = table.get("db").unwrap().as_table().unwrap();
        assert_eq!(db.get("host"), Some(&Value::String("localhost".to_string())));
    }
}
