use std::marker::PhantomData;

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
///     .build_sync();
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
/// let _ctx = AppContext::builder().build_sync();
/// ```
pub struct AppContext<C> {
    config: C,
    // Future feature-gated fields:
    // #[cfg(feature = "logging")]
    // log_guard: WorkerGuard,
    // #[cfg(feature = "sqlite")]
    // db_pool: Pool,
}

impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppContext")
            .field("config", &self.config)
            // Future: #[cfg(feature = "logging")]
            // .field("log_guard", &"<WorkerGuard>")
            .finish()
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
}

impl<Cfg, A> std::fmt::Debug for AppContextBuilder<Cfg, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppContextBuilder").finish()
    }
}

impl AppContextBuilder<NoConfig, SyncBuild> {
    /// Provides the application configuration, transitioning the builder
    /// from `NoConfig` to `Configured<C>`.
    pub fn with_config<C>(self, config: C) -> AppContextBuilder<Configured<C>, SyncBuild> {
        AppContextBuilder {
            cfg: Configured(config),
            _async: PhantomData,
        }
    }
}

impl<C> AppContextBuilder<Configured<C>, SyncBuild> {
    /// Builds the application context.
    ///
    /// This method is infallible — the type-state guarantees that all required
    /// fields are present. When a fallible sync subsystem is added, this
    /// signature will change to return `Result`.
    pub fn build_sync(self) -> AppContext<C> {
        AppContext {
            config: self.cfg.0,
        }
    }
}
