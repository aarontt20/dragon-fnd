//! Variable reference resolution example.
//!
//! Demonstrates the `${path.to.field}` syntax for referencing other config
//! values. References are resolved after all sources merge, using topological
//! sort to handle dependency chains.
//!
//! ```sh
//! cargo run --example variable_resolution
//! ```

use dragon_fnd::ConfigBuilder;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    app: AppSection,
    server: ServerSection,
    database: DatabaseSection,
}

#[derive(Debug, Deserialize)]
struct AppSection {
    name: String,
    env: String,
    version: String,
    port: u16,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ServerSection {
    host: String,
    bind: String,
    banner: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DatabaseSection {
    host: String,
    port: u16,
    name: String,
    url: String,
}

fn main() -> Result<(), dragon_fnd::Error> {
    let config: AppConfig = ConfigBuilder::new()
        .with_file("examples/config/references.toml", true)
        .build()?;

    println!("=== After variable resolution ===");
    println!();
    println!("Primitives (no references):");
    println!("  app.name    = {}", config.app.name);
    println!("  app.env     = {}", config.app.env);
    println!("  app.version = {}", config.app.version);
    println!("  app.port    = {}", config.app.port);
    println!();
    println!("String interpolation (multiple refs in one value):");
    println!("  server.bind   = {}", config.server.bind);
    println!("  server.banner = {}", config.server.banner);
    println!();
    println!("Reference chain (db.name → app.name, db.url → db.name):");
    println!("  database.name = {}", config.database.name);
    println!("  database.url  = {}", config.database.url);

    Ok(())
}
