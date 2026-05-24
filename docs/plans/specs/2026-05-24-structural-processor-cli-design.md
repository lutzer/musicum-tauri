# Structural Processor CLI Integration

**Date:** 2026-05-24  
**Status:** Approved

## Overview

Extend the existing `musicum` CLI with two new capabilities:

1. **`musicum processors`** — list all registered structural processor descriptors
2. **`musicum presets` (extended)** — full CRUD on presets and their structural processor chains

Presets are the bridge between the processor registry and tracks: a named, ordered chain of structural processor entries that can later be applied to a clip.

---

## CLI Surface

### New: `musicum processors`

```
musicum processors list [--json]
```

Lists all registered structural processors with their parameter definitions.

Table output (default):
```
ID      NAME    PARAMETERS
trim    Trim    start=0.0 (time), end=0.0 (time)
cut     Cut     from=0.0 (time), to=0.0 (time)
slice   Slice   at=0.0 (time)
crop    Crop    start=0.0 (time), end=0.0 (time)
```

JSON output (`--json`): array of `ProcessorDescriptor` objects as returned by `descriptors_json`.

### Extended: `musicum presets`

All existing subcommands (`list`, `show`) are retained unchanged except `show`, which is enhanced to display the processor chain.

| Command | Description |
|---|---|
| `musicum presets list [--json]` | List all presets (slug, title) |
| `musicum presets show <slug> [--json]` | Show preset detail including processor chain with UUIDs |
| `musicum presets create --title <title> [--description <desc>]` | Create empty preset; slug auto-generated from title |
| `musicum presets remove <slug>` | Delete preset sidecar and DB row |
| `musicum presets add-processor <preset-slug> <processor-type>` | Append a processor instance with default params; prints instance UUID |
| `musicum presets remove-processor <preset-slug> <instance-uuid>` | Remove a specific processor instance by UUID |

#### `presets create` details

- Slug is derived via `slugify(title)` (same function used for file slugs).
- Errors if a preset with that slug already exists on disk.
- Prints the generated slug on success.

#### `presets show` enhanced output

```
slug:        my-preset
title:       My Preset
description: -

processors:
  UUID                                  KIND          PROC  ENABLED  PARAMS
  a1b2c3d4-e5f6-7890-abcd-ef1234567890  structural    trim  true     start=0.0, end=0.0
  b2c3d4e5-f6a7-8901-bcde-f12345678901  structural    cut   true     from=0.5, to=1.0
```

#### `presets add-processor` details

- `<processor-type>` must match a registered processor id (e.g. `trim`, `cut`, `slice`, `crop`). Errors with a helpful message listing valid types if not found.
- All parameters are set to their declared defaults from `ParameterDescriptor`.
- A new UUID v4 is generated for the instance `id`.
- The entry is appended to the end of the processor chain.
- Prints the generated instance UUID.

#### `presets remove-processor` details

- Errors with "processor not found in preset" if the UUID doesn't match any entry.

---

## Architecture

### Registry Access

A public `registry()` function is added to `structural-processors/src/lib.rs` — the single source of truth for which processors are registered:

```rust
// structural-processors/src/lib.rs
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
```

`structural-processors/src/main.rs` drops its duplicate `fn registry()` and calls `crate::registry()` instead.

`apps/cli/Cargo.toml` gains:
```toml
structural-processors    = { path = "../../libs/structural-processors" }
structural-processor-sdk = { path = "../../libs/structural-processor-sdk" }
```

The CLI calls `structural_processors::registry()` for both `processors list` (descriptor output) and for resolving default params in `add-processor`. `musicum-core` has no registry dependency — it only handles the sidecar/DB data shapes.

### `ProcessorEntry` in Sidecar

