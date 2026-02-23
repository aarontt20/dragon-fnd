# VISION.md — Where dragon-fnd Is Going

This document captures what dragon-fnd will become — the subsystems on the horizon, the design principles driving them, and the architectural shape they will take. For what exists today, see [DESIGN.md](DESIGN.md).

---

## What's Already Built

The config subsystem is complete: `ConfigSource` trait for extensible input channels, `FileSource` (TOML files), `EnvSource` (environment variables with prefix/separator), a `ConfigBuilder` with fluent API and generic `build::<T>()`, and graph-based `${path.to.field}` variable resolution with topological sort, cycle detection, and full value substitution. `AppContext<C>` uses a type-state builder with compile-time enforcement — `build_sync()` returns `Result` and is only available when config is provided.

The logging subsystem is complete: structured logging via `tracing` with console and file outputs, per-layer `EnvFilter` configuration, time-based file rotation (daily/hourly/never) via `tracing-appender`, size-based file rotation with background gzip compression via a custom `SizeRotatingWriter`, retention cleanup (days-based or file-count), fluent builder API (`LoggingBuilder`, `ConsoleBuilder`, `FileBuilder`), and full `AppContext` integration. Logging is the first feature-gated subsystem (`logging` feature). See DESIGN.md for full details. What follows is everything that's still ahead.

---

## The Shape of the Crate

dragon-fnd is a single crate with feature-gated subsystems. Downstream projects declare one dependency and opt into what they need:

```toml
[dependencies]
dragon-fnd = { path = "../dragon-fnd", features = ["logging", "sqlite"] }
```

Each subsystem lives behind its own feature flag. You only pay for what you use — in compile time, dependency weight, and API surface.

| Subsystem | Feature | Depends On | Status |
|-----------|---------|------------|--------|
| Config | *(always on)* | — | Built |
| CLI Args | `cli` | Config | Planned |
| Logging | `logging` | Config | Built |
| SQLite | `sqlite` | Config | Planned |
| FS Storage | `fs` | Config | Planned |
| Graceful Shutdown | `shutdown` | — | Planned |
| HTTP | `http` | Config, Shutdown | Planned |

---

## What It Is Not

- **Not a framework.** dragon-fnd does not own `main()`. It does not prescribe application structure, routing, or lifecycle. The user writes their application; dragon-fnd wires up the infrastructure beneath it.
- **Not opinionated about project type.** A CLI tool, a web scraper, a long-running archiver, and an axum microservice are all equally natural consumers. The library does not hardcode assumptions about what kind of application is being built.
- **Not prescriptive about libraries.** Each subsystem ships a default implementation (behind a feature flag), but the architecture is trait-based. If you want a different logger, a different database, or a different argument parser, the library accommodates that.

---

## The Design Boundary

Every subsystem follows the same three-layer pattern:

| Layer | Who Owns It | Example (Database) |
|-------|-------------|-------------------|
| **Trait / interface** | Library (always available) | "Something that can be initialized from config and produces a handle" |
| **Default implementation** | Library (feature-gated) | `sqlx` pool init with migrations |
| **Custom implementation** | User (in their application) | Their own pool, a wrapper, or nothing at all |

The library says "if you can satisfy this contract, I can manage the lifecycle." It never says "you must use this specific crate." The feature-gated defaults are conveniences — they save you from rebuilding boilerplate for the tenth time, but they are not the only path.

This pattern is established in the config system: `ConfigSource` is the trait, `FileSource` and `EnvSource` are the defaults, and `with_source()` accepts anything that implements the contract.

**Exception: Logging.** The logging subsystem does not define its own trait. `tracing` is the Rust ecosystem's de facto logging abstraction — the crate itself provides the interface contract (instrument with `tracing::info!()`, swap subscribers via layers). The library configures the subscriber; users extend via `tracing-subscriber`'s layer system. This is an explicit exception to the three-layer pattern, justified because adding a library-level trait on top of `tracing` would add indirection with no real value.

---

## Graph-Based Dependency Resolution as a Core Pattern

The library's central architectural insight: **"I have a bag of things with interdependencies — give them to me in the right order"** is the same problem at multiple scales.

**Scale 1 — Config values.** String values reference other string values via `${path.to.field}`. The resolution system collects all references, builds a dependency graph, topological-sorts it, and resolves in order. This is built today (`src/config/resolve.rs`).

