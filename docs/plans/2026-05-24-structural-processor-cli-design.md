# Structural Processor CLI Integration — Implementation Plan

**Goal:** Extend the `musicum` CLI with `processors list` and full CRUD for preset processor chains.

**Architecture:** The `structural-processors` lib exposes a public `registry()` function as the single source of truth for all registered processors. `musicum-core`'s `sidecar::ProcessorEntry` is redesigned as a tagged enum distinguishing structural from audio-plugin entries. The CLI resolves processor defaults from the registry and persists mutations sidecar-first then DB.

**Tech Stack:** Rust, Clap 4 (derive), SeaORM, `structural-processor-sdk`, `structural-processors`, `uuid`, `slug`

---

## File Map

| File | Change |
|---|---|
| `libs/structural-processor-sdk/src/lib.rs` | Rename `ProcessorEntry` → `StructuralProcessorEntry` (struct + macro) |
| `libs/structural-processor-sdk/src/chain.rs` | Update import and all function signatures |
| `libs/structural-processors/src/lib.rs` | Add `pub fn registry()`; update test to use it |
| `libs/structural-processors/src/main.rs` | Remove local `fn registry()`; call `structural_processors::registry()` |
| `libs/musicum-core/src/sidecar.rs` | Replace `ProcessorEntry` flat struct with `ProcessorRef` + `ProcessorEntry` enum; add `read_preset_sidecar` |
| `libs/musicum-core/src/services/preset_service.rs` | Add `create_preset`, `delete_preset`, `update_preset_processors` |
| `libs/musicum-core/tests/preset_service.rs` | New integration tests for the three new functions |
| `apps/cli/Cargo.toml` | Add `structural-processors`, `structural-processor-sdk`, `uuid` deps |
| `apps/cli/src/main.rs` | Add `Commands::Processors`; pass `library_dir` to `presets::run` |
| `apps/cli/src/commands/mod.rs` | Export `processors` module |
| `apps/cli/src/commands/processors.rs` | New: `processors list` |
| `apps/cli/src/commands/presets.rs` | Add `create`, `remove`, `add-processor`, `remove-processor`; enhance `show` |
| `apps/cli/src/output.rs` | Add `print_table_3col` helper |

---

## Task 1: Rename `ProcessorEntry` → `StructuralProcessorEntry` in the SDK

**Why:** Once `sidecar.rs` also has a `ProcessorEntry`, both names collide in any file that imports both crates. Rename at source to avoid ambiguity.

### Step 1.1 — Update `libs/structural-processor-sdk/src/lib.rs`

Replace the struct name and every occurrence inside the macro:

```rust
// libs/structural-processor-sdk/src/lib.rs
pub mod chain;
pub mod processor;

pub use chain::Edit;
pub use processor::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

/// Vtable entry for one processor. Holds plain function pointers — no heap, no trait objects.
pub struct StructuralProcessorEntry {
    pub descriptor:       fn() -> &'static ProcessorDescriptor,
    pub validate:         fn(&Params) -> bool,
    pub apply:            fn(&[f32], u32, u16, &Params) -> Vec<f32>,
    pub output_duration:  fn(f64, &Params) -> f64,
    pub map_time_forward: fn(f64, f64, &Params) -> f64,
    pub map_time_back:    fn(f64, f64, &Params) -> f64,
}

impl StructuralProcessorEntry {
    pub fn of<P: StructuralProcessor>() -> Self {
        Self {
            descriptor:       P::descriptor,
            validate:         P::validate,
            apply:            P::apply,
            output_duration:  P::output_duration,
            map_time_forward: P::map_time_forward,
            map_time_back:    P::map_time_back,
        }
    }
}

#[macro_export]
macro_rules! implement_sp_chain {
    ($($proc:ty),+ $(,)?) => {
        static __SP_REGISTRY_CELL: std::sync::OnceLock<Vec<$crate::StructuralProcessorEntry>> =
            std::sync::OnceLock::new();

        fn __sp_registry() -> &'static [$crate::StructuralProcessorEntry] {
            __SP_REGISTRY_CELL.get_or_init(|| {
                vec![$($crate::StructuralProcessorEntry::of::<$proc>()),+]
            })
        }

        // ... rest of macro unchanged (static muts, __sp_exports mod)
```

