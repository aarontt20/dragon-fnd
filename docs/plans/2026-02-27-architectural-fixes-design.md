# Architectural Fixes Design

Four changes to address structural weaknesses found during a full codebase audit.
Breaking changes to the public API are acceptable (pre-1.0).

---

## 1. ConfigValue — Library-Owned Value Type

### Problem

`ConfigEntry.value` exposes `toml::Value` in the public API. Every `ConfigSource`
implementor must import `toml` and hand-construct `toml::Value` variants. The
primary extension point is coupled to a specific format crate.

### Design

New `ConfigValue` enum replaces `toml::Value` in the public API:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime(String),
    Array(Vec<ConfigValue>),
    Table(ConfigTable),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigTable(BTreeMap<String, ConfigValue>);
```

**Constructors:** `ConfigValue::string()`, `ConfigValue::integer()`, etc. for
scalar values. `ConfigTable::new()` with chainable `.insert()` for tables.

**Datetime handling:** Stored as `String` to avoid leaking `toml::Datetime`.
Parsed to `toml::Value::Datetime` during internal conversion.

**Internal plumbing:** `merge_at_path`, `deep_merge`, `resolve_references`, and
`build()` continue to operate on `toml::Table`/`toml::Value` internally.
Conversion happens at the boundary:

- `ConfigSource::entries()` returns `ConfigValue` (public API)
- `ConfigBuilder::build()` converts to `toml::Value` before merging (internal)
- `FileSource`/`EnvSource` convert from `toml::Value` to `ConfigValue` before returning

**ConfigEntry becomes:**

```rust
pub struct ConfigEntry {
    pub path: Vec<String>,
    pub value: ConfigValue,
}
```

**Downstream impact:** `ConfigSource` implementors change from
`toml::Value::Integer(8080)` to `ConfigValue::integer(8080)`. No `toml` import
needed.

---

## 2. RotationStrategy Enum

### Problem

`FileConfig` has `rotation: Rotation`, `max_bytes: Option<u64>`, and
`compress: bool` as orthogonal fields. Seven lines of runtime validation reject
invalid combinations (e.g., `max_bytes` + `Daily`). The type system should
prevent these states.

### Design

New `RotationStrategy` enum replaces the old `Rotation` enum:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum RotationStrategy {
    Daily,
    Hourly,
    SizeBased { max_bytes: u64 },
    Never,
}
```

`compress` stays on `FileConfig` — it applies to any strategy that produces
rotated files (time-based or size-based).

`FileConfig` loses `rotation: Rotation` and `max_bytes: Option<u64>`. Gains
`rotation: RotationStrategy`.

**Validation rules eliminated (structurally impossible):**

| Rule | Why |
|------|-----|
| `max_bytes` + time-based rotation | `Daily`/`Hourly` have no `max_bytes` field |

**Validation rules remaining:**

| Rule | Check |
|------|-------|
| `max_bytes >= 4096` | On `SizeBased` variant |
| `compress` requires rotation | `compress && rotation == Never` rejected |
| Retention mutual exclusion | `retain_days` vs `retain_files` |
| Retention requires rotation | `Never` + retention rejected |

Down from 7 runtime checks to 4.

**TOML shape:**

```toml
# Time-based:
[file]
rotation = "daily"
compress = true

# Size-based:
[file]
rotation = "size"
max_bytes = 10485760
compress = true

# No rotation:
[file]
rotation = "never"
```

Custom `Deserialize` on `FileConfig`: if `rotation` is `"size"`, read `max_bytes`
from the same table. Otherwise ignore it. `max_bytes` and `compress` stay as flat
keys on `[file]`.

**Builder:** `FileBuilder` loses `.max_bytes()`. Size-based rotation is set via:

```rust
FileBuilder::new("./logs")
    .rotation(RotationStrategy::SizeBased { max_bytes: 10_485_760 })
    .compress(true)
    .retain_files(5)
```

**Old `Rotation` enum:** Removed entirely. `RotationStrategy` replaces it.

---

## 3. Private Fields on Logging Config Types

### Problem

`LoggingConfig`, `ConsoleConfig`, and `FileConfig` are `pub` with public fields.
Anyone can construct them directly, bypassing the builder's mutual-exclusion
enforcement (e.g., `retain_days` clearing `retain_files`).

### Design

Config types stay `pub` (needed as `Deserialize` targets for TOML embedding).
Fields become private.

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    enabled: bool,        // was: pub enabled
    filter: String,       // was: pub filter
    modules: BTreeMap<String, String>,
    console: ConsoleConfig,
    file: FileConfig,
}
```

Same change for `ConsoleConfig` and `FileConfig`. `Deserialize` still works —
serde reads private fields.

**Access patterns:**

Deserialization bridge (preserved):
```rust
let config: AppConfig = ConfigBuilder::new().with_file("app.toml", true).build()?;
let logging = LoggingBuilder::from_config(config.logging)
    .filter("debug")
    .build();
```

Direct mutation (blocked):
```rust
config.logging.file.enabled = true;  // compile error
```

**Getters:** Added sparingly where there's a clear use case. `LoggingConfig::enabled()`
is the primary one — lets users conditionally skip logging setup.

**Test impact:** Integration tests that directly mutate config fields get
rewritten to use the builder API. Unit tests inside the logging crate use
`pub(crate)` access.

---

## 4. Extension Slot on AppContextBuilder

### Problem

`AppContextBuilder` is a fixed registry of library-provided subsystems. Users
cannot store their own state in `AppContext`. Each new subsystem requires a new
field, a new `with_X()` method, and new init code.

### Design

Library subsystems keep their dedicated, type-safe fields. A new extension
mechanism lets users store arbitrary state:

**Builder:**
```rust
pub struct AppContextBuilder<Cfg, Async = SyncBuild> {
    cfg: Cfg,
    _async: PhantomData<Async>,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "logging")]
    logging: Option<LoggingBuilder>,
}
```

`with_extension` available on all builder states:
```rust
impl<Cfg, A> AppContextBuilder<Cfg, A> {
    pub fn with_extension<T: Send + Sync + 'static>(mut self, ext: T) -> Self {
        self.extensions.insert(TypeId::of::<T>(), Box::new(ext));
        self
    }
}
```

**AppContext:**
```rust
pub struct AppContext<C> {
    config: C,
    extensions: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    #[cfg(feature = "logging")]
    log_guard: Option<WorkerGuard>,
}
```

`extensions` is declared before `log_guard` so the guard outlives user
extensions during drop (extensions that log on drop still have a live logger).

**Retrieval:**
```rust
ctx.extension::<MyPool>()  // -> Option<&MyPool>
```

Returns `Option` — no panics (Constraint 1). Last-writer-wins if the same
type is registered twice.

**What this does NOT do:**

- No `Subsystem` trait or dependency graph for extensions
- No compile-time proof that an extension was registered
- Library subsystems do not use the extension slot

**Debug:** Manual `Debug` impl shows extension count since `dyn Any` is not
debuggable.

---

## Deferred

**Issue 5 — `SyncBuild`/`AsyncBuild` type parameter.** Deferred until the first
async subsystem lands and the actual transition mechanism is known.

---

## Interaction Between Changes

Changes 2 and 3 are tightly coupled — `RotationStrategy` changes `FileConfig`'s
fields, and private fields changes their visibility. These should be implemented
together.

Change 1 (`ConfigValue`) is independent of changes 2-4 and can be implemented
first or in parallel.

Change 4 (extensions) is independent and can be implemented last.

Suggested order: 1, then 2+3 together, then 4.
