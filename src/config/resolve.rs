use std::collections::{HashMap, HashSet};

use super::ConfigError;
use toml::{Table, Value};

/// A path in the config tree (e.g., ["database", "url"])
type ConfigPath = Vec<String>;

pub fn resolve_references(table: &mut Table) -> Result<(), ConfigError> {
    // Phase 1: Collect all references as (source_path, target_path) pairs
    let references = collect_references(table)?;

    if references.is_empty() {
        return Ok(());
    }

    // Phase 2: Build dependency graph and topologically sort
    let resolution_order = topological_sort(&references)?;

    // Phase 3: Resolve in dependency order
    for path in resolution_order {
        resolve_at_path(table, &path)?;
    }

    Ok(())
}

/// Collect all references from the config tree
fn collect_references(table: &Table) -> Result<Vec<(ConfigPath, ConfigPath)>, ConfigError> {
    let mut refs = Vec::new();
    let mut path = Vec::new();
    collect_from_table(table, &mut path, &mut refs)?;
    Ok(refs)
}

fn collect_from_table(
    table: &Table,
    path: &mut ConfigPath,
    refs: &mut Vec<(ConfigPath, ConfigPath)>,
) -> Result<(), ConfigError> {
    for (key, val) in table {
        path.push(key.clone());
        collect_from_value(val, path, refs)?;
        path.pop();
    }
    Ok(())
}

fn collect_from_value(
    value: &Value,
    path: &mut ConfigPath,
    refs: &mut Vec<(ConfigPath, ConfigPath)>,
) -> Result<(), ConfigError> {
    match value {
        Value::String(s) => {
            for target in parse_references(s)? {
                refs.push((path.clone(), target));
            }
        }
        Value::Table(t) => collect_from_table(t, path, refs)?,
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                path.push(i.to_string());
                collect_from_value(val, path, refs)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

/// Parse all ${...} references from a string
fn parse_references(s: &str) -> Result<Vec<ConfigPath>, ConfigError> {
    let mut refs = Vec::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            match chars.peek() {
                Some('$') => { chars.next(); }
                Some('{') => {
                    chars.next();
                    let path = consume_until(&mut chars, '}')
                        .ok_or(ConfigError::UnclosedReference)?;
                    refs.push(path.split('.').map(String::from).collect());
                }
                _ => {}
            }
        }
    }

    Ok(refs)
}

fn consume_until(chars: &mut impl Iterator<Item = char>, delim: char) -> Option<String> {
    let mut result = String::new();
    for ch in chars {
        if ch == delim {
            return Some(result);
        }
        result.push(ch);
    }
    None
}

/// Topological sort - returns paths in resolution order
fn topological_sort(references: &[(ConfigPath, ConfigPath)]) -> Result<Vec<ConfigPath>, ConfigError> {
    let mut deps: HashMap<&ConfigPath, HashSet<&ConfigPath>> = HashMap::new();

    for (source, target) in references {
        deps.entry(source).or_default().insert(target);
    }

    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    for source in deps.keys() {
        visit(source, &deps, &mut visited, &mut stack, &mut result)?;
    }

    Ok(result)
}

fn visit<'a>(
    node: &'a ConfigPath,
    deps: &HashMap<&'a ConfigPath, HashSet<&'a ConfigPath>>,
    visited: &mut HashSet<&'a ConfigPath>,
    stack: &mut Vec<&'a ConfigPath>,
    result: &mut Vec<ConfigPath>,
) -> Result<(), ConfigError> {
    if let Some(pos) = stack.iter().position(|n| *n == node) {
        let cycle: Vec<String> = stack[pos..]
            .iter()
            .chain(std::iter::once(&node))
            .map(|p| p.join("."))
            .collect();
        return Err(ConfigError::CircularReference(cycle));
    }
    if visited.contains(node) {
        return Ok(());
    }

    stack.push(node);

    if let Some(node_deps) = deps.get(node) {
        for dep in node_deps {
            if deps.contains_key(dep) {
                visit(dep, deps, visited, stack, result)?;
            }
        }
    }

    stack.pop();
    visited.insert(node);
    result.push(node.clone());

    Ok(())
}

