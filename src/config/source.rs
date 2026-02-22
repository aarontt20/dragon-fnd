use toml::{Table, Value};

use super::ConfigError;

#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub path: Vec<String>,
    pub value: Value,
}

impl ConfigEntry {
    pub fn root(table: Table) -> Self {
        Self {
            path: Vec::new(),
            value: Value::Table(table),
        }
    }

    pub fn at_path(path: Vec<String>, value: Value) -> Self {
        Self { path, value }
    }
}

pub trait ConfigSource: Send + Sync + std::fmt::Debug {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>;
}

pub fn merge_at_path(table: &mut Table, path: &[String], value: Value) {
    let Some((first, rest)) = path.split_first() else {
        // Root-level merge: deep merge if value is a table
        if let Value::Table(overlay) = value {
            deep_merge(table, overlay);
        }
        return;
    };

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
    fn merge_at_empty_path_deep_merges() {
        let mut table = Table::new();
        table.insert("existing".into(), Value::String("kept".into()));

        let mut nested = Table::new();
        nested.insert("inner".into(), Value::String("original".into()));
        table.insert("nested".into(), Value::Table(nested));

        let mut overlay = Table::new();
        overlay.insert("new_key".into(), Value::String("added".into()));

        let mut overlay_nested = Table::new();
        overlay_nested.insert("extra".into(), Value::String("merged".into()));
        overlay.insert("nested".into(), Value::Table(overlay_nested));

        merge_at_path(&mut table, &[], Value::Table(overlay));

        assert_eq!(table["existing"].as_str(), Some("kept"));
        assert_eq!(table["new_key"].as_str(), Some("added"));
        assert_eq!(table["nested"]["inner"].as_str(), Some("original"));
        assert_eq!(table["nested"]["extra"].as_str(), Some("merged"));
    }

    #[test]
    fn merge_at_path_creates_intermediates() {
        let mut table = Table::new();
        let path: Vec<String> = vec!["a".into(), "b".into(), "c".into()];

        merge_at_path(&mut table, &path, Value::String("deep".into()));

        assert_eq!(table["a"]["b"]["c"].as_str(), Some("deep"));
    }

    #[test]
    fn merge_at_path_replaces_leaf() {
        let mut table = Table::new();
        table.insert("key".into(), Value::String("old".into()));

        let path: Vec<String> = vec!["key".into()];
        merge_at_path(&mut table, &path, Value::String("new".into()));

        assert_eq!(table["key"].as_str(), Some("new"));
    }

    #[test]
    fn merge_at_path_merges_tables_at_leaf() {
        let mut table = Table::new();
        let mut existing = Table::new();
        existing.insert("a".into(), Value::String("1".into()));
        table.insert("key".into(), Value::Table(existing));

        let mut overlay = Table::new();
        overlay.insert("b".into(), Value::String("2".into()));

        let path: Vec<String> = vec!["key".into()];
        merge_at_path(&mut table, &path, Value::Table(overlay));

        assert_eq!(table["key"]["a"].as_str(), Some("1"));
        assert_eq!(table["key"]["b"].as_str(), Some("2"));
    }

    #[test]
    fn config_entry_constructors() {
        let mut t = Table::new();
        t.insert("k".into(), Value::String("v".into()));

        let root = ConfigEntry::root(t);
        assert!(root.path.is_empty());
        assert!(root.value.is_table());

        let at = ConfigEntry::at_path(
            vec!["my".into(), "key".into()],
            Value::String("val".into()),
        );
        assert_eq!(at.path, vec!["my", "key"]);
        assert_eq!(at.value.as_str(), Some("val"));
    }
}
