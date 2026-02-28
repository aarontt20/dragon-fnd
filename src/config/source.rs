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

    pub fn datetime(s: impl Into<String>) -> Result<Self, ConfigError> {
        let s = s.into();
        s.parse::<toml::value::Datetime>()
            .map(|_| Self::Datetime(s.clone()))
            .map_err(|_| ConfigError::InvalidDatetime(s))
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

    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.0.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ConfigValue)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for ConfigTable {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoIterator for ConfigTable {
    type Item = (String, ConfigValue);
    type IntoIter = std::collections::btree_map::IntoIter<String, ConfigValue>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl ConfigValue {
    /// Convert to the internal `toml::Value` representation.
    ///
    /// This is `pub(crate)` because external consumers should not need `toml::Value` —
    /// `ConfigValue` exists precisely to avoid that dependency.
    pub(crate) fn into_toml_value(self) -> Result<Value, ConfigError> {
        match self {
            ConfigValue::String(s) => Ok(Value::String(s)),
            ConfigValue::Integer(i) => Ok(Value::Integer(i)),
            ConfigValue::Float(f) => Ok(Value::Float(f)),
            ConfigValue::Boolean(b) => Ok(Value::Boolean(b)),
            ConfigValue::Datetime(s) => s
                .parse::<toml::value::Datetime>()
                .map(Value::Datetime)
                .map_err(|_| ConfigError::InvalidDatetime(s)),
            ConfigValue::Array(arr) => arr
                .into_iter()
                .map(|v| v.into_toml_value())
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            ConfigValue::Table(table) => table
                .0
                .into_iter()
                .map(|(k, v)| v.into_toml_value().map(|v| (k, v)))
                .collect::<Result<Table, _>>()
                .map(Value::Table),
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
    pub(crate) fn root(table: Table) -> Self {
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
        // Preceding match ensures a Table exists at `first` (either pre-existing or freshly inserted).
        // This branch should never execute, but return an error instead of panicking.
        _ => {
            return Err(ConfigError::TypeConflict {
                path: first.clone(),
                existing: "unknown".to_string(),
                incoming: "table".to_string(),
            });
        }
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
        let v = ConfigValue::float(2.21);
        assert_eq!(v, ConfigValue::Float(2.21));
    }

    #[test]
    fn config_value_boolean_constructor() {
        let v = ConfigValue::boolean(true);
        assert_eq!(v, ConfigValue::Boolean(true));
    }

    #[test]
    fn config_value_datetime_constructor() {
        let v = ConfigValue::datetime("2026-01-15T10:30:00").unwrap();
        assert_eq!(v, ConfigValue::Datetime("2026-01-15T10:30:00".to_string()));
    }

    #[test]
    fn config_value_datetime_constructor_rejects_invalid() {
        let err = ConfigValue::datetime("not-a-datetime").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidDatetime(_)));
    }

    #[test]
    fn config_table_new_and_insert() {
        let table = ConfigTable::new()
            .insert("host", ConfigValue::string("localhost"))
            .insert("port", ConfigValue::integer(8080));
        assert_eq!(table.len(), 2);
        assert_eq!(
            table.get("host"),
            Some(&ConfigValue::String("localhost".to_string()))
        );
    }

    // --- ConfigValue <-> toml::Value conversion tests ---

    #[test]
    fn config_value_to_toml_value_scalars() {
        let tv = ConfigValue::string("hello").into_toml_value().unwrap();
        assert_eq!(tv, Value::String("hello".into()));

        let tv = ConfigValue::integer(42).into_toml_value().unwrap();
        assert_eq!(tv, Value::Integer(42));

        let tv = ConfigValue::boolean(true).into_toml_value().unwrap();
        assert_eq!(tv, Value::Boolean(true));

        let tv = ConfigValue::float(2.21).into_toml_value().unwrap();
        assert_eq!(tv, Value::Float(2.21));
    }

    #[test]
    fn config_value_to_toml_value_table() {
        let cv = ConfigValue::Table(
            ConfigTable::new().insert("key", ConfigValue::string("val")),
        );
        let tv = cv.into_toml_value().unwrap();
        assert!(tv.is_table());
        assert_eq!(tv["key"].as_str(), Some("val"));
    }

    #[test]
    fn config_value_to_toml_value_array() {
        let cv = ConfigValue::Array(vec![
            ConfigValue::integer(1),
            ConfigValue::integer(2),
        ]);
        let tv = cv.into_toml_value().unwrap();
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
        let cv = ConfigValue::datetime("2026-01-15T10:30:00").unwrap();
        let tv = cv.into_toml_value().unwrap();
        assert!(tv.is_datetime(), "valid datetime string should parse to Value::Datetime");

        // Round-trip back preserves as Datetime variant
        let cv2: ConfigValue = tv.into();
        match cv2 {
            ConfigValue::Datetime(s) => assert!(s.contains("2026-01-15")),
            other => panic!("expected Datetime, got {other:?}"),
        }
    }

    #[test]
    fn config_value_datetime_invalid_returns_error() {
        // Constructor rejects invalid datetime
        let err = ConfigValue::datetime("not-a-datetime").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidDatetime(_)));

        // Direct enum construction also errors during conversion
        let cv = ConfigValue::Datetime("not-a-datetime".to_string());
        let err = cv.into_toml_value().unwrap_err();
        assert!(matches!(err, ConfigError::InvalidDatetime(_)));
    }

    #[test]
    fn toml_value_to_config_value_nested_table() {
        let mut inner = Table::new();
        inner.insert("a".into(), Value::Integer(1));
        let mut outer = Table::new();
        outer.insert("nested".into(), Value::Table(inner));

        let cv: ConfigValue = Value::Table(outer).into();
        match cv {
            ConfigValue::Table(t) => match t.get("nested") {
                Some(ConfigValue::Table(inner)) => {
                    assert_eq!(inner.get("a"), Some(&ConfigValue::Integer(1)));
                }
                other => panic!("expected nested table, got {other:?}"),
            },
            other => panic!("expected table, got {other:?}"),
        }
    }
}
