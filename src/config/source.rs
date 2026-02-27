use std::collections::BTreeMap;

use toml::{Table, Value};

use super::ConfigError;

/// Library-owned value type for config entries.
///
/// Replaces `toml::Value` in the public API so that `ConfigSource` implementors
/// don't need to depend on the `toml` crate.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    /// Stored as a string to avoid exposing `toml::Datetime`.
    /// Parsed during internal conversion.
    Datetime(String),
    Array(Vec<ConfigValue>),
    Table(ConfigTable),
}

/// An ordered map of string keys to config values.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigTable(pub(crate) BTreeMap<String, ConfigValue>);

impl ConfigValue {
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }

    pub fn integer(i: i64) -> Self {
        Self::Integer(i)
    }

    pub fn float(f: f64) -> Self {
        Self::Float(f)
    }

    pub fn boolean(b: bool) -> Self {
        Self::Boolean(b)
    }

    pub fn datetime(s: impl Into<String>) -> Self {
        Self::Datetime(s.into())
    }
}

impl ConfigTable {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn insert(mut self, key: impl Into<String>, value: ConfigValue) -> Self {
        self.0.insert(key.into(), value);
        self
    }
}

impl Default for ConfigTable {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ConfigValue> for Value {
    fn from(cv: ConfigValue) -> Self {
        match cv {
            ConfigValue::String(s) => Value::String(s),
            ConfigValue::Integer(i) => Value::Integer(i),
            ConfigValue::Float(f) => Value::Float(f),
            ConfigValue::Boolean(b) => Value::Boolean(b),
            ConfigValue::Datetime(s) => {
                // Parse the string as a TOML datetime. If parsing fails (e.g., the
                // user passed a non-datetime string to ConfigValue::datetime()), fall
                // back to Value::String. This is acceptable because the conversion is
                // internal — the resulting toml::Value feeds into merge/resolve, not
                // back to the user. Invalid datetime strings surface as type errors
                // during final deserialization.
                s.parse::<toml::value::Datetime>()
                    .map(Value::Datetime)
                    .unwrap_or_else(|_| Value::String(s))
            }
            ConfigValue::Array(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            }
            ConfigValue::Table(table) => {
                Value::Table(
                    table.0.into_iter().map(|(k, v)| (k, Value::from(v))).collect(),
                )
            }
        }
    }
}

impl From<Value> for ConfigValue {
    fn from(tv: Value) -> Self {
        match tv {
            Value::String(s) => ConfigValue::String(s),
            Value::Integer(i) => ConfigValue::Integer(i),
            Value::Float(f) => ConfigValue::Float(f),
            Value::Boolean(b) => ConfigValue::Boolean(b),
            Value::Datetime(d) => ConfigValue::Datetime(d.to_string()),
            Value::Array(arr) => {
                ConfigValue::Array(arr.into_iter().map(ConfigValue::from).collect())
            }
            Value::Table(table) => {
                ConfigValue::Table(ConfigTable(
                    table.into_iter().map(|(k, v)| (k, ConfigValue::from(v))).collect(),
                ))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigEntry {
    pub path: Vec<String>,
    pub value: ConfigValue,
}

impl ConfigEntry {
    pub fn root(table: Table) -> Self {
        Self {
            path: Vec::new(),
            value: Value::Table(table).into(),
        }
    }

    pub fn at_path(path: Vec<String>, value: ConfigValue) -> Self {
        Self { path, value }
    }
}

pub trait ConfigSource: Send + Sync + std::fmt::Debug {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError>;
}

pub fn merge_at_path(table: &mut Table, path: &[String], value: Value) -> Result<(), ConfigError> {
    let Some((first, rest)) = path.split_first() else {
        // Root-level merge: deep merge if value is a table
        if let Value::Table(overlay) = value {
            deep_merge(table, overlay);
            return Ok(());
        }
        return Err(ConfigError::RootNotTable(value_kind(&value).to_string()));
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
        return Ok(());
    }

    // More path segments remain: ensure intermediate table exists
    match table.get(first) {
        Some(Value::Table(_)) => {} // already a table, fine
        Some(existing) => {
            return Err(ConfigError::TypeConflict {
                path: first.clone(),
                existing: value_kind(existing).to_string(),
                incoming: "table".to_string(),
            });
        }
        None => {
            table.insert(first.clone(), Value::Table(Table::new()));
        }
    }

    match table.get_mut(first) {
        Some(Value::Table(nested)) => {
            merge_at_path(nested, rest, value).map_err(|e| match e {
                ConfigError::TypeConflict {
                    path,
                    existing,
                    incoming,
                } => ConfigError::TypeConflict {
                    path: format!("{first}.{path}"),
                    existing,
                    incoming,
                },
                // Only RootNotTable is possible here; cannot occur with non-empty path
                other => other,
            })?;
        }
        // Preceding match ensures a Table exists at `first` (either pre-existing or freshly inserted)
        _ => unreachable!("expected table at {first:?} after intermediate-key check"),
    }

    Ok(())
}

pub(crate) fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "boolean",
        Value::Datetime(_) => "datetime",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
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

        merge_at_path(&mut table, &[], Value::Table(overlay)).unwrap();

        assert_eq!(table["existing"].as_str(), Some("kept"));
        assert_eq!(table["new_key"].as_str(), Some("added"));
        assert_eq!(table["nested"]["inner"].as_str(), Some("original"));
        assert_eq!(table["nested"]["extra"].as_str(), Some("merged"));
    }

    #[test]
    fn merge_at_empty_path_non_table_returns_error() {
        let mut table = Table::new();
        let err = merge_at_path(&mut table, &[], Value::String("not a table".into())).unwrap_err();
        assert!(matches!(err, ConfigError::RootNotTable(_)));
    }

    #[test]
    fn merge_at_path_creates_intermediates() {
        let mut table = Table::new();
        let path: Vec<String> = vec!["a".into(), "b".into(), "c".into()];

        merge_at_path(&mut table, &path, Value::String("deep".into())).unwrap();

        assert_eq!(table["a"]["b"]["c"].as_str(), Some("deep"));
    }

    #[test]
    fn merge_at_path_replaces_leaf() {
        let mut table = Table::new();
        table.insert("key".into(), Value::String("old".into()));

        let path: Vec<String> = vec!["key".into()];
        merge_at_path(&mut table, &path, Value::String("new".into())).unwrap();

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
        merge_at_path(&mut table, &path, Value::Table(overlay)).unwrap();

        assert_eq!(table["key"]["a"].as_str(), Some("1"));
        assert_eq!(table["key"]["b"].as_str(), Some("2"));
    }

    #[test]
    fn config_entry_constructors() {
        let mut t = Table::new();
        t.insert("k".into(), Value::String("v".into()));

        let root = ConfigEntry::root(t);
        assert!(root.path.is_empty());
        assert!(matches!(root.value, ConfigValue::Table(_)));

        let at = ConfigEntry::at_path(
            vec!["my".into(), "key".into()],
            ConfigValue::string("val"),
        );
        assert_eq!(at.path, vec!["my", "key"]);
        assert_eq!(at.value, ConfigValue::String("val".to_string()));
    }

    #[test]
    fn merge_at_path_type_conflict_at_intermediate() {
        let mut table = Table::new();
        table.insert("server".to_string(), Value::String("localhost".into()));

        let result = merge_at_path(
            &mut table,
            &["server".to_string(), "port".to_string()],
            Value::Integer(8080),
        );

        match result {
            Err(ConfigError::TypeConflict { path, .. }) => {
                assert_eq!(path, "server");
            }
            other => panic!("expected TypeConflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_at_path_type_conflict_preserves_full_path() {
        let mut table = Table::new();
        let mut db = Table::new();
        db.insert("pool".to_string(), Value::String("invalid".into()));
        table.insert("database".to_string(), Value::Table(db));

        let result = merge_at_path(
            &mut table,
            &[
                "database".to_string(),
                "pool".to_string(),
                "max".to_string(),
            ],
            Value::Integer(10),
        );

        match result {
            Err(ConfigError::TypeConflict { path, .. }) => {
                assert_eq!(path, "database.pool");
            }
            other => panic!("expected TypeConflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_at_path_type_conflict_three_levels_deep() {
        let mut table = Table::new();
        let mut a = Table::new();
        let mut b = Table::new();
        b.insert("c".to_string(), Value::String("leaf".into()));
        a.insert("b".to_string(), Value::Table(b));
        table.insert("a".to_string(), Value::Table(a));

        let result = merge_at_path(
            &mut table,
            &[
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string(),
            ],
            Value::Integer(42),
        );

        match result {
            Err(ConfigError::TypeConflict { path, .. }) => {
                assert_eq!(path, "a.b.c");
            }
            other => panic!("expected TypeConflict, got {other:?}"),
        }
    }

    #[test]
    fn merge_at_empty_path_overlay_replaces_scalar() {
        let mut table = Table::new();
        table.insert("port".into(), Value::Integer(3000));

        let mut overlay = Table::new();
        overlay.insert("port".into(), Value::Integer(9000));

        merge_at_path(&mut table, &[], Value::Table(overlay)).unwrap();
        assert_eq!(table["port"].as_integer(), Some(9000));
    }

    #[test]
    fn merge_at_path_scalar_replaces_table_at_leaf() {
        let mut table = Table::new();
        let mut nested = Table::new();
        nested.insert("a".into(), Value::String("1".into()));
        table.insert("key".into(), Value::Table(nested));

        let path: Vec<String> = vec!["key".into()];
        merge_at_path(&mut table, &path, Value::String("override".into())).unwrap();

        assert_eq!(table["key"].as_str(), Some("override"));
    }

    // --- ConfigValue and ConfigTable tests ---

    #[test]
    fn config_value_string_constructor() {
        let v = ConfigValue::string("hello");
        assert_eq!(v, ConfigValue::String("hello".to_string()));
    }

    #[test]
    fn config_value_integer_constructor() {
        let v = ConfigValue::integer(42);
        assert_eq!(v, ConfigValue::Integer(42));
    }

    #[test]
    fn config_value_float_constructor() {
        let v = ConfigValue::float(3.14);
        assert_eq!(v, ConfigValue::Float(3.14));
    }

    #[test]
    fn config_value_boolean_constructor() {
        let v = ConfigValue::boolean(true);
        assert_eq!(v, ConfigValue::Boolean(true));
    }

    #[test]
    fn config_value_datetime_constructor() {
        let v = ConfigValue::datetime("2026-01-15T10:30:00");
        assert_eq!(v, ConfigValue::Datetime("2026-01-15T10:30:00".to_string()));
    }

    #[test]
    fn config_table_new_and_insert() {
        let table = ConfigTable::new()
            .insert("host", ConfigValue::string("localhost"))
            .insert("port", ConfigValue::integer(8080));
        assert_eq!(table.0.len(), 2);
        assert_eq!(
            table.0.get("host"),
            Some(&ConfigValue::String("localhost".to_string()))
        );
    }

    // --- ConfigValue <-> toml::Value conversion tests ---

    #[test]
    fn config_value_to_toml_value_scalars() {
        let cv = ConfigValue::string("hello");
        let tv: Value = cv.into();
        assert_eq!(tv, Value::String("hello".into()));

        let cv = ConfigValue::integer(42);
        let tv: Value = cv.into();
        assert_eq!(tv, Value::Integer(42));

        let cv = ConfigValue::boolean(true);
        let tv: Value = cv.into();
        assert_eq!(tv, Value::Boolean(true));

        let cv = ConfigValue::float(3.14);
        let tv: Value = cv.into();
        assert_eq!(tv, Value::Float(3.14));
    }

    #[test]
    fn config_value_to_toml_value_table() {
        let cv = ConfigValue::Table(
            ConfigTable::new().insert("key", ConfigValue::string("val")),
        );
        let tv: Value = cv.into();
        assert!(tv.is_table());
        assert_eq!(tv["key"].as_str(), Some("val"));
    }

    #[test]
    fn config_value_to_toml_value_array() {
        let cv = ConfigValue::Array(vec![
            ConfigValue::integer(1),
            ConfigValue::integer(2),
        ]);
        let tv: Value = cv.into();
        assert!(tv.is_array());
        assert_eq!(tv.as_array().unwrap().len(), 2);
    }

    #[test]
    fn toml_value_to_config_value_scalars() {
        let tv = Value::String("hello".into());
        let cv: ConfigValue = tv.into();
        assert_eq!(cv, ConfigValue::String("hello".to_string()));

        let tv = Value::Integer(42);
        let cv: ConfigValue = tv.into();
        assert_eq!(cv, ConfigValue::Integer(42));

        let tv = Value::Float(2.72);
        let cv: ConfigValue = tv.into();
        assert_eq!(cv, ConfigValue::Float(2.72));

        let tv = Value::Boolean(false);
        let cv: ConfigValue = tv.into();
        assert_eq!(cv, ConfigValue::Boolean(false));
    }

    #[test]
    fn toml_value_to_config_value_array() {
        let arr = Value::Array(vec![Value::Integer(1), Value::String("two".into())]);
        let cv: ConfigValue = arr.into();
        match cv {
            ConfigValue::Array(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], ConfigValue::Integer(1));
                assert_eq!(items[1], ConfigValue::String("two".to_string()));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn config_value_datetime_round_trip() {
        // Valid TOML datetime parses successfully
        let cv = ConfigValue::datetime("2026-01-15T10:30:00");
        let tv: Value = cv.into();
        assert!(tv.is_datetime(), "valid datetime string should parse to Value::Datetime");

        // Round-trip back preserves as Datetime variant
        let cv2: ConfigValue = tv.into();
        match cv2 {
            ConfigValue::Datetime(s) => assert!(s.contains("2026-01-15")),
            other => panic!("expected Datetime, got {other:?}"),
        }
    }

    #[test]
    fn config_value_datetime_invalid_falls_back_to_string() {
        let cv = ConfigValue::datetime("not-a-datetime");
        let tv: Value = cv.into();
        // Invalid datetime falls back to Value::String
        assert_eq!(tv, Value::String("not-a-datetime".into()));
    }

    #[test]
    fn toml_value_to_config_value_nested_table() {
        let mut inner = Table::new();
        inner.insert("a".into(), Value::Integer(1));
        let mut outer = Table::new();
        outer.insert("nested".into(), Value::Table(inner));

        let cv: ConfigValue = Value::Table(outer).into();
        match cv {
            ConfigValue::Table(t) => match t.0.get("nested") {
                Some(ConfigValue::Table(inner)) => {
                    assert_eq!(inner.0.get("a"), Some(&ConfigValue::Integer(1)));
                }
                other => panic!("expected nested table, got {other:?}"),
            },
            other => panic!("expected table, got {other:?}"),
        }
    }
}