Keep everything inside `__sp_exports` identical — only the two type references above change.

### Step 1.2 — Update `libs/structural-processor-sdk/src/chain.rs`

Change the import line and every function signature:

```rust
use crate::{Params, ProcessorDescriptor, StructuralProcessorEntry};

pub fn apply_chain(
    registry: &[StructuralProcessorEntry],
    ...
) -> Vec<f32> { ... }

pub fn descriptors_json(registry: &[StructuralProcessorEntry]) -> String { ... }

pub fn validate_edit(registry: &[StructuralProcessorEntry], ...) -> bool { ... }

pub fn map_time_forward(
    registry: &[StructuralProcessorEntry],
    ...
) -> f64 { ... }

pub fn map_time_back(
    registry: &[StructuralProcessorEntry],
    ...
) -> f64 { ... }

fn find<'a>(registry: &'a [StructuralProcessorEntry], edit_type: &str)
    -> Option<&'a StructuralProcessorEntry> { ... }
```

Also update the test helper at the bottom of chain.rs:

```rust
fn reg() -> Vec<StructuralProcessorEntry> {
    vec![
        StructuralProcessorEntry::of::<PassProcessor>(),
        StructuralProcessorEntry::of::<HalfProcessor>(),
    ]
}
```

### Step 1.3 — Run SDK tests

```
cargo test -p structural-processor-sdk
```

Expected: all tests pass.

---

## Task 2: Update `structural-processors` after the rename

### Step 2.1 — Update `libs/structural-processors/src/lib.rs` test helper

Inside `#[cfg(test)] mod tests`, change:

```rust
fn registry() -> Vec<structural_processor_sdk::StructuralProcessorEntry> {
    use crate::processors::{
        crop::CropProcessor, cut::CutProcessor, slice::SliceProcessor, trim::TrimProcessor,
    };
    use structural_processor_sdk::StructuralProcessorEntry;
    vec![
        StructuralProcessorEntry::of::<TrimProcessor>(),
        StructuralProcessorEntry::of::<CutProcessor>(),
        StructuralProcessorEntry::of::<SliceProcessor>(),
        StructuralProcessorEntry::of::<CropProcessor>(),
    ]
}
```

### Step 2.2 — Update `libs/structural-processors/src/main.rs`

Change the import:

```rust
use structural_processor_sdk::{
    chain::{apply_chain, descriptors_json, Edit},
    StructuralProcessorEntry,
};
```

And the local `fn registry()` return type:

```rust
fn registry() -> Vec<StructuralProcessorEntry> {
    vec![
        StructuralProcessorEntry::of::<TrimProcessor>(),
        // ...
    ]
}
```

### Step 2.3 — Run structural-processors tests

```
cargo test -p structural-processors
```

Expected: all existing tests pass.

---

## Task 3: Add `pub fn registry()` to structural-processors and dedup

**Why:** The CLI and `main.rs` both need the processor registry. Defining it once in lib.rs as a public function removes duplication and gives the CLI a stable import path.

### Step 3.1 — Write failing test

In `libs/structural-processors/src/lib.rs`, add inside the tests module:

```rust
#[test]
fn public_registry_has_four_entries() {
    let r = super::registry();
    assert_eq!(r.len(), 4);
    let ids: Vec<&str> = r.iter().map(|e| (e.descriptor)().id).collect();
    assert!(ids.contains(&"trim"));
    assert!(ids.contains(&"cut"));
    assert!(ids.contains(&"slice"));
    assert!(ids.contains(&"crop"));
}
```

Run: `cargo test -p structural-processors public_registry` — expect compile error (no `registry` in lib scope yet).

### Step 3.2 — Implement `pub fn registry()`