**Scale 2 — Subsystem initialization.** Subsystems depend on other subsystems (logging needs config, database needs config, HTTP needs config + shutdown). The builder will collect registered subsystems, build a dependency graph from declared dependencies, topological-sort it, and initialize in order.

**Scale 3 — Shutdown teardown.** Cleanup must happen in reverse dependency order — the HTTP server shuts down before the database pool closes, the database pool closes before the logger flushes. This is the same graph, walked backwards.

All three scales use the same algorithm. If the topological sort implementation in `resolve.rs` proves reusable (or worth extracting into a shared utility), the library has one dependency resolution engine powering config, initialization, and teardown. This is not accidental — it reflects the fundamental nature of the problem dragon-fnd solves: managing a graph of interdependent subsystems.

---

## Lessons from the Previous Attempt

The old version (dragon-fnd-old) built all planned subsystems: config with file discovery, environment overlay, and CLI args; logging with tracing and file rotation; database with sqlx; HTTP with axum and graceful shutdown. It worked, but it had structural problems that this rewrite addresses:

**Panicking APIs.** `ctx.database()` and `ctx.http_config()` panicked if the subsystem was not enabled. Library code should never panic. The rewrite will ensure every accessor returns `Result` or `Option`.

**Silent failures.** Missing environment-specific config files produced no warning. CLI path navigation failed silently. Unclosed `${` braces were treated as literal text. The rewrite uses graph-based resolution with explicit errors for every failure mode.

**Rigid initialization order.** The old builder used type-state to enforce async requirements (good), but hardcoded the initialization sequence inside `build()` (bad). Adding a subsystem meant manually wiring it into the correct position. The rewrite will separate these concerns: type-state for async enforcement, dependency graph for initialization order.

**Library owning CLI args.** The old `.with_args::<A>()` called `A::parse()` internally and stored the result in a type-erased `Any`. This forced the library to depend on clap, own the parsing, and prescribe the args pattern. The rewrite keeps args as a user concern: parse however you want, wrap the result in a `ConfigSource`, and hand it to the builder.

These are not just bugs to fix — they are design constraints for the rewrite. Every subsystem will be evaluated against them.

---

## AppContext and the Type-State Builder

`AppContext` is the spine. It holds all initialized subsystems and provides typed access to each one. The builder collects registrations and resolves initialization order.

Two compile-time dimensions:

**1. Async requirement.** The builder tracks whether any registered subsystem needs an async runtime. Enabling `sqlite` or `http` transitions the builder to a `RequiresAsync` state. `build_sync()` is only available when no async subsystem is registered. `build()` (async) is always available. This is enforced by the type system — calling `build_sync()` on a builder with database enabled does not compile.

**2. Subsystem availability.** The built `AppContext` should expose accessors only for subsystems that were registered. If you didn't register a database, `ctx.database()` should not exist on the type — not panic, not return `Option`, but literally not be a method. Whether this is achievable via trait bounds, associated types, or a pragmatic `Option`-based fallback is an open design question to be resolved when the second subsystem lands.

### Dependency Resolution at Build Time

When `build()` is called, the builder holds a set of registered subsystem intents. Instead of hardcoding initialization order:

1. Each subsystem declares its dependencies (logging needs config, database needs config, HTTP needs config + shutdown)
2. `build()` topological-sorts the dependency graph
3. Subsystems initialize in resolved order
4. Missing required dependencies produce a clear error (e.g., HTTP registered without shutdown)

This mirrors the graph-based approach already used in variable resolution — the same pattern applied at the subsystem level.

### Usage Shape

```rust
let ctx = AppContext::builder()
    .with_logging(LoggingBuilder::from_config(config.logging))  // feature: "logging"
    .with_config(config)        // always available
    .with_database("app.db")    // feature: "sqlite" (planned)
    .with_shutdown()             // feature: "shutdown" (planned)
    .build()                     // async, because sqlite requires it
    .await?;

ctx.config();     // &MyConfig — always available
ctx.database();   // &Pool — only available because with_database() was called
```

Note: `with_logging()` is available on all builder states (before or after `with_config()`). Logging initialization happens inside `build_sync()` / `build()`, not at registration time.

---

## CLI Args Subsystem

**Feature:** `cli`

The library does not own argument parsing. It does not depend on clap. It does not call `parse()`. It does not define what arguments look like.

What it does: provide a `ConfigSource` adapter that makes it trivial to feed parsed CLI arguments into the config layer. The user parses args however they want (clap, pico-args, manual parsing, whatever), then wraps the result:

