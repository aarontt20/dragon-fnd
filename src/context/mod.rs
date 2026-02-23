use std::marker::PhantomData;

use crate::Error;

#[cfg(feature = "logging")]
use tracing_appender::non_blocking::WorkerGuard;

/// Application context holding configuration and subsystem handles.
///
/// `AppContext<C>` is parameterized by the configuration type `C`, which is
/// deserialized once at build time and available via [`config()`](Self::config).
///
/// # Building
///
/// Use [`AppContext::builder()`] to create an [`AppContextBuilder`], configure it,
/// and call [`build_sync()`](AppContextBuilder::build_sync) to produce the context.
///
/// ```no_run
/// use dragon_fnd::{AppContext, ConfigBuilder};
/// use serde::Deserialize;
///
/// #[derive(Debug, Deserialize)]
/// struct MyConfig { name: String }
///
/// let config: MyConfig = ConfigBuilder::new()
///     .with_file("config.toml", true)
///     .build()
///     .unwrap();
///
/// let ctx = AppContext::builder()
///     .with_config(config)
///     .build_sync()
///     .unwrap();
///
/// println!("{}", ctx.config().name);
/// ```
///
/// # Compile-time safety
///
/// The builder uses type-state to enforce that configuration is provided
/// before building. Attempting to call `build_sync()` without `with_config()`
/// will not compile:
///
/// ```compile_fail
/// use dragon_fnd::context::AppContext;
/// // ERROR: build_sync() is not available on AppContextBuilder<NoConfig, SyncBuild>
/// let _ctx = AppContext::builder().build_sync().unwrap();
/// ```
pub struct AppContext<C> {
    config: C,
    // Future feature-gated fields:
    // #[cfg(feature = "sqlite")]
    // db_pool: Pool,
    #[cfg(feature = "logging")]
    #[allow(dead_code)] // held for Drop — flushes pending log writes
    log_guard: Option<WorkerGuard>,
}

impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContext");
        s.field("config", &self.config);
        #[cfg(feature = "logging")]
        s.field("log_guard", &"<WorkerGuard>");
        s.finish()
    }
}

impl<C> AppContext<C> {
    /// Returns a reference to the application configuration.
    pub fn config(&self) -> &C {
        &self.config
    }
}

impl AppContext<()> {
    /// Creates a new builder for constructing an `AppContext`.
    pub fn builder() -> AppContextBuilder<NoConfig> {
        AppContextBuilder {
            cfg: NoConfig,
            _async: PhantomData,
            #[cfg(feature = "logging")]
            logging: None,
        }
    }
}

// -- Type-state markers --

/// Marker: no configuration has been provided yet.
#[doc(hidden)]
pub struct NoConfig;

/// Marker: configuration of type `C` has been provided.
#[doc(hidden)]
pub struct Configured<C>(C);

/// Marker: only synchronous subsystems are registered.
#[doc(hidden)]
pub struct SyncBuild;

// Future:
// #[doc(hidden)]
// pub struct AsyncBuild;

/// Builder for [`AppContext`], using type-state to enforce correct construction.
///
/// The builder tracks two type-level dimensions:
/// - `Cfg`: whether configuration has been provided (`NoConfig` or `Configured<C>`)
/// - `Async`: whether async subsystems are registered (`SyncBuild` or future `AsyncBuild`)
#[must_use = "builders do nothing until .build_sync() is called"]
pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
    #[cfg(feature = "logging")]
    logging: Option<crate::logging::LoggingBuilder>,
}

impl<Cfg, A> std::fmt::Debug for AppContextBuilder<Cfg, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContextBuilder");
        #[cfg(feature = "logging")]
        s.field("logging", &self.logging.is_some());
        s.finish()
    }
}

impl<A> AppContextBuilder<NoConfig, A> {
    /// Provides the application configuration, transitioning the builder
    /// from `NoConfig` to `Configured<C>`.
    pub fn with_config<C>(self, config: C) -> AppContextBuilder<Configured<C>, A> {
        AppContextBuilder {
            cfg: Configured(config),
            _async: PhantomData,
            #[cfg(feature = "logging")]
            logging: self.logging,
        }
    }
}

#[cfg(feature = "logging")]
impl<Cfg, A> AppContextBuilder<Cfg, A> {
    /// Registers a logging configuration to be initialized at build time.
    ///
    /// Available on all builder states — you can register logging before or after
    /// providing configuration.
    pub fn with_logging(mut self, builder: crate::logging::LoggingBuilder) -> Self {
        self.logging = Some(builder);
        self
    }
}

impl<C> AppContextBuilder<Configured<C>, SyncBuild> {
    /// Builds the application context, initializing all registered subsystems.
    ///
    /// Returns an error if any subsystem fails to initialize.
    pub fn build_sync(self) -> Result<AppContext<C>, Error> {
        #[cfg(feature = "logging")]
        let log_guard = match self.logging {
            Some(builder) => crate::logging::init_logging(&builder.into_config())?,
            None => None,
        };

        Ok(AppContext {
            config: self.cfg.0,
            #[cfg(feature = "logging")]
            log_guard,
        })
    }
}