Add above the `implement_sp_chain!` macro invocation in `lib.rs`:

```rust
pub mod processors;

pub fn registry() -> Vec<structural_processor_sdk::StructuralProcessorEntry> {
    use processors::{
        crop::CropProcessor, cut::CutProcessor,
        slice::SliceProcessor, trim::TrimProcessor,
    };
    vec![
        structural_processor_sdk::StructuralProcessorEntry::of::<TrimProcessor>(),
        structural_processor_sdk::StructuralProcessorEntry::of::<CutProcessor>(),
        structural_processor_sdk::StructuralProcessorEntry::of::<SliceProcessor>(),
        structural_processor_sdk::StructuralProcessorEntry::of::<CropProcessor>(),
    ]
}

structural_processor_sdk::implement_sp_chain!(
    processors::trim::TrimProcessor,
    processors::cut::CutProcessor,
    processors::slice::SliceProcessor,
    processors::crop::CropProcessor,
);
```

Remove the `fn registry()` from the tests module and replace it with `super::registry()` calls. The existing `chain_trim_then_cut` and other tests use the local `registry()` — change them to call `super::registry()` instead.

### Step 3.3 — Run test

```
cargo test -p structural-processors public_registry
```

Expected: passes.

### Step 3.4 — Update `main.rs` to use the lib's `registry()`

In `libs/structural-processors/src/main.rs`:

1. Remove `fn registry() -> Vec<StructuralProcessorEntry> { ... }` entirely.
2. Add `use structural_processors::registry;` at the top imports block.
3. Callers of `registry()` remain unchanged — they still call `registry()` which now resolves to the lib's function.

```rust
// libs/structural-processors/src/main.rs
mod processors;

use std::io::{self, Read};

use structural_processors::registry;
use structural_processor_sdk::chain::{apply_chain, descriptors_json, Edit};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

// read_wav, write_wav, main unchanged
```

Note: The `mod processors;` declaration in `main.rs` is separate from `lib.rs`'s `pub mod processors;`. Both compile fine as independent crate roots; Rust does not confuse them.

### Step 3.5 — Verify binary still compiles

```
cargo build -p structural-processors
```

---

## Task 4: Redesign `sidecar::ProcessorEntry` as a tagged enum

**Why:** The CLI needs to distinguish structural vs audio-plugin entries in the UI. A flat `kind: String` field is stringly-typed and error-prone; a Rust enum gives exhaustive matching and clear serialisation semantics.

### Step 4.1 — Write a failing sidecar round-trip test

Create `libs/musicum-core/tests/sidecar_processor_entry.rs`:

```rust
use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};

#[test]
fn structural_entry_round_trips() {
    let entry = ProcessorEntry::Structural {
        id: "uuid-1".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"structural\""));
    let back: ProcessorEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

#[test]
fn audio_plugin_entry_round_trips() {
    let entry = ProcessorEntry::AudioPlugin {
        id: "uuid-2".into(),
        enabled: false,
        processor: ProcessorRef {
            id: "gain".into(),
            params: serde_json::json!({ "gain": 0.8 }),
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"audio-plugin\""));
    let back: ProcessorEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}
```

Run: `cargo test -p musicum-core sidecar_processor_entry` — expect compile error.

### Step 4.2 — Replace `ProcessorEntry` in `libs/musicum-core/src/sidecar.rs`

