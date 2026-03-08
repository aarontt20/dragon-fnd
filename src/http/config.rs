use serde::Deserialize;

/// Configuration for the HTTP server.
///
/// All fields have sensible defaults and can be deserialized from TOML:
///
/// ```toml
/// [http]
/// host = "127.0.0.1"
/// port = 3000
/// ```
///
/// Fields are `pub(crate)` — use [`HttpBuilder`](super::HttpBuilder) for
/// programmatic construction or deserialize from TOML via
/// [`HttpBuilder::from_config()`](super::HttpBuilder::from_config).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
        }
    }
}

impl HttpConfig {
    pub(crate) fn addr_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let config = HttpConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn addr_string() {
        let config = HttpConfig {
            host: "127.0.0.1".into(),
            port: 3000,
        };
        assert_eq!(config.addr_string(), "127.0.0.1:3000");
    }

    #[test]
    fn addr_string_defaults() {
        let config = HttpConfig::default();
        assert_eq!(config.addr_string(), "0.0.0.0:8080");
    }

    #[test]
    fn deserialize_minimal_toml() {
        let toml = "";
        let config: HttpConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml = r#"
            host = "127.0.0.1"
            port = 3000
        "#;
        let config: HttpConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn deserialize_host_only() {
        let toml = r#"host = "localhost""#;
        let config: HttpConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn deserialize_port_only() {
        let toml = "port = 9090";
        let config: HttpConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 9090);
    }
}
