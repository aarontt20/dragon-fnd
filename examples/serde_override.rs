//! SerdeSource override example — the CLI-args pattern.
//!
//! Demonstrates how a serializable struct (simulating parsed CLI arguments)
//! can override file-based config. `Option::None` fields are omitted during
//! serialization, so only explicitly-set values override file defaults.
//!
//! ```sh
//! cargo run --example serde_override
//! ```

use dragon_fnd::{ConfigBuilder, SerdeSource};
use serde::{Deserialize, Serialize};

// --- Config target (what we deserialize into) ---

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
#[allow(dead_code)]
struct DatabaseSection {
    host: String,
    port: u16,
    name: String,
    url: String,
}

// --- CLI args overlay (what simulates parsed arguments) ---
//
// The struct mirrors the config shape so it merges naturally.
// Option fields with skip_serializing_if let us represent "not provided"
// — those fields won't appear in the serialized table, so file defaults
// are preserved.

#[derive(Serialize)]
struct Args {
    app: AppArgs,
}

#[derive(Serialize)]
struct AppArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    debug: Option<bool>,
}

fn main() -> Result<(), dragon_fnd::Error> {
    // Simulate parsed CLI: user passed --debug but not --name
    let args = Args {
        app: AppArgs {
            name: None,          // not provided → file default preserved
            debug: Some(true),   // provided → overrides file value
        },
    };

    let config: AppConfig = ConfigBuilder::new()
        .with_file("examples/config/default.toml", true)
        .with_source(SerdeSource::new(&args)?)  // highest priority
        .build()?;

    println!("=== Config after SerdeSource override ===");
    println!("app.name  = {} (from file — args had None)", config.app.name);
    println!("app.debug = {} (from args — overrode file)", config.app.debug);
    println!("db.host   = {} (from file — args didn't touch database)", config.database.host);

    Ok(())
}