Replace the existing flat struct with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorRef {
    pub id:     String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProcessorEntry {
    Structural {
        id:        String,
        enabled:   bool,
        processor: ProcessorRef,
    },
    #[serde(rename = "audio-plugin")]
    AudioPlugin {
        id:        String,
        enabled:   bool,
        processor: ProcessorRef,
    },
}
```

`ClipSidecar.processors` and `PresetSidecar.processors` remain `Vec<ProcessorEntry>` — the type just changes.

### Step 4.3 — Run the new test

```
cargo test -p musicum-core sidecar_processor_entry
```

Expected: both tests pass.

### Step 4.4 — Run all musicum-core tests to catch breakage

```
cargo test -p musicum-core
```

Fix any compilation errors from changed field access (e.g., code that read `.kind`, `.params` directly on the old flat struct). The main consumer is `presets.rs` in the CLI — fix after the CLI tasks below.

---

## Task 5: Add `read_preset_sidecar` single-file helper

**Why:** The CLI's `add-processor` and `remove-processor` need to load one preset by slug, not all presets. The existing `read_preset_sidecars` reads the whole directory.

### Step 5.1 — Add to `libs/musicum-core/src/sidecar.rs`

```rust
pub fn read_preset_sidecar(library_dir: &Path, slug: &str) -> Result<PresetSidecar, ServiceError> {
    let path = library_dir
        .join(".musicum")
        .join("presets")
        .join(format!("{slug}.musicum-preset.json"));
    if !path.exists() {
        return Err(ServiceError::NotFound(format!("preset '{slug}'")));
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}
```

---

## Task 6: Add new `preset_service` functions

### Step 6.1 — Write failing tests first

Create `libs/musicum-core/tests/preset_service.rs`:

```rust
mod common;

use musicum_core::{db, sidecar::{self, ProcessorEntry, ProcessorRef}, services::preset_service};
use tempfile::tempdir;

async fn setup() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = db::connect(dir.path().to_str().unwrap()).await.unwrap();
    (db, dir)
}

#[tokio::test]
async fn create_preset_writes_sidecar_and_db() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    let model = preset_service::create_preset(&db, lib, "my-preset", "My Preset", "").await.unwrap();

    assert_eq!(model.slug, "my-preset");
    assert_eq!(model.title, "My Preset");

    // Sidecar exists
    let sc = sidecar::read_preset_sidecar(dir.path(), "my-preset").unwrap();
    assert_eq!(sc.slug, "my-preset");
    assert!(sc.processors.is_empty());
}

#[tokio::test]
async fn create_preset_errors_if_sidecar_exists() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "dup", "Dup", "").await.unwrap();
    let err = preset_service::create_preset(&db, lib, "dup", "Dup", "").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn delete_preset_removes_sidecar_and_db_row() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "gone", "Gone", "").await.unwrap();
    preset_service::delete_preset(&db, lib, "gone").await.unwrap();

    // Sidecar gone
    assert!(sidecar::read_preset_sidecar(dir.path(), "gone").is_err());
    // DB row gone
    let err = preset_service::get_preset_by_slug(&db, "gone").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn update_preset_processors_persists_to_db() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "p1", "P1", "").await.unwrap();

    let processors = vec![ProcessorEntry::Structural {
        id: "uuid-abc".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    }];

    preset_service::update_preset_processors(&db, lib, "p1", processors).await.unwrap();

    let model = preset_service::get_preset_by_slug(&db, "p1").await.unwrap();
    assert!(model.processors.contains("trim"));
}
```

Run: `cargo test -p musicum-core --test preset_service` — expect compile errors.

### Step 6.2 — Implement the three functions

Add to `libs/musicum-core/src/services/preset_service.rs`:

```rust
use std::path::Path;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder,
};
use uuid::Uuid;

use crate::db::entities::preset;
use crate::sidecar::{self, PresetSidecar};
use crate::ServiceError;

// existing list_presets and get_preset_by_slug stay unchanged

