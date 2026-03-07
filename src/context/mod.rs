use std::any::{Any, TypeId};
use std::collections::HashMap;
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
///
/// Registering an async subsystem like SQLite transitions the builder to
/// `AsyncBuild`, making `build_sync()` unavailable — you must use `build().await`:
///
/// ```compile_fail
/// use dragon_fnd::context::AppContext;
/// use dragon_fnd::sqlite::SqliteBuilder;
/// // ERROR: build_sync() is not available on AppContextBuilder<_, AsyncBuild>
/// let _ctx = AppContext::builder()
///     .with_config(())
///     .with_sqlite(SqliteBuilder::new("test.db"))
///     .build_sync();
/// ```
///
/// The same applies to `with_shutdown()`:
///
/// ```compile_fail
/// use dragon_fnd::context::AppContext;
/// use dragon_fnd::shutdown::ShutdownBuilder;
/// // ERROR: build_sync() is not available on AppContextBuilder<_, AsyncBuild>
/// let _ctx = AppContext::builder()
///     .with_config(())
///     .with_shutdown(ShutdownBuilder::new())
///     .build_sync();
/// ```
pub struct AppContext<C> {
    // Fields ordered for correct drop sequence (Rust drops in declaration order):
    // 1. config — pure data, no cleanup
    // 2. extensions — user-provided, drop before subsystems
    // 3. [future: http_handle] — drop server before database
    // 4. sqlite_pool — close database connections (logging during drop is captured)
    // 5. log_guard — MUST be last, flushes pending log writes
    config: C,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "shutdown")]
    shutdown: Option<crate::shutdown::Shutdown>,
    #[cfg(feature = "sqlite")]
    sqlite_pool: Option<sqlx::SqlitePool>,
    #[cfg(feature = "logging")]
    #[allow(dead_code)] // held for Drop — flushes pending log writes
    log_guard: Option<WorkerGuard>,
}

impl<C: std::fmt::Debug> std::fmt::Debug for AppContext<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContext");
        s.field("config", &self.config);
        s.field("extensions", &self.extensions.len());
        #[cfg(feature = "shutdown")]
        s.field("shutdown", &self.shutdown.is_some());
        #[cfg(feature = "sqlite")]
        s.field("sqlite_pool", &self.sqlite_pool.is_some());
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

    /// Returns a reference to an extension of type `T`, if one was registered.
    pub fn extension<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.extensions
            .get(&TypeId::of::<T>())
            .and_then(|boxed| boxed.downcast_ref::<T>())
    }

    /// Returns a reference to the shutdown handle, if one was registered.
    ///
    /// Returns `None` if `with_shutdown()` was not called during builder construction.
    #[cfg(feature = "shutdown")]
    pub fn shutdown(&self) -> Option<&crate::shutdown::Shutdown> {
        self.shutdown.as_ref()
    }

    /// Returns a reference to the SQLite connection pool, if one was registered.
    ///
    /// Returns `None` if `with_sqlite()` was not called during builder construction.
    #[cfg(feature = "sqlite")]
    pub fn sqlite(&self) -> Option<&sqlx::SqlitePool> {
        self.sqlite_pool.as_ref()
    }
}

impl AppContext<()> {
    /// Creates a new builder for constructing an `AppContext`.
    pub fn builder() -> AppContextBuilder<NoConfig> {
        AppContextBuilder {
            cfg: NoConfig,
            _async: PhantomData,
            extensions: HashMap::new(),
            #[cfg(feature = "logging")]
            logging: None,
            #[cfg(feature = "sqlite")]
            sqlite: None,
            #[cfg(feature = "shutdown")]
            shutdown: None,
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

/// Marker: at least one async subsystem is registered.
#[cfg(feature = "_async")]
#[doc(hidden)]
pub struct AsyncBuild;

/// Builder for [`AppContext`], using type-state to enforce correct construction.
///
/// The builder tracks two type-level dimensions:
/// - `Cfg`: whether configuration has been provided (`NoConfig` or `Configured<C>`)
/// - `Async`: whether async subsystems are registered (`SyncBuild` or `AsyncBuild`)
#[must_use = "builders do nothing until .build_sync() or .build().await is called"]
pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "logging")]
    logging: Option<crate::logging::LoggingBuilder>,
    #[cfg(feature = "sqlite")]
    sqlite: Option<crate::sqlite::SqliteBuilder>,
    #[cfg(feature = "shutdown")]
    shutdown: Option<crate::shutdown::ShutdownBuilder>,
}

impl<Cfg, A> std::fmt::Debug for AppContextBuilder<Cfg, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("AppContextBuilder");
        s.field("extensions", &self.extensions.len());
        #[cfg(feature = "logging")]
        s.field("logging", &self.logging.is_some());
        #[cfg(feature = "sqlite")]
        s.field("sqlite", &self.sqlite.is_some());
        #[cfg(feature = "shutdown")]
        s.field("shutdown", &self.shutdown.is_some());
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
            extensions: self.extensions,
            #[cfg(feature = "logging")]
            logging: self.logging,
            #[cfg(feature = "sqlite")]
            sqlite: self.sqlite,
            #[cfg(feature = "shutdown")]
            shutdown: self.shutdown,
        }
    }
}

