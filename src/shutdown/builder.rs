use std::time::Duration;

const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(30);

/// Fluent builder for shutdown configuration.
///
/// This is what [`AppContextBuilder::with_shutdown()`](crate::context::AppContextBuilder) accepts.
/// There is no `from_config()` — this subsystem has no serde-deserializable config struct.
/// Grace period is set programmatically via the builder.
#[derive(Debug, Clone)]
#[must_use = "builders do nothing until passed to AppContextBuilder::with_shutdown()"]
pub struct ShutdownBuilder {
    pub(crate) grace_period: Duration,
}

impl Default for ShutdownBuilder {
    fn default() -> Self {
        Self {
            grace_period: DEFAULT_GRACE_PERIOD,
        }
    }
}

impl ShutdownBuilder {
    /// Creates a new builder with a 30-second default grace period.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total time budget for all cleanup hooks to complete.
    pub fn grace_period(mut self, duration: Duration) -> Self {
        self.grace_period = duration;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_grace_period() {
        let builder = ShutdownBuilder::new();
        assert_eq!(builder.grace_period, Duration::from_secs(30));
    }

    #[test]
    fn custom_grace_period() {
        let builder = ShutdownBuilder::new().grace_period(Duration::from_secs(60));
        assert_eq!(builder.grace_period, Duration::from_secs(60));
    }

    #[test]
    fn clone() {
        let builder = ShutdownBuilder::new().grace_period(Duration::from_secs(10));
        let cloned = builder.clone();
        assert_eq!(builder.grace_period, cloned.grace_period);
    }

    #[test]
    fn debug() {
        let builder = ShutdownBuilder::new();
        let debug = format!("{:?}", builder);
        assert!(debug.contains("ShutdownBuilder"));
        assert!(debug.contains("grace_period"));
    }

    #[test]
    fn default_trait() {
        let builder = ShutdownBuilder::default();
        assert_eq!(builder.grace_period, Duration::from_secs(30));
    }
}
