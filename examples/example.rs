use dragon_fnd::{AppContext, Config};
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
#[allow(dead_code)]
struct DatabaseSection {
    host: String,
    port: u16,
    name: String,
    url: String,
}

fn main() -> Result<(), dragon_fnd::Error> {
    // Deserialize once at build time
    let ctx = AppContext::builder()
        .with_config(
            Config::builder()
                .with_file("examples/default.toml", true)
                .with_file("examples/dev.toml", false)
                .build::<AppConfig>()?,
        )
        .build()?;

    // Zero-cost reference access
    let config = ctx.config();

    println!("App: {} (debug={})", config.app.name, config.app.debug);
    println!("Database URL: {}", config.database.url);

    Ok(())
}
