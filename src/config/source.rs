//! Core abstractions for configuration sources.
//!
//! This module provides the trait and types that unify all configuration sources
//! (files, environment variables, CLI args, etc.) under a single abstraction.

use toml::{Table, Value};

use super::ConfigError;

/// A single configuration entry to merge into the config table.
///
/// All configuration sources produce entries in this format, enabling
/// unified merge logic regardless of source type.
#[derive(Debug, Clone)]
pub struct ConfigEntry {
    /// Path segments to the target location.
    /// Empty path means root-level merge (for complete tables like files).
    /// Non-empty path like `["database", "host"]` targets nested locations.
    pub path: Vec<String>,

    /// The value to merge at the target path.
    pub value: Value,
}

impl ConfigEntry {
    /// Creates a root-level entry (for merging complete tables).
    pub fn root(table: Table) -> Self {
        Self {
            path: Vec::new(),
            value: Value::Table(table),
        }
    }

    /// Creates an entry at a specific path.
    pub fn at_path(path: Vec<String>, value: Value) -> Self {
        Self { path, value }
    }
}

/// A source of configuration entries.
///
/// Implement this trait to create custom configuration sources.
/// The builder collects entries from all sources and merges them
/// in registration order.
///
/// # Example
///
/// ```ignore
/// struct MySource { /* ... */ }
///
/// impl ConfigSource for MySource {
///     fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
///         Ok(vec![
///             ConfigEntry::at_path(
///                 vec!["my".into(), "key".into()],
///                 toml::Value::String("value".into()),
///             ),
///         ])
///     }
/// }
/// ```
pub trait ConfigSource: Send + Sync + std::fmt::Debug {
    /// Produces configuration entries to merge.
    ///
    /// Returns a vector of entries, each specifying a path and value.
    /// Entries are applied in order, so later entries override earlier ones.
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>;
}

/// Merges a value at the given path into the table.
///
/// This is the unified merge function that handles all merge scenarios:
/// - Empty path with Table value: deep merge at root level
/// - Non-empty path: navigate/create intermediate tables, then merge or replace
///
/// Deep merging applies to nested tables: keys are merged recursively rather
/// than replaced entirely. Non-table values (including arrays) replace entirely.
pub fn merge_at_path(table: &mut Table, path: &[String], value: Value) {
    if path.is_empty() {
        // Root-level merge: deep merge if value is a table
        if let Value::Table(overlay) = value {
            deep_merge(table, overlay);
        } else {
            // Non-table at root level - unusual but we could handle by ignoring
            // or we could return an error. For now, ignore non-table roots.
        }
        return;
    }

    // Non-empty path: navigate to target location
    let (first, rest) = path.split_first().expect("path is non-empty");

    if rest.is_empty() {
        // At final key: merge or replace depending on types
        match (table.get_mut(first), &value) {
            (Some(Value::Table(base)), Value::Table(overlay)) => {
                deep_merge(base, overlay.clone());
            }
            _ => {
                table.insert(first.clone(), value);
            }
        }
        return;
    }

    // More path segments remain: ensure intermediate table exists
    if !matches!(table.get(first), Some(Value::Table(_))) {
        table.insert(first.clone(), Value::Table(Table::new()));
    }

    if let Some(Value::Table(nested)) = table.get_mut(first) {
        merge_at_path(nested, rest, value);
    }
}

/// Deep merges an overlay table into a base table.
///
/// For each key in overlay:
/// - If both base and overlay have tables at that key, merge recursively
/// - Otherwise, overlay value replaces base value
fn deep_merge(base: &mut Table, overlay: Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(base_table)), Value::Table(overlay_table)) => {
                deep_merge(base_table, overlay_table);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_at_empty_path_deep_merges() {
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
    }

    #[test]
    fn test_merge_at_path_creates_intermediates() {
        let mut table = Table::new();

        merge_at_path(
            &mut table,
            &["a".into(), "b".into(), "c".into()],
            Value::Integer(123),
        );

        let a = table.get("a").unwrap().as_table().unwrap();
        let b = a.get("b").unwrap().as_table().unwrap();
        assert_eq!(b.get("c"), Some(&Value::Integer(123)));
    }

    #[test]
    fn test_merge_at_path_replaces_leaf() {
        let mut table = Table::new();
        table.insert("key".into(), Value::String("old".into()));

        merge_at_path(&mut table, &["key".into()], Value::String("new".into()));

        assert_eq!(table.get("key"), Some(&Value::String("new".into())));
    }

    #[test]
    fn test_merge_at_path_merges_tables_at_leaf() {
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
    }

    #[test]
    fn test_config_entry_constructors() {
        let root = ConfigEntry::root(Table::new());
        assert!(root.path.is_empty());

        let at_path = ConfigEntry::at_path(vec!["a".into(), "b".into()], Value::Integer(42));
        assert_eq!(at_path.path, vec!["a", "b"]);
    }
}