impl<Cfg, A> AppContextBuilder<Cfg, A> {
    /// Stores an extension value, retrievable later via [`AppContext::extension()`].
    ///
    /// Available on all builder states. If the same type is registered twice,
    /// the last value wins.
    pub fn with_extension<T: Send + Sync + 'static>(mut self, ext: T) -> Self {
        self.extensions.insert(TypeId::of::<T>(), Box::new(ext));
        self
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

// -- SyncBuild → AsyncBuild transition --

#[cfg(feature = "_async")]
impl<Cfg> AppContextBuilder<Cfg, SyncBuild> {
    /// Converts this builder from SyncBuild to AsyncBuild, propagating all fields.
    ///
    /// Centralizes field propagation so each new async subsystem adds one line
    /// here instead of editing every other subsystem's transition method.
    fn into_async(self) -> AppContextBuilder<Cfg, AsyncBuild> {
        AppContextBuilder {
            cfg: self.cfg,
            _async: PhantomData,
            extensions: self.extensions,
            #[cfg(feature = "logging")]
            logging: self.logging,
            #[cfg(feature = "sqlite")]
            sqlite: self.sqlite,
            #[cfg(feature = "shutdown")]
            shutdown: self.shutdown,
        }
    }
}

// -- with_sqlite: SyncBuild → AsyncBuild --

#[cfg(feature = "sqlite")]
impl<Cfg> AppContextBuilder<Cfg, SyncBuild> {
    /// Registers a SQLite database pool to be initialized at build time.
    ///
    /// Transitions the builder to `AsyncBuild` — `build_sync()` is no longer
    /// available, and `build().await` must be used instead.
    pub fn with_sqlite(
        self,
        builder: crate::sqlite::SqliteBuilder,
    ) -> AppContextBuilder<Cfg, AsyncBuild> {
        let mut b = self.into_async();
        b.sqlite = Some(builder);
        b
    }
}

// -- with_sqlite: AsyncBuild → AsyncBuild --

#[cfg(feature = "sqlite")]
impl<Cfg> AppContextBuilder<Cfg, AsyncBuild> {
    /// Registers a SQLite database pool to be initialized at build time.
    ///
    /// Available when the builder is already in `AsyncBuild` state (e.g.,
    /// after registering another async subsystem).
    pub fn with_sqlite(mut self, builder: crate::sqlite::SqliteBuilder) -> Self {
        self.sqlite = Some(builder);
        self
    }
}

// -- with_shutdown: SyncBuild → AsyncBuild --

#[cfg(feature = "shutdown")]
impl<Cfg> AppContextBuilder<Cfg, SyncBuild> {
    /// Registers shutdown handling to be initialized at build time.
    ///
    /// Transitions the builder to `AsyncBuild` — `build_sync()` is no longer
    /// available, and `build().await` must be used instead.
    pub fn with_shutdown(
        self,
        builder: crate::shutdown::ShutdownBuilder,
    ) -> AppContextBuilder<Cfg, AsyncBuild> {
        let mut b = self.into_async();
        b.shutdown = Some(builder);
        b
    }
}

// -- with_shutdown: AsyncBuild → AsyncBuild --

#[cfg(feature = "shutdown")]
impl<Cfg> AppContextBuilder<Cfg, AsyncBuild> {
    /// Registers shutdown handling to be initialized at build time.
    ///
    /// Available when the builder is already in `AsyncBuild` state (e.g.,
    /// after registering another async subsystem).
    pub fn with_shutdown(mut self, builder: crate::shutdown::ShutdownBuilder) -> Self {
        self.shutdown = Some(builder);
        self
    }
}

// -- build_sync: SyncBuild only --

impl<C> AppContextBuilder<Configured<C>, SyncBuild> {
    /// Builds the application context, initializing all registered subsystems.
    ///
    /// Only available when no async subsystems are registered. If you need
    /// SQLite or other async subsystems, use [`build()`](AppContextBuilder::build) instead.
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
            extensions: self.extensions,
            #[cfg(feature = "shutdown")]
            shutdown: None,
            #[cfg(feature = "sqlite")]
            sqlite_pool: None,
            #[cfg(feature = "logging")]
            log_guard,
        })
    }
}

// -- build: AsyncBuild only --

#[cfg(feature = "_async")]
impl<C> AppContextBuilder<Configured<C>, AsyncBuild> {
    /// Builds the application context, initializing all registered subsystems.
    ///
    /// Initializes subsystems in dependency order: logging first (so other
    /// subsystems can log during init), then shutdown, then database.
    ///
    /// Returns an error if any subsystem fails to initialize.
    pub async fn build(self) -> Result<AppContext<C>, Error> {
        // Init logging (sync) — before other subsystems so they can log
        #[cfg(feature = "logging")]
        let log_guard = match self.logging {
            Some(builder) => crate::logging::init_logging(&builder.into_config())?,
            None => None,
        };

        // Init shutdown (async — requires tokio runtime)
        // TODO: Replace hardcoded init sequence with topological sort when HTTP subsystem lands
        #[cfg(feature = "shutdown")]
        let shutdown = match self.shutdown {
            Some(builder) => Some(crate::shutdown::init_shutdown(builder)?),
            None => None,
        };

        // Init sqlite (async)
        #[cfg(feature = "sqlite")]
        let sqlite_pool = match self.sqlite {
            Some(builder) => Some(crate::sqlite::init_pool(&builder.into_config()).await?),
            None => None,
        };

        Ok(AppContext {
            config: self.cfg.0,
            extensions: self.extensions,
            #[cfg(feature = "shutdown")]
            shutdown,
            #[cfg(feature = "sqlite")]
            sqlite_pool,
            #[cfg(feature = "logging")]
            log_guard,
        })
    }
}
