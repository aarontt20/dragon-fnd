use toml::Value;

use super::source::{ConfigEntry, ConfigSource};
use super::ConfigError;

#[derive(Debug, Clone)]
pub struct EnvSource {
    prefix: String,
    separator: String,
}

impl EnvSource {
    pub fn new(prefix: impl Into<String>, separator: impl Into<String>) -> Self {
        let separator = separator.into();
        assert!(!separator.is_empty(), "separator must not be empty");
        Self {
            prefix: prefix.into(),
            separator,
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

fn looks_like_integer(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}