`sidecar::ProcessorEntry` is redesigned as an internally-tagged enum that distinguishes structural processors from audio-plugin processors. The existing flat struct is replaced:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorRef {
    pub id:     String,            // processor/plugin id, e.g. "trim" or "gain"
    pub params: serde_json::Value, // {"start": 0.0, "end": 0.0}
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProcessorEntry {
    Structural {
        id:        String, // instance UUID
        enabled:   bool,
        processor: ProcessorRef,
    },
    #[serde(rename = "audio-plugin")]
    AudioPlugin {
        id:        String, // instance UUID
        enabled:   bool,
        processor: ProcessorRef,
    },
}
```

Serialises to:
```json
[
  {
    "type": "structural",
    "id": "a1b2c3d4-0000-0000-0000-000000000001",
    "enabled": true,
    "processor": { "id": "trim", "params": { "start": 0.0, "end": 0.0 } }
  },
  {
    "type": "audio-plugin",
    "id": "e5f6a7b8-0000-0000-0000-000000000002",
    "enabled": true,
    "processor": { "id": "gain", "params": { "gain": 0.8 } }
  }
]
```

`add-processor` always creates a `ProcessorEntry::Structural` variant. Audio-plugin entries are out of scope for this spec.

The `ProcessorEntry` enum replaces the old flat struct in both `ClipSidecar.processors` and `PresetSidecar.processors`.

### Preset Persistence (Sidecar + DB)

All write operations follow the rule from CLAUDE.md: write sidecar first, then immediately propagate to the DB. No separate `musicum sync` step required for preset mutations.

**Write path for create / add-processor / remove-processor:**
1. Read current `PresetSidecar` from `.musicum/presets/<slug>.musicum-preset.json` (or construct new)
2. Mutate in memory
3. Call `sidecar::write_preset_sidecar(library_dir, &sc)`
4. Upsert the single DB row via `preset_service`

**Write path for remove:**
1. Delete `.musicum/presets/<slug>.musicum-preset.json`
2. Delete the DB row via `preset_service::delete_preset`

### `preset_service` New Functions

All live in `libs/musicum-core/src/services/preset_service.rs`:

```rust
pub async fn create_preset(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
    title: &str,
    description: &str,
) -> Result<preset::Model, ServiceError>

pub async fn delete_preset(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
) -> Result<(), ServiceError>

pub async fn update_preset_processors(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError>
```

The CLI command handlers read the single preset sidecar file directly (`.musicum/presets/<slug>.musicum-preset.json`) to load the current state, mutate, then call `update_preset_processors`.

### Passing `library_dir` to Preset Commands

`commands::presets::run` currently receives only `db`. The signature is extended to:
```rust
pub async fn run(
    db: &DatabaseConnection,
    library_dir: &str,
    args: PresetsArgs,
) -> Result<()>
```

`main.rs` passes `app_settings.library_dir.as_str()` alongside `db`.

---

## File Changes

| File | Change |
|---|---|
| `libs/structural-processor-sdk/src/lib.rs` | Rename `ProcessorEntry` → `StructuralProcessorEntry` throughout |
| `libs/structural-processors/src/lib.rs` | Add `pub fn registry()` — single source of truth for the processor vtable; update to `StructuralProcessorEntry` |
| `libs/structural-processors/src/main.rs` | Drop duplicate `fn registry()`; call `crate::registry()`; update to `StructuralProcessorEntry` |
| `apps/cli/Cargo.toml` | Add `structural-processors`, `structural-processor-sdk` deps |
| `apps/cli/src/main.rs` | Add `Commands::Processors`; pass `library_dir` to presets |
| `apps/cli/src/commands/mod.rs` | Export new `processors` module |
| `apps/cli/src/commands/processors.rs` | New file — `processors list` implementation |
| `apps/cli/src/commands/presets.rs` | Add `create`, `remove`, `add-processor`, `remove-processor`; enhance `show` |
| `libs/musicum-core/src/sidecar.rs` | Replace `ProcessorEntry` flat struct with tagged enum + `ProcessorRef` struct |
| `libs/musicum-core/src/services/preset_service.rs` | Add `create_preset`, `delete_preset`, `update_preset_processors` |

No DB schema changes (processors column remains a JSON text blob). Sidecar format changes: `ProcessorEntry` gains the `type` discriminator and nested `processor` object. Existing sidecars in the old flat format will fail to deserialise — acceptable in the current dev-only state.

---

## Future Work (Out of Scope)

- **Apply preset to a clip/track**: Copy the preset's processor chain into the clip's sidecar. Tracked separately.
- **Edit processor params**: Modify individual parameter values on an existing processor instance in a preset.
- **Reorder processors**: Change the position of a processor within the chain.
