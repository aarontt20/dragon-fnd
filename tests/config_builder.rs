use dragon_fnd::{ConfigBuilder, ConfigEntry, ConfigError, ConfigSource, ConfigValue};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct SimpleConfig {
    name: String,
    port: u16,
}

// -- Custom source for testing with_source() --

#[derive(Debug)]
struct StaticSource {
    entries: Vec<ConfigEntry>,
}

impl StaticSource {
    fn new(entries: Vec<ConfigEntry>) -> Self {
        Self { entries }
    }
}

impl ConfigSource for StaticSource {
    fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
        Ok(self.entries.clone())
    }
}

#[test]
fn builder_with_file_deserializes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"test-app\"\nport = 3000\n").unwrap();

    let config: SimpleConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .build()
        .unwrap();

    assert_eq!(config.name, "test-app");
    assert_eq!(config.port, 3000);
}

#[test]
fn builder_multiple_sources_override() {
    let dir = tempfile::tempdir().unwrap();

    let base = dir.path().join("base.toml");
    std::fs::write(&base, "name = \"base\"\nport = 3000\n").unwrap();

    let overlay = dir.path().join("overlay.toml");
    std::fs::write(&overlay, "port = 9000\n").unwrap();

    let config: SimpleConfig = ConfigBuilder::new()
        .with_file(&base, true)
        .with_file(&overlay, true)
        .build()
        .unwrap();

    assert_eq!(config.name, "base");
    assert_eq!(config.port, 9000);
}

#[test]
fn builder_with_custom_source() {
    let source = StaticSource::new(vec![
        ConfigEntry::at_path(
            vec!["name".into()],
            ConfigValue::string("from-source"),
        ),
        ConfigEntry::at_path(
            vec!["port".into()],
            ConfigValue::integer(4000),
        ),
    ]);

    let config: SimpleConfig = ConfigBuilder::new()
        .with_source(source)
        .build()
        .unwrap();

    assert_eq!(config.name, "from-source");
    assert_eq!(config.port, 4000);
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResolvedConfig {
    host: String,
    url: String,
}

#[test]
fn builder_resolves_references() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "host = \"localhost\"\nurl = \"http://${host}:8080\"\n",
    )
    .unwrap();

    let config: ResolvedConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .build()
        .unwrap();

    assert_eq!(config.url, "http://localhost:8080");
}

#[test]
fn builder_error_propagates_from_source() {
    let result = ConfigBuilder::new()
        .with_file("/nonexistent/config.toml", true)
        .build::<SimpleConfig>();

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ConfigError::FileNotFound(_)
    ));
}

#[test]
fn builder_deserialize_error_missing_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"test\"\n").unwrap();

    let result = ConfigBuilder::new()
        .with_file(&path, true)
        .build::<SimpleConfig>();

    assert!(matches!(result, Err(ConfigError::DeserializeError(_))));
}

#[test]
fn builder_deserialize_error_wrong_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"test\"\nport = \"not_a_number\"\n").unwrap();

    let result = ConfigBuilder::new()
        .with_file(&path, true)
        .build::<SimpleConfig>();

    assert!(matches!(result, Err(ConfigError::DeserializeError(_))));
}

#[test]
fn builder_no_sources() {
    let result = ConfigBuilder::new().build::<SimpleConfig>();
    assert!(matches!(result, Err(ConfigError::DeserializeError(_))));
}

#[test]
fn builder_resolves_cross_source_references() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "host = \"localhost\"\n").unwrap();

    let source = StaticSource::new(vec![ConfigEntry::at_path(
        vec!["url".into()],
        ConfigValue::string("http://${host}:8080"),
    )]);

    let config: ResolvedConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .with_source(source)
        .build()
        .unwrap();

    assert_eq!(config.url, "http://localhost:8080");
}

#[test]
fn builder_propagates_custom_source_error() {
    #[derive(Debug)]
    struct FailingSource;

    impl ConfigSource for FailingSource {
        fn entries(&self) -> Result<Vec<ConfigEntry>, ConfigError> {
            Err(ConfigError::InvalidSeparator)
        }
    }

    let result = ConfigBuilder::new()
        .with_source(FailingSource)
        .build::<SimpleConfig>();

    assert!(matches!(result, Err(ConfigError::InvalidSeparator)));
}