pub async fn create_preset(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
    title: &str,
    description: &str,
) -> Result<preset::Model, ServiceError> {
    let lib = Path::new(library_dir);
    let sidecar_path = lib
        .join(".musicum")
        .join("presets")
        .join(format!("{slug}.musicum-preset.json"));
    if sidecar_path.exists() {
        return Err(ServiceError::InvalidInput(format!(
            "preset '{slug}' already exists"
        )));
    }

    let sc = PresetSidecar {
        version: 1,
        slug: slug.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        processors: vec![],
    };
    sidecar::write_preset_sidecar(lib, &sc)?;

    let now = chrono::Utc::now().to_rfc3339();
    let model = preset::ActiveModel {
        id:          Set(Uuid::new_v4().to_string()),
        slug:        Set(slug.to_string()),
        title:       Set(title.to_string()),
        description: Set(description.to_string()),
        processors:  Set("[]".to_string()),
        created_at:  Set(now.clone()),
        updated_at:  Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub async fn delete_preset(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    let lib = Path::new(library_dir);
    let sidecar_path = lib
        .join(".musicum")
        .join("presets")
        .join(format!("{slug}.musicum-preset.json"));
    if sidecar_path.exists() {
        std::fs::remove_file(&sidecar_path)?;
    }
    preset::Entity::delete_by_id(model.id).exec(db).await?;
    Ok(())
}

pub async fn update_preset_processors(
    db: &DatabaseConnection,
    _library_dir: &str,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    let processors_json = serde_json::to_string(&processors)?;
    let now = chrono::Utc::now().to_rfc3339();
    preset::ActiveModel {
        id:          Set(model.id),
        slug:        Set(model.slug),
        title:       Set(model.title),
        description: Set(model.description),
        processors:  Set(processors_json),
        created_at:  Set(model.created_at),
        updated_at:  Set(now),
    }
    .update(db)
    .await?;
    Ok(())
}
```

Note: `delete_by_id` requires importing `sea_orm::EntityTrait` and the entity must have a primary key. The preset entity uses `id: String` as primary key; use `.delete_by_id(model.id)` (or find then delete via `ActiveModel`).

### Step 6.3 — Run the tests

```
cargo test -p musicum-core --test preset_service
```

Expected: all four pass.

### Step 6.4 — Run full test suite

```
cargo test -p musicum-core
```

---

## Task 7: Add CLI dependencies

### Step 7.1 — Update `apps/cli/Cargo.toml`

```toml
[dependencies]
musicum-core             = { path = "../../libs/musicum-core" }
structural-processors    = { path = "../../libs/structural-processors" }
structural-processor-sdk = { path = "../../libs/structural-processor-sdk" }
clap                     = { version = "4", features = ["derive"] }
sea-orm.workspace        = true
tokio.workspace          = true
serde_json.workspace     = true
anyhow.workspace         = true
serde.workspace          = true
uuid.workspace           = true
crossterm                = "0.28"
slug                     = "0.1"
```

### Step 7.2 — Verify it compiles

```
cargo build -p musicum-cli
```

---

## Task 8: Wire `Commands::Processors` and fix presets in CLI `main.rs`

### Step 8.1 — Update `apps/cli/src/main.rs`

```rust
mod commands;
mod output;
mod settings;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "musicum", about = "Musicum audio library CLI", version)]
struct Cli {
    #[arg(long, global = true)]
    library: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Sync,
    Files(commands::files::FilesArgs),
    Clips(commands::clips::ClipsArgs),
    Collections(commands::collections::CollectionsArgs),
    Presets(commands::presets::PresetsArgs),
    Processors(commands::processors::ProcessorsArgs),
    Play {
        target: String,
        #[arg(long, conflicts_with = "clip")]
        file: bool,
        #[arg(long, conflicts_with = "file")]
        clip: bool,
    },
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut app_settings = settings::load()?;
    if let Some(lib) = cli.library {
        app_settings.library_dir = lib;
    }

    match cli.command {
        Commands::Config => {
            println!("Settings file: {}", settings::settings_path().display());
            println!("Library dir:   {}", app_settings.library_dir);
            if let Some(gen) = &app_settings.generated_dir {
                println!("Generated dir: {gen}");
            }
            return Ok(());
        }
        _ => {}
    }

    let db = musicum_core::db::connect(&app_settings.library_dir).await?;
    let library_dir = app_settings.library_dir.as_str();

