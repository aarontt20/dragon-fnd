//! Environment variable layering example.
//!
//! Loads `examples/config/default.toml` as a base, then overlays any environment
//! variables matching the prefix `DRAGON_EXAMPLE` with separator `__`.
//!
//! Try it:
//!
//! ```sh
//! cargo run --example env_override
//! DRAGON_EXAMPLE__APP__DEBUG=true cargo run --example env_override
//! DRAGON_EXAMPLE__DATABASE__PORT=3306 cargo run --example env_override
//! ```

use dragon_fnd::ConfigBuilder;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    app: AppSection,
    database: DatabaseSection,
}

#[derive(Debug, Deserialize)]
struct AppSection {
    name: String,
    debug: bool,
}

#[derive(Debug, Deserialize)]
struct DatabaseSection {
    host: String,
    port: u16,
    name: String,
    url: String,
}

fn main() -> Result<(), dragon_fnd::Error> {
    // File defaults first, env vars second (higher priority).
    // Later sources override earlier ones during merge.
    let config: AppConfig = ConfigBuilder::new()
        .with_file("examples/config/default.toml", true)
        .with_env("DRAGON_EXAMPLE", "__")
        .build()?;

    println!("=== Resolved config ===");
    println!("app.name  = {}", config.app.name);
    println!("app.debug = {}", config.app.debug);
    println!("db.host   = {}", config.database.host);
    println!("db.port   = {}", config.database.port);
    println!("db.name   = {}", config.database.name);
    println!("db.url    = {}", config.database.url);
    println!();
    println!("Try overriding a value:");
    println!("  DRAGON_EXAMPLE__APP__DEBUG=true cargo run --example env_override");

    Ok(())
}
