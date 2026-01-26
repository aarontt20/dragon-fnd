//! Environment variable configuration source.

use toml::Value;

use super::source::{ConfigEntry, ConfigSource};
use super::ConfigError;

/// A configuration source that loads from environment variables.
///
/// Environment variables are mapped to config paths by:
/// 1. Removing the prefix and separator
/// 2. Splitting remaining segments on the separator
/// 3. Converting path segments to lowercase
///
/// For example, with prefix `"APP"` and separator `"__"`:
/// - `APP__DATABASE__HOST=localhost` -> `["database", "host"]` = "localhost"
/// - `APP__SERVER__PORT=8080` -> `["server", "port"]` = 8080
///
/// Values are coerced from strings to the most specific type:
/// - Integer (if all digits with optional leading `-`)
/// - Float (if contains `.` and parses successfully)
/// - Boolean (`true`/`false`, case-insensitive)
/// - String (fallback)
#[derive(Debug, Clone)]
pub struct EnvSource {
    prefix: String,
    separator: String,
}

impl EnvSource {
    /// Creates a new environment variable source.
    ///
    /// # Arguments
    ///
    /// * `prefix` - The prefix that identifies relevant env vars (e.g., "MYAPP")
    /// * `separator` - The separator between path segments (e.g., "__")
    pub fn new(prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            separator: separator.into(),
        }
    }
}

impl ConfigSource for EnvSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        let prefix_with_sep = format!("{}{}", self.prefix, self.separator);
        let mut entries = Vec::new();

        for (key, value) in std::env::vars() {
            if let Some(path_str) = key.strip_prefix(&prefix_with_sep) {
                if path_str.is_empty() {
                    continue;
                }

                let path: Vec<String> = path_str
                    .split(&self.separator)
                    .map(|s| s.to_lowercase())
                    .collect();

                let coerced_value = coerce_value(&value);
                entries.push(ConfigEntry::at_path(path, coerced_value));
            }
        }

        Ok(entries)
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
    fn test_env_source_basic() {
        let mut guard = EnvGuard::new();
        guard.set("TESTAPP2__HOST", "localhost");
        guard.set("TESTAPP2__PORT", "8080");

        let source = EnvSource::new("TESTAPP2", "__");
        let entries = source.entries().unwrap();

        // Find the entries we care about
        let host_entry = entries.iter().find(|e| e.path == vec!["host"]);
        let port_entry = entries.iter().find(|e| e.path == vec!["port"]);

        assert_eq!(
            host_entry.map(|e| &e.value),
            Some(&Value::String("localhost".to_string()))
        );
        assert_eq!(port_entry.map(|e| &e.value), Some(&Value::Integer(8080)));
    }

    #[test]
    fn test_env_source_nested() {
        let mut guard = EnvGuard::new();
        guard.set("MYAPP2__DATABASE__HOST", "db.example.com");
        guard.set("MYAPP2__DATABASE__PORT", "5432");
        guard.set("MYAPP2__SERVER__ENABLED", "true");

        let source = EnvSource::new("MYAPP2", "__");
        let entries = source.entries().unwrap();

        let db_host = entries
            .iter()
            .find(|e| e.path == vec!["database", "host"]);
        let db_port = entries
            .iter()
            .find(|e| e.path == vec!["database", "port"]);
        let srv_enabled = entries
            .iter()
            .find(|e| e.path == vec!["server", "enabled"]);

        assert_eq!(
            db_host.map(|e| &e.value),
            Some(&Value::String("db.example.com".to_string()))
        );
        assert_eq!(db_port.map(|e| &e.value), Some(&Value::Integer(5432)));
        assert_eq!(srv_enabled.map(|e| &e.value), Some(&Value::Boolean(true)));
    }

    #[test]
    fn test_env_source_case_conversion() {
        let mut guard = EnvGuard::new();
        guard.set("APP2__UPPER_CASE__NESTED_KEY", "value");

        let source = EnvSource::new("APP2", "__");
        let entries = source.entries().unwrap();

        let entry = entries
            .iter()
            .find(|e| e.path == vec!["upper_case", "nested_key"]);

        assert_eq!(
            entry.map(|e| &e.value),
            Some(&Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_env_source_ignores_unrelated() {
        let mut guard = EnvGuard::new();
        guard.set("APP3__KEY", "value");
        guard.set("OTHER3__KEY", "ignored");
        guard.set("APP3EXTRA__KEY", "also_ignored");

        let source = EnvSource::new("APP3", "__");
        let entries = source.entries().unwrap();

        // Should only have the APP3__KEY entry
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, vec!["key"]);
    }

    #[test]
    fn test_env_source_empty_path_ignored() {
        let mut guard = EnvGuard::new();
        // Just the prefix with separator but no path
        guard.set("APP4__", "value");

        let source = EnvSource::new("APP4", "__");
        let entries = source.entries().unwrap();

        // Should be empty - no valid path
        assert!(entries.is_empty());
    }

    #[test]
    fn test_env_source_custom_separator() {
        let mut guard = EnvGuard::new();
        guard.set("APP5_DB_HOST", "localhost");

        let source = EnvSource::new("APP5", "_");
        let entries = source.entries().unwrap();

        let entry = entries.iter().find(|e| e.path == vec!["db", "host"]);
        assert_eq!(
            entry.map(|e| &e.value),
            Some(&Value::String("localhost".to_string()))
        );
    }
}