    match cli.command {
        Commands::Sync        => commands::sync::run(&db, &app_settings).await?,
        Commands::Files(a)    => commands::files::run(&db, a).await?,
        Commands::Clips(a)    => commands::clips::run(&db, a).await?,
        Commands::Collections(a) => commands::collections::run(&db, a).await?,
        Commands::Presets(a)  => commands::presets::run(&db, library_dir, a).await?,
        Commands::Processors(a) => commands::processors::run(a),
        Commands::Play { target, file, clip } =>
            commands::play::run(&db, target, file, clip).await?,
        Commands::Config => unreachable!(),
    }

    Ok(())
}
```

### Step 8.2 — Export `processors` in `apps/cli/src/commands/mod.rs`

```rust
pub mod clips;
pub mod collections;
pub mod files;
pub mod play;
pub mod presets;
pub mod processors;
pub mod sync;
```

---

## Task 9: Implement `apps/cli/src/commands/processors.rs`

**Why:** `processors list` reads the registry from `structural_processors::registry()` and formats it as a table or JSON array.

### Step 9.1 — Create the file

```rust
// apps/cli/src/commands/processors.rs
use clap::{Args, Subcommand};
use structural_processor_sdk::processor::ParameterDescriptor;

use crate::output::{print_json, print_table_3col};

