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
    let mut in_progress = HashSet::new();

    for source in deps.keys() {
        visit(source, &deps, &mut visited, &mut in_progress, &mut result)?;
    }

    Ok(result)
}

fn visit<'a>(
    node: &'a ConfigPath,
    deps: &HashMap<&'a ConfigPath, HashSet<&'a ConfigPath>>,
    visited: &mut HashSet<&'a ConfigPath>,
    in_progress: &mut HashSet<&'a ConfigPath>,
    result: &mut Vec<ConfigPath>,
) -> Result<(), ConfigError> {
    if in_progress.contains(node) {
        return Err(ConfigError::CircularReference);
    }
    if visited.contains(node) {
        return Ok(());
    }

    in_progress.insert(node);

    if let Some(node_deps) = deps.get(node) {
        for dep in node_deps {
            if deps.contains_key(dep) {
                visit(dep, deps, visited, in_progress, result)?;
            }
        }
    }

    in_progress.remove(node);
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

    let mut current: &Value = table.get(&path[0]).ok_or_else(err)?;

    for segment in &path[1..] {
        current = match current {
            Value::Table(t) => t.get(segment).ok_or_else(err)?,
            Value::Array(arr) => arr.get(segment.parse::<usize>().map_err(|_| err())?).ok_or_else(err)?,
            _ => return Err(err()),
        };
    }

    Ok(current)
}

fn get_value_mut<'a>(table: &'a mut Table, path: &[String]) -> Result<&'a mut Value, ConfigError> {
    let err = || ConfigError::ReferenceNotFound(path.join("."));

    let mut current: &mut Value = table.get_mut(&path[0]).ok_or_else(err)?;

    for segment in &path[1..] {
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
    let s = s.trim();
    if s.starts_with("${") && s.ends_with('}') && s.matches("${").count() == 1 {
        Some(&s[2..s.len() - 1])
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
