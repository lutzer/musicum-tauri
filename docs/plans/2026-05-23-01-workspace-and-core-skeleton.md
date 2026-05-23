# Workspace Bootstrap + musicum-core Skeleton Implementation Plan

**Goal:** Wire the Cargo workspace root and create a compiling musicum-core crate with all module stubs in place — no logic yet, just the skeleton everything else builds on.

**Architecture:** A single `Cargo.toml` at the repo root unifies all crates under one workspace. The existing plugin/processor libs are pulled in as workspace members. `musicum-core` is a new library crate with four top-level modules: `db`, `services`, `audio`, and `error`.

**Tech Stack:** Rust (Cargo workspace, edition 2021), thiserror for error types.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `Cargo.toml` | Workspace root — all members, shared deps |
| Create | `libs/musicum-core/Cargo.toml` | Core library manifest |
| Create | `libs/musicum-core/src/lib.rs` | Public re-exports, module declarations |
| Create | `libs/musicum-core/src/error.rs` | `ServiceError` enum (thiserror) |
| Create | `libs/musicum-core/src/db/mod.rs` | Empty stub |
| Create | `libs/musicum-core/src/db/schema.rs` | `SCHEMA_VERSION` constant |
| Create | `libs/musicum-core/src/db/entities/mod.rs` | Empty stub |
| Create | `libs/musicum-core/src/services/mod.rs` | Empty stub |
| Create | `libs/musicum-core/src/audio/mod.rs` | Empty stub |
| Delete | `libs/audio-plugin-sdk/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/gain/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/reverb/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/pan/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/normalize/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/level-meter/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/audio-plugins/oscilloscope/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/structural-processor-sdk/Cargo.lock` | No longer a standalone workspace |
| Delete | `libs/structural-processors/Cargo.lock` | No longer a standalone workspace |

---

### Task 1: Remove stale per-crate Cargo.lock files

Each plugin crate was previously its own workspace root and therefore has a `Cargo.lock`. Once pulled into the repo workspace those files are ignored by Cargo but cause confusion.

**Step 1.1** — Remove them:
```bash
rm libs/audio-plugin-sdk/Cargo.lock
rm libs/audio-plugins/gain/Cargo.lock
rm libs/audio-plugins/reverb/Cargo.lock
rm libs/audio-plugins/pan/Cargo.lock
rm libs/audio-plugins/normalize/Cargo.lock
rm libs/audio-plugins/level-meter/Cargo.lock
rm libs/audio-plugins/oscilloscope/Cargo.lock
rm libs/structural-processor-sdk/Cargo.lock
rm libs/structural-processors/Cargo.lock
```

**Step 1.2** — Verify they are gone:
```bash
find libs -name "Cargo.lock"
# Expected output: (empty)
```

---

### Task 2: Create the workspace Cargo.toml

**Step 2.1** — Create `Cargo.toml` at repo root:
```toml
[workspace]
resolver = "2"
members = [
    "libs/musicum-core",
    "libs/audio-plugin-sdk",
    "libs/audio-plugins/gain",
    "libs/audio-plugins/reverb",
    "libs/audio-plugins/pan",
    "libs/audio-plugins/normalize",
    "libs/audio-plugins/oscilloscope",
    "libs/audio-plugins/level-meter",
    "libs/structural-processor-sdk",
    "libs/structural-processors",
]

[workspace.dependencies]
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
uuid        = { version = "1", features = ["v4", "serde"] }
tokio       = { version = "1", features = ["full"] }
sea-orm     = { version = "1", features = ["sqlx-sqlite", "runtime-tokio-rustls", "macros"] }
thiserror   = "1"
anyhow      = "1"
tracing     = "1"
chrono      = { version = "0.4", features = ["serde"] }
```

**Step 2.2** — Verify the workspace is valid:
```bash
cargo metadata --no-deps --format-version 1 | python3 -m json.tool | grep '"name"' | sort
# Expected: lists all member crate names including gain, reverb, etc.
```

---

### Task 3: Create musicum-core/Cargo.toml

**Step 3.1** — Create `libs/musicum-core/Cargo.toml`:
```toml
[package]
name = "musicum-core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde.workspace      = true
serde_json.workspace = true
uuid.workspace       = true
tokio.workspace      = true
sea-orm.workspace    = true
thiserror.workspace  = true
anyhow.workspace     = true
tracing.workspace    = true
chrono.workspace     = true

symphonia   = { version = "0.5", features = ["all"] }
cpal        = "0.15"
rtrb        = "0.3"
slug        = "0.1"
walkdir     = "2"

audio-plugin-sdk         = { path = "../audio-plugin-sdk" }
plugin-gain              = { path = "../audio-plugins/gain",        package = "gain" }
plugin-reverb            = { path = "../audio-plugins/reverb",      package = "reverb" }
plugin-pan               = { path = "../audio-plugins/pan",         package = "pan" }
plugin-normalize         = { path = "../audio-plugins/normalize",   package = "normalize" }
plugin-level-meter       = { path = "../audio-plugins/level-meter", package = "level-meter" }
plugin-oscilloscope      = { path = "../audio-plugins/oscilloscope",package = "oscilloscope" }
structural-processor-sdk = { path = "../structural-processor-sdk" }
structural-processors    = { path = "../structural-processors" }

[dev-dependencies]
hound       = "3"
tempfile    = "3"
tokio       = { version = "1", features = ["full"] }
```

Note: `package = "gain"` renames the dependency to `plugin-gain` in Rust code because `gain` is a reserved-looking name and conflicts would occur if you depend on multiple plugins. The local crate name in Rust source becomes `plugin_gain`, `plugin_reverb`, etc.

**Step 3.2** — Verify it appears in metadata:
```bash
cargo metadata --no-deps --format-version 1 | grep musicum-core
# Expected: musicum-core appears in the output
```

---

### Task 4: Create the error type

**Step 4.1** — Create `libs/musicum-core/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}
```

---

### Task 5: Create module stubs

**Step 5.1** — Create `libs/musicum-core/src/db/schema.rs`:
```rust
pub const SCHEMA_VERSION: u32 = 1;
```

**Step 5.2** — Create `libs/musicum-core/src/db/entities/mod.rs`:
```rust
// Entity modules will be declared here as they are created.
```

**Step 5.3** — Create `libs/musicum-core/src/db/mod.rs`:
```rust
pub mod entities;
pub mod schema;
```

**Step 5.4** — Create `libs/musicum-core/src/services/mod.rs`:
```rust
// Service modules will be declared here as they are created.
```

**Step 5.5** — Create `libs/musicum-core/src/audio/mod.rs`:
```rust
// Audio engine modules will be declared here as they are created.
```

**Step 5.6** — Create `libs/musicum-core/src/lib.rs`:
```rust
pub mod audio;
pub mod db;
pub mod error;
pub mod services;

pub use error::ServiceError;
```

---

### Task 6: Verify the workspace compiles

**Step 6.1** — Run check on the whole workspace:
```bash
cargo check -p musicum-core
# Expected: Compiling musicum-core ... Finished
```

If you see "error[E0432]: unresolved import" for any plugin, the package renaming alias may need adjustment. Check the exact package name in each plugin's `Cargo.toml` `[package] name` field and adjust the `package = "..."` alias in step 3.1 accordingly.

**Step 6.2** — Confirm all workspace members check cleanly:
```bash
cargo check --workspace
# Expected: All crates check without errors
```

---

## What's next

Plan 02 builds the full DB layer (SeaORM entities + `connect()` + schema version) on top of this skeleton.