```rust
let args = Args::parse();  // user's code, user's parser

let config: MyConfig = Config::builder()
    .with_file("config/default.toml", true)
    .with_env("MYAPP", "__")
    .with_source(ClapSource::new(args))  // args become just another config source
    .build()?;
```

`ClapSource` (or whatever the adapter is called) serializes the args struct to TOML values and produces `ConfigEntry` items — the same interface as `FileSource` and `EnvSource`. `Option::None` fields do not produce entries, preserving lower-priority values from files and env vars.

The key insight: CLI args are not special. They are just another config source with highest priority. The library provides the adapter; the user provides the parser and the types.

---

## Logging Subsystem — Built

**Feature:** `logging` — See [DESIGN.md](DESIGN.md) for full details.

Future extensions: additional output sinks (network, syslog).

---

## SQLite Subsystem

**Feature:** `sqlite`

Database pool initialization and lifecycle via `sqlx`. Reads the database path and pool configuration from the config layer. Runs pending migrations from a `./migrations/` directory. Tests connectivity at initialization (not deferred to the first query — the old version's lack of this was a known problem).

Enabling this feature transitions the builder to `RequiresAsync`, since `sqlx` pool creation and migration execution are async operations. `build_sync()` becomes unavailable at compile time.

---

## FS Storage Subsystem

**Feature:** `fs`

Managed directory structures for applications that need organized file storage. Reads a base directory and optional subdirectory layout from config. Provides path resolution and directory creation.

Use cases:
- An archiver's download tree (`{base}/downloads/{site}/{artist}/`)
- Cache directories (`{base}/cache/`)
- Log directories (`{base}/logs/`)
- Temporary staging areas

The subsystem manages paths and directories — it does not manage file contents. It ensures directories exist, provides resolved paths, and handles cleanup if configured. The application decides what to write and where.

---

## Graceful Shutdown Subsystem

**Feature:** `shutdown`

Signal handling and coordinated cleanup. Registers handlers for SIGTERM and SIGINT (with platform-appropriate behavior on Windows). Produces a `ShutdownSignal` that other subsystems can clone and await.

Cleanup hooks run in reverse registration order when shutdown is triggered. Subsystems that hold resources (database pools, file handles, network listeners) register cleanup logic during initialization. The shutdown subsystem orchestrates teardown.

This subsystem has no config dependency — it is pure runtime infrastructure. It does not need the config layer to know how to handle signals.

---

## HTTP Subsystem

**Feature:** `http`

Axum server lifecycle management. Reads bind address, port, and graceful shutdown timeout from config. Integrates with the shutdown subsystem for coordinated teardown.

The library provides:
- TCP listener binding from config
- Graceful shutdown wired to the shutdown subsystem's signal
- A `serve()` function that takes an axum `Router`

The library does not provide:
- Routing (the user builds their `Router`)
- Middleware (the user composes their middleware stack)
- Request/response types (the user defines their handlers)

The library manages the server's lifecycle — startup, readiness, and graceful shutdown. The user owns everything that happens between request and response.

---

## The Environment Question — Resolved

The old version had a first-class "environment" concept — the `{PREFIX}_ENV` variable determined whether the application ran in development, testing, or production, and subsystems applied environment-aware defaults (pretty logging in dev, JSON logging in prod).

**Decision: Environment is just config.** The user creates `config/dev.toml` and `config/prod.toml` and layers them explicitly. The library has no opinion about environments. This was confirmed when the logging subsystem landed — logging config is fully explicit (format, filter, outputs) with no hidden environment-aware defaults. What you see in config is what you get.

---

## The ConfigSource Extension Pattern

`ConfigSource` is the architectural backbone. Every input channel — files, environment variables, CLI args, and anything not yet imagined (remote config servers, secret managers, CI/CD variables) — integrates through the same one-method trait.

Future considerations that do not require changing the trait:

- **Async config sources** — Remote endpoints and secret managers need async loading. An `AsyncConfigSource` variant or a runtime adapter that blocks could handle this without changing existing synchronous sources.
- **Explicit priority** — Currently, priority is implicit in registration order. If sources need to declare their own priority (e.g., "CLI args always win regardless of registration order"), the builder could sort sources before merging.
- **Validation hooks** — Post-merge, pre-resolve validation (e.g., "warn if an unknown section appears") could be added to the builder without touching the source interface.

The trait is deliberately simple: one method, one return type, no lifecycle hooks. This simplicity is the feature — it keeps the barrier to implementing a new source as low as possible.
