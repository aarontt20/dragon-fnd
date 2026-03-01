use serde::Serialize;

use super::source::{ConfigEntry, ConfigSource};
use super::ConfigError;

/// A `ConfigSource` that serializes any `T: Serialize` into the config pipeline.
///
/// Most commonly used to feed parsed CLI arguments into `ConfigBuilder`, but works
/// with any serializable struct — hardcoded defaults, remote config responses, test
/// fixtures, etc.
///
/// Serialization happens eagerly at construction time via `toml::Table::try_from()`.
/// `Option::None` fields are omitted from the resulting table, which means they do not
/// override values from lower-priority sources. For maximum robustness against future
/// `toml` crate changes, annotate Option fields with
/// `#[serde(skip_serializing_if = "Option::is_none")]`.
///
/// # Priority
///
/// Sources are merged in registration order — later sources override earlier ones.
/// For CLI-args-override-everything behavior, register `SerdeSource` last:
///
/// ```no_run
/// use dragon_fnd::{ConfigBuilder, SerdeSource};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Deserialize, Serialize)]
/// struct AppConfig {
///     name: String,
///     port: u16,
/// }
///
/// #[derive(Serialize)]
/// struct Args {
///     name: Option<String>,
///     port: Option<u16>,
/// }
///
/// let args = Args { name: Some("app".to_string()), port: None };
/// let config: AppConfig = ConfigBuilder::new()
///     .with_file("config/default.toml", true)
///     .with_env("MYAPP", "__")
///     .with_source(SerdeSource::new(&args).unwrap())  // highest priority
///     .build()
///     .unwrap();
/// ```
///
/// # Limitations
///
/// Values take a roundtrip through TOML's type system (`T → toml::Table → merge →
/// toml::Value → T'`). Types that cannot be represented as a TOML table (bare scalars,
/// bare arrays) will produce `ConfigError::SerializeError` at construction time.
/// `u64` values exceeding `i64::MAX` also fail (TOML integers are `i64`).
///
/// Enums with data variants, unit enum variants, `Vec<T>`, `HashMap<String, V>`,
/// newtype wrappers, and `Option<Option<T>>` all work correctly.
#[derive(Debug, Clone)]
pub struct SerdeSource {
    table: toml::Table,
}

impl SerdeSource {
    /// Serialize `value` into a TOML table for use as a config source.
    ///
    /// Takes `&T` so the caller retains ownership of the original value.
    /// Returns `Err(ConfigError::SerializeError)` if the value cannot be
    /// represented as a TOML table.
    pub fn new<T: Serialize>(value: &T) -> Result<Self, ConfigError> {
        let table =
            toml::Table::try_from(value).map_err(ConfigError::SerializeError)?;
        Ok(Self { table })
    }
}

