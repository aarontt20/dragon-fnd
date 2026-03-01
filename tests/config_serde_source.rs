use dragon_fnd::{ConfigBuilder, ConfigError, SerdeSource};
use serde::{Deserialize, Serialize};

// --- Test config types ---

#[derive(Debug, Deserialize, PartialEq)]
struct SimpleConfig {
    name: String,
    port: u16,
}

#[derive(Debug, Serialize)]
struct SimpleArgs {
    name: String,
    port: u16,
}

#[derive(Debug, Deserialize, PartialEq)]
struct NestedConfig {
    app: String,
    server: ServerConfig,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ServerConfig {
    host: String,
    port: u16,
}

#[derive(Debug, Serialize)]
struct NestedArgs {
    app: Option<String>,
    server: ServerOverride,
}

#[derive(Debug, Serialize)]
struct ServerOverride {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ConfigWithVec {
    name: String,
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ArgsWithVec {
    name: String,
    tags: Vec<String>,
}

// --- Tests ---

#[test]
fn serde_source_standalone() {
    let args = SimpleArgs {
        name: "my-app".to_string(),
        port: 9090,
    };

    let config: SimpleConfig = ConfigBuilder::new()
        .with_source(SerdeSource::new(&args).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.name, "my-app");
    assert_eq!(config.port, 9090);
}

#[test]
fn file_then_serde_override() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"from-file\"\nport = 3000\n").unwrap();

    // Only override port, leave name as None (should keep file value)
    #[derive(Debug, Serialize)]
    struct PartialArgs {
        name: Option<String>,
        port: Option<u16>,
    }

    let args = PartialArgs {
        name: None,
        port: Some(9090),
    };

    let config: SimpleConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .with_source(SerdeSource::new(&args).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.name, "from-file"); // kept from file
    assert_eq!(config.port, 9090); // overridden by args
}

#[test]
fn serde_source_with_nested_structs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "app = \"default\"\n\n[server]\nhost = \"0.0.0.0\"\nport = 3000\n",
    )
    .unwrap();

    // Override only the port, keep host and app from file
    let args = NestedArgs {
        app: None,
        server: ServerOverride {
            host: None,
            port: Some(8080),
        },
    };

    let config: NestedConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .with_source(SerdeSource::new(&args).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.app, "default"); // kept from file
    assert_eq!(config.server.host, "0.0.0.0"); // kept from file
    assert_eq!(config.server.port, 8080); // overridden by args
}

#[test]
fn serde_source_with_variable_references() {
    // Sanity check: existing ${...} resolution pipeline works when
    // SerdeSource participates in the source stack
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "host = \"localhost\"\nurl = \"http://${host}:${port}\"\n",
    )
    .unwrap();

    #[derive(Debug, Serialize)]
    struct PortArgs {
        port: u16,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct ResolvedConfig {
        host: String,
        port: u16,
        url: String,
    }

    let args = PortArgs { port: 9090 };

    let config: ResolvedConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .with_source(SerdeSource::new(&args).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.url, "http://localhost:9090");
}

#[test]
fn serde_source_serialize_error() {
    // Non-struct type produces ConfigError::SerializeError at new(), before build()
    let result = SerdeSource::new(&"bare string");
    assert!(matches!(result, Err(ConfigError::SerializeError(_))));
}

#[test]
fn three_way_merge() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "name = \"from-file\"\nport = 3000\n").unwrap();

    // Env overrides file, args override env
    // We can't easily set env vars in parallel tests, so simulate with
    // two SerdeSource layers instead (same merge semantics)
    #[derive(Debug, Serialize)]
    struct Layer2 {
        name: Option<String>,
        port: Option<u16>,
    }

    #[derive(Debug, Serialize)]
    struct Layer3 {
        name: Option<String>,
        port: Option<u16>,
    }

    let layer2 = Layer2 {
        name: Some("from-layer2".to_string()),
        port: None, // keep file value
    };

    let layer3 = Layer3 {
        name: None,  // keep layer2 value
        port: Some(9999),
    };

    let config: SimpleConfig = ConfigBuilder::new()
        .with_file(&path, true)
        .with_source(SerdeSource::new(&layer2).unwrap())
        .with_source(SerdeSource::new(&layer3).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.name, "from-layer2"); // file -> layer2 override -> layer3 keeps
    assert_eq!(config.port, 9999); // file -> layer2 keeps -> layer3 override
}

#[test]
fn serde_source_with_vec_fields() {
    let args = ArgsWithVec {
        name: "tagged-app".to_string(),
        tags: vec!["web".to_string(), "api".to_string()],
    };

    let config: ConfigWithVec = ConfigBuilder::new()
        .with_source(SerdeSource::new(&args).unwrap())
        .build()
        .unwrap();

    assert_eq!(config.name, "tagged-app");
    assert_eq!(config.tags, vec!["web", "api"]);
}