/// Resolve all references at a specific path
fn resolve_at_path(table: &mut Table, path: &[String]) -> Result<(), ConfigError> {
    // Get the string value (clone to release borrow)
    let s = match get_value(table, path)? {
        Value::String(s) => s.clone(),
        _ => return Ok(()),
    };

    // Resolve and apply
    let resolved = resolve_string(&s, table)?;
    *get_value_mut(table, path)? = resolved;

    Ok(())
}

fn get_value<'a>(table: &'a Table, path: &[String]) -> Result<&'a Value, ConfigError> {
    let err = || ConfigError::ReferenceNotFound(path.join("."));

    let (first, rest) = path.split_first().ok_or_else(err)?;
    let mut current: &Value = table.get(first).ok_or_else(err)?;

    for segment in rest {
        current = match current {
            Value::Table(t) => t.get(segment).ok_or_else(err)?,
            Value::Array(arr) => arr.get(segment.parse::<usize>().map_err(|_| err())?).ok_or_else(err)?,
            _ => return Err(err()),
        };
    }

    Ok(current)
}

// Structurally identical to get_value; duplicated because Rust's borrow
// checker requires separate shared and mutable traversal paths.
// If the navigation logic changes, update both functions.
fn get_value_mut<'a>(table: &'a mut Table, path: &[String]) -> Result<&'a mut Value, ConfigError> {
    let err = || ConfigError::ReferenceNotFound(path.join("."));

    let (first, rest) = path.split_first().ok_or_else(err)?;
    let mut current: &mut Value = table.get_mut(first).ok_or_else(err)?;

    for segment in rest {
        current = match current {
            Value::Table(t) => t.get_mut(segment).ok_or_else(err)?,
            Value::Array(arr) => arr.get_mut(segment.parse::<usize>().map_err(|_| err())?).ok_or_else(err)?,
            _ => return Err(err()),
        };
    }

    Ok(current)
}

/// Check if string is exactly `${path}` (full value substitution)
fn is_pure_reference(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("${")?.strip_suffix('}')?;
    if !inner.contains('}') && !inner.contains("${") {
        Some(inner)
    } else {
        None
    }
}

fn resolve_string(s: &str, table: &Table) -> Result<Value, ConfigError> {
    // Pure reference: return the value directly (any type)
    if let Some(path) = is_pure_reference(s) {
        return lookup_value(table, path).cloned();
    }

    // String interpolation
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            match chars.peek() {
                Some('$') => {
                    chars.next();
                    result.push('$');
                }
                Some('{') => {
                    chars.next();
                    let path = consume_until(&mut chars, '}').ok_or(ConfigError::UnclosedReference)?;
                    result.push_str(&value_to_string(lookup_value(table, &path)?, &path)?);
                }
                _ => result.push('$'),
            }
        } else {
            result.push(ch);
        }
    }

    Ok(Value::String(result))
}

fn lookup_value<'a>(table: &'a Table, path: &str) -> Result<&'a Value, ConfigError> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(ConfigError::InvalidReferencePath(path.to_string()));
    }

    let err = || ConfigError::ReferenceNotFound(path.to_string());

    let mut current = table.get(parts[0]).ok_or_else(err)?;
    for part in &parts[1..] {
        current = current.as_table().and_then(|t| t.get(*part)).ok_or_else(err)?;
    }

    Ok(current)
}

