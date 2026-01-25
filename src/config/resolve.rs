//! Variable reference resolution for configuration values.
//!
//! Supports `${section.field}` syntax for cross-referencing values within config.
//! Use `$${...}` to escape and produce a literal `${...}`.

use super::ConfigError;
use toml::{Table, Value};

/// Resolves all `${path.to.field}` references in the configuration table.
///
/// Iteratively resolves references until no more substitutions are made.
/// Returns an error if a circular reference is detected or a referenced path doesn't exist.
pub fn resolve_references(table: &mut Table) -> Result<(), ConfigError> {
    const MAX_ITERATIONS: usize = 100;

    for _ in 0..MAX_ITERATIONS {
        let snapshot = table.clone();
        let substitutions = resolve_pass(table, &snapshot)?;
        if substitutions == 0 {
            return Ok(());
        }
    }

    Err(ConfigError::CircularReference)
}

/// Performs a single resolution pass over all string values.
/// Returns the number of substitutions made.
fn resolve_pass(table: &mut Table, root: &Table) -> Result<usize, ConfigError> {
    let mut count = 0;

    for (_key, value) in table.iter_mut() {
        count += resolve_value(value, root)?;
    }

    Ok(count)
}

/// Resolves references in a single value (recursively for tables/arrays).
fn resolve_value(value: &mut Value, root: &Table) -> Result<usize, ConfigError> {
    match value {
        Value::String(s) => resolve_string(s, root),
        Value::Table(t) => resolve_pass(t, root),
        Value::Array(arr) => {
            let mut count = 0;
            for item in arr.iter_mut() {
                count += resolve_value(item, root)?;
            }
            Ok(count)
        }
        _ => Ok(0),
    }
}

/// Resolves all `${...}` references in a string.
/// Handles `$$` escape sequences.
fn resolve_string(s: &mut String, root: &Table) -> Result<usize, ConfigError> {
    let mut result = String::with_capacity(s.len());
    let mut substitutions = 0;
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            match chars.peek() {
                Some('$') => {
                    // Escape sequence: $$ -> $
                    chars.next();
                    result.push('$');
                }
                Some('{') => {
                    // Reference: ${path.to.field}
                    chars.next(); // consume '{'
                    let path = consume_until(&mut chars, '}')
                        .ok_or(ConfigError::UnclosedReference)?;

                    let resolved = lookup_path(root, &path)?;
                    result.push_str(&resolved);
                    substitutions += 1;
                }
                _ => {
                    // Just a lone $
                    result.push('$');
                }
            }
        } else {
            result.push(ch);
        }
    }

    *s = result;
    Ok(substitutions)
}

/// Consumes characters until the delimiter, returning the collected string.
fn consume_until(chars: &mut std::iter::Peekable<std::str::Chars>, delim: char) -> Option<String> {
    let mut result = String::new();
    for ch in chars.by_ref() {
        if ch == delim {
            return Some(result);
        }
        result.push(ch);
    }
    None // Delimiter not found
}

/// Looks up a dotted path in the TOML table and returns the value as a string.
fn lookup_path(root: &Table, path: &str) -> Result<String, ConfigError> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(ConfigError::InvalidReferencePath(path.to_string()));
    }

    let not_found = || ConfigError::ReferenceNotFound(path.to_string());

    // First lookup from root table
    let mut current = root.get(parts[0]).ok_or_else(not_found)?;

    // Traverse remaining path segments
    for part in &parts[1..] {
        current = current
            .as_table()
            .and_then(|t| t.get(*part))
            .ok_or_else(not_found)?;
    }

    value_to_string(current, path)
}

/// Converts a TOML value to its string representation.
fn value_to_string(value: &Value, path: &str) -> Result<String, ConfigError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Float(f) => Ok(f.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Datetime(dt) => Ok(dt.to_string()),
        Value::Array(_) | Value::Table(_) => {
            Err(ConfigError::NonScalarReference(path.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(toml_str: &str) -> Table {
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn test_simple_reference() {
        let mut table = make_table(
            r#"
            host = "localhost"
            url = "http://${host}/api"
            "#,
        );
        resolve_references(&mut table).unwrap();
        assert_eq!(table["url"].as_str().unwrap(), "http://localhost/api");
    }

    #[test]
    fn test_nested_path() {
        let mut table = make_table(
            r#"
            [server]
            host = "example.com"
            port = 8080

            [client]
            endpoint = "https://${server.host}:${server.port}"
            "#,
        );
        resolve_references(&mut table).unwrap();
        assert_eq!(
            table["client"]["endpoint"].as_str().unwrap(),
            "https://example.com:8080"
        );
    }

    #[test]
    fn test_chained_references() {
        let mut table = make_table(
            r#"
            a = "hello"
            b = "${a} world"
            c = "${b}!"
            "#,
        );
        resolve_references(&mut table).unwrap();
        assert_eq!(table["c"].as_str().unwrap(), "hello world!");
    }

    #[test]
    fn test_escape_sequence() {
        let mut table = make_table(
            r#"
            value = "use $${VAR} for env vars"
            "#,
        );
        resolve_references(&mut table).unwrap();
        assert_eq!(
            table["value"].as_str().unwrap(),
            "use ${VAR} for env vars"
        );
    }

    #[test]
    fn test_integer_coercion() {
        let mut table = make_table(
            r#"
            port = 3000
            url = "http://localhost:${port}"
            "#,
        );
        resolve_references(&mut table).unwrap();
        assert_eq!(table["url"].as_str().unwrap(), "http://localhost:3000");
    }

    #[test]
    fn test_circular_reference() {
        let mut table = make_table(
            r#"
            a = "${b}"
            b = "${a}"
            "#,
        );
        let result = resolve_references(&mut table);
        assert!(matches!(result, Err(ConfigError::CircularReference)));
    }

    #[test]
    fn test_missing_reference() {
        let mut table = make_table(
            r#"
            url = "${nonexistent.path}"
            "#,
        );
        let result = resolve_references(&mut table);
        assert!(matches!(result, Err(ConfigError::ReferenceNotFound(_))));
    }

    #[test]
    fn test_array_values() {
        let mut table = make_table(
            r#"
            base = "/api"
            endpoints = ["${base}/users", "${base}/posts"]
            "#,
        );
        resolve_references(&mut table).unwrap();
        let endpoints = table["endpoints"].as_array().unwrap();
        assert_eq!(endpoints[0].as_str().unwrap(), "/api/users");
        assert_eq!(endpoints[1].as_str().unwrap(), "/api/posts");
    }
}
