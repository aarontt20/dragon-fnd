use super::config::HttpConfig;

/// Fluent builder for HTTP server configuration.
///
/// This is what [`AppContextBuilder::with_http()`](crate::context::AppContextBuilder) accepts.
/// Use [`HttpBuilder::new()`] for programmatic construction, or
/// [`HttpBuilder::from_config()`] to bridge from a deserialized [`HttpConfig`].
#[derive(Debug, Clone, Default)]
#[must_use = "builders do nothing until passed to AppContextBuilder::with_http()"]
pub struct HttpBuilder {
    config: HttpConfig,
}

impl HttpBuilder {
    /// Creates a new builder with defaults: `0.0.0.0:8080`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a builder from a deserialized [`HttpConfig`].
    ///
    /// Bridges TOML configuration into the builder API.
    pub fn from_config(config: HttpConfig) -> Self {
        Self { config }
    }

    /// Sets the host address to bind to.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }

    /// Sets the port to bind to.
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    pub(crate) fn into_config(self) -> HttpConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults() {
        let config = HttpBuilder::new().into_config();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn from_config_preserves_values() {
        let original = HttpConfig {
            host: "127.0.0.1".into(),
            port: 3000,
        };
        let config = HttpBuilder::from_config(original).into_config();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
    }

    #[test]
    fn fluent_chain() {
        let config = HttpBuilder::new()
            .host("localhost")
            .port(9090)
            .into_config();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 9090);
    }

    #[test]
    fn host_accepts_string_types() {
        let _ = HttpBuilder::new().host("127.0.0.1");
        let _ = HttpBuilder::new().host(String::from("127.0.0.1"));
    }

    #[test]
    fn clone() {
        let builder = HttpBuilder::new().host("10.0.0.1").port(4000);
        let cloned = builder.clone();
        let c1 = builder.into_config();
        let c2 = cloned.into_config();
        assert_eq!(c1.host, c2.host);
        assert_eq!(c1.port, c2.port);
    }

    #[test]
    fn debug() {
        let builder = HttpBuilder::new();
        let debug = format!("{:?}", builder);
        assert!(debug.contains("HttpBuilder"));
    }

    #[test]
    fn default_trait() {
        let builder = HttpBuilder::default();
        let config = builder.into_config();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
    }
}
