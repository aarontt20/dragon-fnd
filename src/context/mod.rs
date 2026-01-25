//! Application context for managing shared application state.

use crate::Error;

/// Central application context holding configuration and shared resources.
///
/// Generic over the configuration type `C`, which is deserialized once at build time.
/// Access configuration via [`config()`](Self::config) for zero-cost reads.
///
/// ## Example
///
/// ```no_run
/// use dragon_fnd::{AppContext, Config};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct MyConfig {
///     name: String,
///     port: u16,
/// }
///
/// let ctx = AppContext::builder()
///     .with_config(
///         Config::builder()
///             .with_file("config.toml", true)
///             .build::<MyConfig>()?
///     )
///     .build()?;
///
/// let config = ctx.config();  // &MyConfig, zero-cost
/// # Ok::<(), dragon_fnd::Error>(())
/// ```
#[derive(Debug)]
pub struct AppContext<C> {
    config: C,
}

impl<C> AppContext<C> {
    /// Returns a reference to the configuration.
    ///
    /// This is a zero-cost operation since the config was deserialized at build time.
    pub fn config(&self) -> &C {
        &self.config
    }
}

impl AppContext<()> {
    /// Creates a new builder for constructing an `AppContext`.
    pub fn builder() -> AppContextBuilder<()> {
        AppContextBuilder { config: None }
    }
}

/// Builder for constructing an [`AppContext`].
///
/// The builder starts with no config (`AppContextBuilder<()>`) and transitions
/// to `AppContextBuilder<C>` when [`with_config`](Self::with_config) is called.
#[derive(Debug)]
#[must_use = "builders do nothing until .build() is called"]
pub struct AppContextBuilder<C> {
    config: Option<C>,
}

impl AppContextBuilder<()> {
    /// Attaches a configuration to the application context.
    ///
    /// The configuration should be the result of [`Config::builder().build()`](crate::Config::build).
    pub fn with_config<C>(self, config: C) -> AppContextBuilder<C> {
        AppContextBuilder {
            config: Some(config),
        }
    }
}

impl<C> AppContextBuilder<C> {
    /// Builds the `AppContext`.
    ///
    /// Returns an error if no configuration was provided.
    pub fn build(self) -> Result<AppContext<C>, Error> {
        Ok(AppContext {
            config: self.config.ok_or(Error::MissingConfig)?,
        })
    }
}