#[derive(Debug, Args)]
pub struct ProcessorsArgs {
    #[command(subcommand)]
    pub command: ProcessorsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProcessorsCommand {
    List {
        #[arg(long)]
        json: bool,
    },
}

pub fn run(args: ProcessorsArgs) {
    match args.command {
        ProcessorsCommand::List { json } => {
            let registry = structural_processors::registry();
            if json {
                let descriptors: Vec<_> =
                    registry.iter().map(|e| (e.descriptor)()).collect();
                print_json(&descriptors);
            } else if registry.is_empty() {
                println!("No processors registered.");
            } else {
                let rows: Vec<(String, String, String)> = registry
                    .iter()
                    .map(|e| {
                        let d = (e.descriptor)();
                        let params = d
                            .parameters
                            .iter()
                            .map(|p| match p {
                                ParameterDescriptor::Time { id, default, .. } => {
                                    format!("{id}={default} (time)")
                                }
                                ParameterDescriptor::Int { id, default, .. } => {
                                    format!("{id}={default} (int)")
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        (d.id.to_string(), d.name.to_string(), params)
                    })
                    .collect();
                print_table_3col(("ID", "NAME", "PARAMETERS"), rows);
            }
        }
    }
}
```

### Step 9.2 — Add `print_table_3col` to `apps/cli/src/output.rs`

```rust
pub fn print_table_3col(
    headers: (&str, &str, &str),
    rows: Vec<(String, String, String)>,
) {
    let c1 = rows.iter().map(|(a, _, _)| a.len()).max().unwrap_or(0).max(headers.0.len());
    let c2 = rows.iter().map(|(_, b, _)| b.len()).max().unwrap_or(0).max(headers.1.len());
    println!("{:<c1$}  {:<c2$}  {}", headers.0, headers.1, headers.2);
    println!("{}", "─".repeat(c1 + c2 + 4 + headers.2.len().min(60)));
    for (a, b, c) in rows {
        println!("{a:<c1$}  {b:<c2$}  {c}");
    }
}
```

### Step 9.3 — Build and smoke-test

```
cargo build -p musicum-cli
musicum processors list
```

Expected output:
```
ID      NAME    PARAMETERS
────────────────────────────────────────────────────
trim    Trim    start=0.0 (time), end=0.0 (time)
cut     Cut     from=0.0 (time), to=0.0 (time)
slice   Slice   at=0.0 (time)
crop    Crop    start=0.0 (time), end=0.0 (time)
```

---

## Task 10: Extend `apps/cli/src/commands/presets.rs`

### Step 10.1 — Add new subcommands to `PresetsCommand` enum

```rust
#[derive(Debug, Subcommand)]
pub enum PresetsCommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Remove {
        slug: String,
    },
    AddProcessor {
        preset_slug: String,
        processor_type: String,
    },
    RemoveProcessor {
        preset_slug: String,
        instance_uuid: String,
    },
}
```

### Step 10.2 — Update `run` signature to accept `library_dir`

```rust
pub async fn run(db: &DatabaseConnection, library_dir: &str, args: PresetsArgs) -> Result<()> {
```

### Step 10.3 — Implement all match arms

Full `run` function:

```rust
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use musicum_core::services::preset_service;
use musicum_core::sidecar::{self, ProcessorEntry, ProcessorRef};
use sea_orm::DatabaseConnection;
use slug::slugify;
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;
use std::path::Path;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct PresetsArgs {
    #[command(subcommand)]
    pub command: PresetsCommand,
}

#[derive(Debug, Subcommand)]
pub enum PresetsCommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Remove {
        slug: String,
    },
    AddProcessor {
        preset_slug: String,
        processor_type: String,
    },
    RemoveProcessor {
        preset_slug: String,
        instance_uuid: String,
    },
}

pub async fn run(db: &DatabaseConnection, library_dir: &str, args: PresetsArgs) -> Result<()> {
    match args.command {
        PresetsCommand::List { json } => {
            let presets = preset_service::list_presets(db).await?;
            if json {
                print_json(&presets);
            } else if presets.is_empty() {
                println!("No presets. Add a sidecar under .musicum/presets/ and run sync.");
            } else {
                print_table(
                    ("SLUG", "TITLE"),
                    presets.iter().map(|p| (p.slug.clone(), p.title.clone())).collect(),
                );
            }
        }

        PresetsCommand::Show { slug, json } => {
            let preset = preset_service::get_preset_by_slug(db, &slug).await?;
            if json {
                print_json(&preset);
            } else {
                let processors: Vec<ProcessorEntry> =
                    serde_json::from_str(&preset.processors).unwrap_or_default();
                print_detail(vec![
                    ("slug", preset.slug.clone()),
                    ("title", preset.title.clone()),
                    ("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
                ]);
                if processors.is_empty() {
                    println!("\nprocessors: (none)");
                } else {
                    println!("\nprocessors:");
                    let uuid_w = 36;
                    let kind_w = 12;
                    let proc_w = 6;
                    println!("  {:<uuid_w$}  {:<kind_w$}  {:<proc_w$}  ENABLED  PARAMS",
                        "UUID", "KIND", "PROC");
                    println!("  {}", "─".repeat(uuid_w + kind_w + proc_w + 30));
                    for entry in &processors {
                        let (id, kind, proc_id, enabled, params) = match entry {
                            ProcessorEntry::Structural { id, enabled, processor } => (
                                id.as_str(), "structural", processor.id.as_str(), *enabled,
                                format_params(&processor.params),
                            ),
                            ProcessorEntry::AudioPlugin { id, enabled, processor } => (
                                id.as_str(), "audio-plugin", processor.id.as_str(), *enabled,
                                format_params(&processor.params),
                            ),
                        };
                        println!(
                            "  {:<uuid_w$}  {:<kind_w$}  {:<proc_w$}  {:<7}  {}",
                            id, kind, proc_id, enabled, params
                        );
                    }
                }
            }
        }

        PresetsCommand::Create { title, description } => {
            let slug = slugify(&title);
            preset_service::create_preset(db, library_dir, &slug, &title, &description).await?;
            println!("{slug}");
        }

        PresetsCommand::Remove { slug } => {
            preset_service::delete_preset(db, library_dir, &slug).await?;
            println!("removed '{slug}'");
        }

        PresetsCommand::AddProcessor { preset_slug, processor_type } => {
            let registry = structural_processors::registry();
            let entry = registry
                .iter()
                .find(|e| (e.descriptor)().id == processor_type)
                .ok_or_else(|| {
                    let valid: Vec<&str> = registry.iter().map(|e| (e.descriptor)().id).collect();
                    anyhow::anyhow!(
                        "unknown processor type '{}'. Valid types: {}",
                        processor_type,
                        valid.join(", ")
                    )
                })?;

            let descriptor = (entry.descriptor)();
            let mut default_params = serde_json::Map::new();
            for p in descriptor.parameters {
                let val = match p {
                    ParameterDescriptor::Time { id, default, .. } => {
                        (*id, serde_json::json!(*default))
                    }
                    ParameterDescriptor::Int { id, default, .. } => {
                        (*id, serde_json::json!(*default))
                    }
                };
                default_params.insert(val.0.to_string(), val.1);
            }

            let instance_id = Uuid::new_v4().to_string();
            let new_entry = ProcessorEntry::Structural {
                id: instance_id.clone(),
                enabled: true,
                processor: ProcessorRef {
                    id: processor_type.clone(),
                    params: serde_json::Value::Object(default_params),
                },
            };

            let lib = Path::new(library_dir);
            let mut sc = sidecar::read_preset_sidecar(lib, &preset_slug)?;
            sc.processors.push(new_entry);
            sidecar::write_preset_sidecar(lib, &sc)?;
            preset_service::update_preset_processors(db, library_dir, &preset_slug, sc.processors).await?;

            println!("{instance_id}");
        }

        PresetsCommand::RemoveProcessor { preset_slug, instance_uuid } => {
            let lib = Path::new(library_dir);
            let mut sc = sidecar::read_preset_sidecar(lib, &preset_slug)?;
            let original_len = sc.processors.len();
            sc.processors.retain(|e| {
                let id = match e {
                    ProcessorEntry::Structural { id, .. } => id.as_str(),
                    ProcessorEntry::AudioPlugin { id, .. } => id.as_str(),
                };
                id != instance_uuid
            });
            if sc.processors.len() == original_len {
                bail!("processor '{instance_uuid}' not found in preset '{preset_slug}'");
            }
            sidecar::write_preset_sidecar(lib, &sc)?;
            preset_service::update_preset_processors(db, library_dir, &preset_slug, sc.processors).await?;
            println!("removed processor '{instance_uuid}'");
        }
    }
    Ok(())
}

fn format_params(params: &serde_json::Value) -> String {
    match params.as_object() {
        None => "{}".to_string(),
        Some(map) => map
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
```

### Step 10.4 — Build

```
cargo build -p musicum-cli
```

Fix any type errors. Common pitfall: `serde_json::from_str::<Vec<ProcessorEntry>>(&preset.processors).unwrap_or_default()` — requires `ProcessorEntry` to implement `Default` (it doesn't; use `unwrap_or_else(|_| vec![])` instead).

---

## Task 11: End-to-end smoke test

Assumes a library directory at `~/Music/musicum-lib` (or use `--library /tmp/test-lib`):

```bash
# 1. List processors
musicum processors list

# 2. Create a preset
musicum presets create --title "My Mix"
# → my-mix

# 3. List presets
musicum presets list
# → my-mix  My Mix

# 4. Add a processor
musicum presets add-processor my-mix trim
# → <uuid>

# 5. Show preset with processor
musicum presets show my-mix

# 6. Add another processor
musicum presets add-processor my-mix cut

# 7. Remove a processor (use the UUID from step 4)
musicum presets remove-processor my-mix <uuid-from-step-4>

# 8. Confirm only cut remains
musicum presets show my-mix

# 9. Remove the preset entirely
musicum presets remove my-mix

# 10. Confirm it's gone
musicum presets list
```

Also verify: `musicum processors list --json` produces valid JSON, and `musicum presets show my-mix --json` does the same.

---

## Task 12: Final test run

```
cargo test -p structural-processor-sdk
cargo test -p structural-processors
cargo test -p musicum-core
cargo build -p musicum-cli
```

All tests green, binary builds clean.

---

## Execution Handoff

**Plan complete and saved to `docs/plans/2026-05-24-structural-processor-cli-design.md`.**

- **REQUIRED SUB-SKILL:** Use execute-plan
- Batch execution with checkpoints for review

ARGUMENTS: 2026-05-24-structural-processor-cli-design.md
