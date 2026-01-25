//! Foundation library providing configuration management and application context.
//!
//! # Example
//!
//! ```no_run
//! use dragon_fnd::{AppContext, Config};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct MyConfig {
//!     name: String,
//!     port: u16,
//! }
//!
//! let ctx = AppContext::builder()
//!     .with_config(
//!         Config::builder()
//!             .with_file("config/default.toml", true)
//!             .with_file("config/local.toml", false)  // optional override
//!             .build::<MyConfig>()?,
//!     )
//!     .build()?;
//!
//! let config = ctx.config();  // &MyConfig, zero-cost
//! # Ok::<(), dragon_fnd::Error>(())
//! ```
//!
//! Configuration files support `${path.to.field}` variable references.
//! See [`Config`] for details.

pub mod config;
pub mod context;
mod error;

pub use config::{Config, ConfigError};
pub use context::AppContext;
pub use error::Error;