impl ConfigSource for SerdeSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        Ok(vec![ConfigEntry::root(self.table.clone())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Test structs ---

    #[derive(Serialize)]
    struct Basic {
        name: String,
        port: u16,
    }

    #[derive(Serialize)]
    struct Nested {
        server: Server,
        debug: bool,
    }

    #[derive(Serialize)]
    struct Server {
        host: String,
        port: u16,
    }

    #[derive(Serialize)]
    struct WithOptions {
        name: Option<String>,
        port: Option<u16>,
        verbose: Option<bool>,
    }

    #[derive(Serialize)]
    struct WithNestedOptions {
        server: InnerOptions,
    }

    #[derive(Serialize)]
    struct InnerOptions {
        host: Option<String>,
        port: Option<u16>,
    }

    #[derive(Serialize)]
    struct Empty {}

    #[derive(Serialize)]
    struct WithU64 {
        value: u64,
    }

    #[derive(Serialize)]
    struct WithVec {
        tags: Vec<String>,
        counts: Vec<i64>,
    }

    #[derive(Serialize)]
    enum DataEnum {
        File { path: String },
    }

    #[derive(Serialize)]
    struct WithDataEnum {
        source: DataEnum,
    }

    #[derive(Serialize)]
    enum UnitEnum {
        #[allow(dead_code)]
        Info,
        Warn,
        #[allow(dead_code)]
        Error,
    }

    #[derive(Serialize)]
    struct WithUnitEnum {
        level: UnitEnum,
    }

    #[derive(Serialize)]
    struct WithNestedOption {
        value: Option<Option<String>>,
    }

    // --- Tests ---

    #[test]
    fn basic_struct() {
        let input = Basic {
            name: "app".to_string(),
            port: 8080,
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(source.table["name"].as_str(), Some("app"));
        assert_eq!(source.table["port"].as_integer(), Some(8080));
    }

    #[test]
    fn nested_struct() {
        let input = Nested {
            server: Server {
                host: "localhost".to_string(),
                port: 3000,
            },
            debug: true,
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(source.table["server"]["host"].as_str(), Some("localhost"));
        assert_eq!(source.table["server"]["port"].as_integer(), Some(3000));
        assert_eq!(source.table["debug"].as_bool(), Some(true));
    }

    #[test]
    fn none_fields_omitted() {
        let input = WithOptions {
            name: None,
            port: None,
            verbose: None,
        };
        let source = SerdeSource::new(&input).unwrap();
        assert!(!source.table.contains_key("name"));
        assert!(!source.table.contains_key("port"));
        assert!(!source.table.contains_key("verbose"));
    }

    #[test]
    fn some_fields_present() {
        let input = WithOptions {
            name: Some("test".to_string()),
            port: Some(9090),
            verbose: Some(true),
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(source.table["name"].as_str(), Some("test"));
        assert_eq!(source.table["port"].as_integer(), Some(9090));
        assert_eq!(source.table["verbose"].as_bool(), Some(true));
    }

    #[test]
    fn mixed_none_and_some() {
        let input = WithOptions {
            name: Some("app".to_string()),
            port: None,
            verbose: Some(false),
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(source.table["name"].as_str(), Some("app"));
        assert!(!source.table.contains_key("port"));
        assert_eq!(source.table["verbose"].as_bool(), Some(false));
    }

    #[test]
    fn all_none_nested() {
        let input = WithNestedOptions {
            server: InnerOptions {
                host: None,
                port: None,
            },
        };
        let source = SerdeSource::new(&input).unwrap();
        // Unlike top-level None fields (which are absent entirely), a nested struct
        // with all-None fields produces an empty sub-table. During merge this is
        // harmless: deep_merge leaves any pre-existing keys in the base table intact.
        let server = source.table["server"].as_table().unwrap();
        assert!(server.is_empty());
    }

    #[test]
    fn empty_struct() {
        let input = Empty {};
        let source = SerdeSource::new(&input).unwrap();
        assert!(source.table.is_empty());
    }

    #[test]
    fn vec_fields() {
        let input = WithVec {
            tags: vec!["a".to_string(), "b".to_string()],
            counts: vec![1, 2, 3],
        };
        let source = SerdeSource::new(&input).unwrap();
        let tags = source.table["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].as_str(), Some("a"));
        let counts = source.table["counts"].as_array().unwrap();
        assert_eq!(counts.len(), 3);
        assert_eq!(counts[2].as_integer(), Some(3));
    }

    #[test]
    fn non_struct_errors() {
        // Bare string cannot become a TOML table
        let result = SerdeSource::new(&"just a string");
        assert!(matches!(
            result,
            Err(ConfigError::SerializeError(_))
        ));

        // Vec cannot become a TOML table
        let result = SerdeSource::new(&vec![1, 2, 3]);
        assert!(matches!(
            result,
            Err(ConfigError::SerializeError(_))
        ));
    }

    #[test]
    fn u64_overflow_errors() {
        // TOML integers are i64, so u64 values exceeding i64::MAX fail
        let input = WithU64 { value: u64::MAX };
        let result = SerdeSource::new(&input);
        assert!(matches!(
            result,
            Err(ConfigError::SerializeError(_))
        ));
    }

    #[test]
    fn enum_with_data_serializes_as_nested_table() {
        // Externally-tagged enums with data serialize as nested tables in toml 0.8
        let input = WithDataEnum {
            source: DataEnum::File {
                path: "/tmp".to_string(),
            },
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(
            source.table["source"]["File"]["path"].as_str(),
            Some("/tmp")
        );
    }

    #[test]
    fn unit_enum_variants() {
        let input = WithUnitEnum {
            level: UnitEnum::Warn,
        };
        let source = SerdeSource::new(&input).unwrap();
        assert_eq!(source.table["level"].as_str(), Some("Warn"));
    }

    #[test]
    fn nested_option_some_none_omitted() {
        // Some(None) is caught by toml's UnsupportedNone handler and omitted,
        // same as a direct None field
        let input = WithNestedOption {
            value: Some(None),
        };
        let source = SerdeSource::new(&input).unwrap();
        assert!(!source.table.contains_key("value"));
    }

    #[test]
    fn entries_returns_single_root() {
        let input = Basic {
            name: "test".to_string(),
            port: 80,
        };
        let source = SerdeSource::new(&input).unwrap();
        let entries = source.entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].path.is_empty());
    }
}
