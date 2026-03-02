mod builder;
mod config;
mod error;
mod init;

pub(crate) use init::init_pool;

pub use builder::SqliteBuilder;
pub use config::{JournalMode, SqliteConfig};
pub use error::SqliteError;

// Re-export pool type so users can write dragon_fnd::sqlite::SqlitePool
// without depending on sqlx directly.
pub use sqlx::SqlitePool;
