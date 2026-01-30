use super::ConfigError;
use toml::{Table, Value};

const MAX_ITERATIONS: usize = 100;

pub fn resolve_references(table: &mut Table) -> Result<(), ConfigError> {
    for _ in 0..MAX_ITERATIONS {
        let snapshot = table.clone();
        let substitutions = resolve_pass(table, &snapshot)?;
        if substitutions == 0 {
            return Ok(());
        }
    }

    Err(ConfigError::CircularReference)
}

fn resolve_pass(table: &mut Table, root: &Table) -> Result<usize, ConfigError> {
    let mut count = 0;

    for (_key, value) in table.iter_mut() {
        count += resolve_value(value, root)?;
    }

    Ok(count)
}

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