fn value_to_string(value: &Value, path: &str) -> Result<String, ConfigError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Float(f) => Ok(f.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Datetime(dt) => Ok(dt.to_string()),
        Value::Array(_) | Value::Table(_) => Err(ConfigError::NonScalarReference(path.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_from_toml(s: &str) -> Table {
        toml::from_str(s).unwrap()
    }

    // --- Basic String Interpolation (4 tests) ---

    #[test]
    fn simple_reference() {
        let mut table = table_from_toml(r#"
            name = "world"
            greeting = "hello ${name}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["greeting"].as_str(), Some("hello world"));
    }

    #[test]
    fn multiple_references_in_one_string() {
        let mut table = table_from_toml(r#"
            host = "localhost"
            port = 8080
            url = "${host}:${port}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["url"].as_str(), Some("localhost:8080"));
    }

    #[test]
    fn nested_path_reference() {
        let mut table = table_from_toml(r#"
            [database]
            host = "db.example.com"

            [app]
            db_host = "${database.host}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["app"]["db_host"].as_str(), Some("db.example.com"));
    }

    #[test]
    fn no_references_unchanged() {
        let mut table = table_from_toml(r#"
            name = "hello"
            count = 42
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["name"].as_str(), Some("hello"));
        assert_eq!(table["count"].as_integer(), Some(42));
    }

    // --- Full Value Substitution (6 tests) ---

    #[test]
    fn pure_reference_preserves_integer() {
        let mut table = table_from_toml(r#"
            default_port = 3000
            port = "${default_port}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["port"].as_integer(), Some(3000));
    }

    #[test]
    fn pure_reference_preserves_boolean() {
        let mut table = table_from_toml(r#"
            debug = true
            verbose = "${debug}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["verbose"].as_bool(), Some(true));
    }

    #[test]
    fn pure_reference_preserves_float() {
        let mut table = table_from_toml(r#"
            rate = 0.75
            ratio = "${rate}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["ratio"].as_float(), Some(0.75));
    }

    #[test]
    fn pure_reference_copies_array() {
        let mut table = table_from_toml(r#"
            tags = ["a", "b", "c"]
            labels = "${tags}"
        "#);
        resolve_references(&mut table).unwrap();
        let arr = table["labels"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0].as_str(), Some("a"));
    }

    #[test]
    fn pure_reference_copies_table() {
        let mut table = table_from_toml(r#"
            [defaults]
            host = "localhost"
            port = 8080

            [server]
            config = "${defaults}"
        "#);
        resolve_references(&mut table).unwrap();
        let config = table["server"]["config"].as_table().unwrap();
        assert_eq!(config["host"].as_str(), Some("localhost"));
        assert_eq!(config["port"].as_integer(), Some(8080));
    }

    #[test]
    fn trailing_brace_is_interpolation_not_pure_reference() {
        let mut table = table_from_toml(r#"
            name = "world"
            msg = "${name}}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["msg"].as_str(), Some("world}"));
    }

    #[test]
    fn whitespace_around_reference_is_interpolation() {
        let mut table = table_from_toml(r#"
            value = "hello"
            padded = "  ${value}  "
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["padded"].as_str(), Some("  hello  "));
    }

    // --- Chained References (3 tests) ---

    #[test]
    fn chained_references() {
        let mut table = table_from_toml(r#"
            c = "final"
            b = "${c}"
            a = "${b}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["a"].as_str(), Some("final"));
    }

    #[test]
    fn chained_pure_references() {
        let mut table = table_from_toml(r#"
            c = 42
            b = "${c}"
            a = "${b}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["a"].as_integer(), Some(42));
    }

    #[test]
    fn deep_chain() {
        let mut table = table_from_toml(r#"
            v5 = "end"
            v4 = "${v5}"
            v3 = "${v4}"
            v2 = "${v3}"
            v1 = "${v2}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["v1"].as_str(), Some("end"));
    }

    // --- Arrays (2 tests) ---

    #[test]
    fn references_in_array_elements() {
        let mut table = table_from_toml(r#"
            host = "localhost"
            port = 8080
            urls = ["http://${host}", "port: ${port}"]
        "#);
        resolve_references(&mut table).unwrap();
        let arr = table["urls"].as_array().unwrap();
        assert_eq!(arr[0].as_str(), Some("http://localhost"));
        assert_eq!(arr[1].as_str(), Some("port: 8080"));
    }

    #[test]
    fn pure_reference_in_array() {
        let mut table = table_from_toml(r#"
            default_port = 3000
            ports = ["${default_port}"]
        "#);
        resolve_references(&mut table).unwrap();
        let arr = table["ports"].as_array().unwrap();
        assert_eq!(arr[0].as_integer(), Some(3000));
    }

    // --- Escape Sequences (3 tests) ---

    #[test]
    fn escaped_dollar_sign_with_reference() {
        let mut table = table_from_toml(r#"
            name = "world"
            msg = "$$hello ${name}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["msg"].as_str(), Some("$hello world"));
    }

    #[test]
    fn mixed_escaped_and_reference() {
        let mut table = table_from_toml(r#"
            amount = 50
            price = "$$${amount}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["price"].as_str(), Some("$50"));
    }

    #[test]
    fn string_without_references_unchanged() {
        let mut table = table_from_toml(r#"
            text = "$$not_a_ref"
        "#);
        resolve_references(&mut table).unwrap();
        // $$ without actual ${} references — string is not processed
        assert_eq!(table["text"].as_str(), Some("$$not_a_ref"));
    }

    // --- Type Coercion in Interpolation (2 tests) ---

    #[test]
    fn integer_to_string_in_interpolation() {
        let mut table = table_from_toml(r#"
            port = 8080
            msg = "port ${port}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["msg"].as_str(), Some("port 8080"));
    }

    #[test]
    fn boolean_to_string_in_interpolation() {
        let mut table = table_from_toml(r#"
            enabled = true
            msg = "debug: ${enabled}"
        "#);
        resolve_references(&mut table).unwrap();
        assert_eq!(table["msg"].as_str(), Some("debug: true"));
    }

    // --- Error Cases (12 tests) ---

    #[test]
    fn circular_reference_detected() {
        let mut table = table_from_toml(r#"
            a = "${b}"
            b = "${a}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        match err {
            ConfigError::CircularReference(cycle) => {
                assert!(cycle.contains(&"a".to_string()));
                assert!(cycle.contains(&"b".to_string()));
                assert_eq!(cycle.first(), cycle.last(), "cycle should close");
            }
            other => panic!("expected CircularReference, got {other:?}"),
        }
    }

    #[test]
    fn self_reference_detected() {
        let mut table = table_from_toml(r#"
            a = "${a}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        match err {
            ConfigError::CircularReference(cycle) => {
                assert_eq!(cycle, vec!["a", "a"]);
            }
            other => panic!("expected CircularReference, got {other:?}"),
        }
    }

    #[test]
    fn three_way_cycle_detected() {
        let mut table = table_from_toml(r#"
            a = "${b}"
            b = "${c}"
            c = "${a}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        match err {
            ConfigError::CircularReference(cycle) => {
                assert!(cycle.contains(&"a".to_string()));
                assert!(cycle.contains(&"b".to_string()));
                assert!(cycle.contains(&"c".to_string()));
                assert_eq!(cycle.first(), cycle.last(), "cycle should close");
            }
            other => panic!("expected CircularReference, got {other:?}"),
        }
    }

    #[test]
    fn reference_not_found() {
        let mut table = table_from_toml(r#"
            a = "${nonexistent}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::ReferenceNotFound(_)));
    }

    #[test]
    fn nested_reference_not_found() {
        let mut table = table_from_toml(r#"
            [database]
            host = "localhost"

            [app]
            url = "${database.port}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::ReferenceNotFound(_)));
    }

    #[test]
    fn unclosed_reference_with_valid_reference() {
        let mut table = table_from_toml(r#"
            name = "world"
            msg = "${name} ${unclosed"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::UnclosedReference));
    }

    #[test]
    fn unclosed_reference_alone() {
        let mut table = table_from_toml(r#"
            a = "${unclosed"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::UnclosedReference));
    }

    #[test]
    fn unclosed_reference_in_nested_table() {
        let mut table = table_from_toml(r#"
            [server]
            url = "${unclosed"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::UnclosedReference));
    }

    #[test]
    fn unclosed_reference_in_array() {
        let mut table = table_from_toml(r#"
            items = ["${unclosed"]
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::UnclosedReference));
    }

    #[test]
    fn non_scalar_in_interpolation() {
        let mut table = table_from_toml(r#"
            [nested]
            key = "value"

            msg = "text ${nested}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::NonScalarReference(_)));
    }

    #[test]
    fn array_in_interpolation() {
        let mut table = table_from_toml(r#"
            items = [1, 2, 3]
            msg = "items: ${items}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::NonScalarReference(_)));
    }

    #[test]
    fn get_value_empty_path_returns_error() {
        let table = table_from_toml(r#"
            name = "hello"
        "#);
        let empty: Vec<String> = vec![];
        let err = get_value(&table, &empty).unwrap_err();
        assert!(matches!(err, ConfigError::ReferenceNotFound(_)));
    }

    #[test]
    fn invalid_path_empty_segment() {
        let mut table = table_from_toml(r#"
            a = "${x..y}"
        "#);
        let err = resolve_references(&mut table).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidReferencePath(_)));
    }
}
