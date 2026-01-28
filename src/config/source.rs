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
    if path.is_empty() {
        if let Value::Table(overlay) = value {
            deep_merge(table, overlay);
        }
        return;
    }

    let (first, rest) = path.split_first().expect("path is non-empty");

    if rest.is_empty() {
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
