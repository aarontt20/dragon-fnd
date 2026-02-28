use super::source::{ConfigEntry, ConfigSource, ConfigValue};
use super::ConfigError;

#[derive(Debug, Clone)]
pub struct EnvSource {
    prefix: String,
    separator: String,
}

impl EnvSource {
    pub fn new(prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            separator: separator.into(),
        }
    }
}

impl ConfigSource for EnvSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        if self.prefix.is_empty() {
            return Err(ConfigError::InvalidPrefix);
        }
        if self.separator.is_empty() {
            return Err(ConfigError::InvalidSeparator);
        }

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

                if path.iter().any(|s| s.is_empty()) {
                    return Err(ConfigError::EmptyPathSegment { var: key.clone() });
                }

                let coerced_value = coerce_value(&value);
                entries.push(ConfigEntry::at_path(path, coerced_value));
            }
        }

        // Sort by path for deterministic merge order — std::env::vars()
        // iteration order is unspecified and can vary between runs.
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(entries)
    }
}

fn coerce_value(s: &str) -> ConfigValue {
    // Try boolean first (case-insensitive)
    if s.eq_ignore_ascii_case("true") {
        return ConfigValue::boolean(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return ConfigValue::boolean(false);
    }

    // Try integer (only if it looks like an integer: optional minus, then digits)
    if looks_like_integer(s) {
        if let Ok(i) = s.parse::<i64>() {
            return ConfigValue::integer(i);
        }
    }

    // Try float (if contains decimal point)
    if s.contains('.') {
        if let Ok(f) = s.parse::<f64>() {
            return ConfigValue::float(f);
        }
    }

    // Fallback to string
    ConfigValue::string(s)
}

fn looks_like_integer(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    // Reject leading zeros (match TOML rules: 007 is not a valid integer)
    if s.len() > 1 && s.starts_with('0') {
        return false;
    }
    s.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    struct EnvGuard {
        vars: Vec<String>,
    }

    impl EnvGuard {
        fn new(vars: &[(&str, &str)]) -> Self {
            let keys: Vec<String> = vars.iter().map(|(k, _)| k.to_string()).collect();
            for (key, value) in vars {
                // SAFETY: Tests run serially via #[serial], preventing data races.
                unsafe { std::env::set_var(key, value) };
            }
            Self { vars: keys }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for key in &self.vars {
                // SAFETY: Tests run serially via #[serial], preventing data races.
                unsafe { std::env::remove_var(key) };
            }
        }
    }

    // --- coerce_value tests ---

    #[test]
    fn coerce_integer() {
        assert_eq!(coerce_value("42"), ConfigValue::Integer(42));
        assert_eq!(coerce_value("-7"), ConfigValue::Integer(-7));
        assert_eq!(coerce_value("0"), ConfigValue::Integer(0));
    }

    #[test]
    fn coerce_float() {
        assert_eq!(coerce_value("3.15"), ConfigValue::Float(3.15));
        assert_eq!(coerce_value("-1.5"), ConfigValue::Float(-1.5));
        assert_eq!(coerce_value("0.0"), ConfigValue::Float(0.0));
    }

    #[test]
    fn coerce_boolean() {
        assert_eq!(coerce_value("true"), ConfigValue::Boolean(true));
        assert_eq!(coerce_value("false"), ConfigValue::Boolean(false));
        assert_eq!(coerce_value("TRUE"), ConfigValue::Boolean(true));
        assert_eq!(coerce_value("False"), ConfigValue::Boolean(false));
    }

    #[test]
    fn coerce_string() {
        assert_eq!(coerce_value("hello"), ConfigValue::String("hello".to_string()));
        assert_eq!(coerce_value("007"), ConfigValue::String("007".to_string()));
    }

    #[test]
    fn coerce_leading_zero_stays_string() {
        assert_eq!(coerce_value("007"), ConfigValue::String("007".to_string()));
        assert_eq!(coerce_value("01"), ConfigValue::String("01".to_string()));
        assert_eq!(coerce_value("-01"), ConfigValue::String("-01".to_string()));
        // Single zero is fine
        assert_eq!(coerce_value("0"), ConfigValue::Integer(0));
    }

    #[test]
    fn coerce_edge_cases() {
        assert_eq!(coerce_value(""), ConfigValue::String("".to_string()));
        assert_eq!(coerce_value("-"), ConfigValue::String("-".to_string()));
        assert_eq!(coerce_value("1.2.3"), ConfigValue::String("1.2.3".to_string()));
    }

    // --- EnvSource tests ---

    #[test]
    #[serial]
    fn env_source_basic() {
        let _guard = EnvGuard::new(&[
            ("TEST1__NAME", "hello"),
            ("TEST1__PORT", "8080"),
        ]);

        let source = EnvSource::new("TEST1", "__");
        let entries = source.entries().unwrap();
        assert_eq!(entries.len(), 2);

        let name_entry = entries.iter().find(|e| e.path == vec!["name"]).unwrap();
        assert_eq!(name_entry.value, ConfigValue::String("hello".to_string()));

        let port_entry = entries.iter().find(|e| e.path == vec!["port"]).unwrap();
        assert_eq!(port_entry.value, ConfigValue::Integer(8080));
    }

    #[test]
    #[serial]
    fn env_source_nested() {
        let _guard = EnvGuard::new(&[
            ("TEST2__A__B", "nested"),
            ("TEST2__X__Y", "also_nested"),
        ]);

        let source = EnvSource::new("TEST2", "__");
        let entries = source.entries().unwrap();
        assert_eq!(entries.len(), 2);

        let ab = entries.iter().find(|e| e.path == vec!["a", "b"]).unwrap();
        assert_eq!(ab.value, ConfigValue::String("nested".to_string()));

        let xy = entries.iter().find(|e| e.path == vec!["x", "y"]).unwrap();
        assert_eq!(xy.value, ConfigValue::String("also_nested".to_string()));
    }

    #[test]
    #[serial]
    fn env_source_case_conversion() {
        let _guard = EnvGuard::new(&[("TEST3__UPPER_CASE", "value")]);

        let source = EnvSource::new("TEST3", "__");
        let entries = source.entries().unwrap();

        let entry = entries.iter().find(|e| e.path == vec!["upper_case"]).unwrap();
        assert_eq!(entry.value, ConfigValue::String("value".to_string()));
    }

    #[test]
    #[serial]
    fn env_source_ignores_unrelated() {
        let _guard = EnvGuard::new(&[
            ("APP3__KEY", "found"),
            ("APP3EXTRA__KEY", "ignored"),
            ("OTHER__KEY", "ignored"),
        ]);

        let source = EnvSource::new("APP3", "__");
        let entries = source.entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, vec!["key"]);
    }

    #[test]
    #[serial]
    fn env_source_empty_path_ignored() {
        let _guard = EnvGuard::new(&[
            ("TEST4__", "no_path"),
            ("TEST4__VALID", "has_path"),
        ]);

        let source = EnvSource::new("TEST4", "__");
        let entries = source.entries().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, vec!["valid"]);
    }

    #[test]
    #[serial]
    fn env_source_custom_separator() {
        let _guard = EnvGuard::new(&[("TEST5_A_B", "value")]);

        let source = EnvSource::new("TEST5", "_");
        let entries = source.entries().unwrap();

        let entry = entries.iter().find(|e| e.path == vec!["a", "b"]).unwrap();
        assert_eq!(entry.value, ConfigValue::String("value".to_string()));
    }

    #[test]
    fn env_source_empty_separator_returns_error() {
        let source = EnvSource::new("APP", "");
        let err = source.entries().unwrap_err();

        assert!(matches!(err, ConfigError::InvalidSeparator));
    }

    #[test]
    fn env_source_empty_prefix_returns_error() {
        let source = EnvSource::new("", "__");
        let err = source.entries().unwrap_err();

        assert!(matches!(err, ConfigError::InvalidPrefix));
    }

    #[test]
    #[serial]
    fn env_source_empty_segment_error() {
        let _guard = EnvGuard::new(&[("EMPTYSEG__A____B", "value")]);
        let source = EnvSource::new("EMPTYSEG", "__");
        let err = source.entries().unwrap_err();
        assert!(matches!(err, ConfigError::EmptyPathSegment { .. }));
    }
}
