# Plugin/Processor Registry Refactor Implementation Plan

**Goal:** Remove direct SDK deps from the CLI, expose a frontend-safe `ParamInfo`/`EditEntry` interface from `musicum-core`, and split decode/plugin logic out of `player.rs` into `processor_chain.rs`.

**Architecture:** Three independent passes applied in order: (1) add the registry query API in musicum-core with tests, (2) extract processor/plugin logic from player.rs into a new sibling file, (3) strip SDK deps from the CLI and rewrite processors.rs to use the new API. Each pass compiles and tests green before the next begins.

**Tech Stack:** Rust, `audio-plugin-sdk`, `structural-processor-sdk`, `musicum-core`, `cargo test`, `cargo clippy`

---

## File Map

| Action | Path | Responsibility after change |
|---|---|---|
| Modify | `libs/musicum-core/src/audio/registry.rs` | Add `ParamInfo`, `EditEntry`, `EditType`, `list_entries()`, `get_entry()` |
| Modify | `libs/musicum-core/src/audio/mod.rs` | Declare `processor_chain` module; re-export new types |
| Modify | `libs/musicum-core/src/lib.rs` | Re-export `ParamInfo`, `EditEntry`, `EditType` |
| Create | `libs/musicum-core/src/audio/processor_chain.rs` | `PluginHandle`, constants, `build_plugin_handles`, `build_fresh_chain`, `decode_loop` |
| Modify | `libs/musicum-core/src/audio/player.rs` | Give `PlaybackState` `pub(super)`; remove moved items; import from processor_chain |
| Modify | `apps/cli/Cargo.toml` | Remove `audio-plugin-sdk`, `structural-processors`, `structural-processor-sdk` |
| Modify | `apps/cli/src/commands/processors.rs` | Use `registry.list_entries()` instead of SDK type matches |

---

## Pass 1 — Registry query API

### Task 1: Write failing tests for `list_entries` and `get_entry`

**Files:**
- Modify: `libs/musicum-core/src/audio/registry.rs`

Add at the bottom of the existing `#[cfg(test)] mod tests` block (before the closing `}`):

```rust
    #[test]
    fn list_entries_contains_all_structural() {
        let reg = EditRegistry::default();
        let entries = reg.list_entries();
        for id in ["trim", "cut", "slice", "crop"] {
            assert!(
                entries.iter().any(|e| e.id == id && matches!(e.edit_type, EditType::Structural)),
                "missing structural entry '{id}'"
            );
        }
    }

    #[test]
    fn list_entries_contains_all_plugins() {
        let reg = EditRegistry::default();
        let entries = reg.list_entries();
        for id in ["gain", "reverb", "pan", "normalize", "level-meter", "oscilloscope"] {
            assert!(
                entries.iter().any(|e| e.id == id && matches!(e.edit_type, EditType::Plugin)),
                "missing plugin entry '{id}'"
            );
        }
    }

    #[test]
    fn get_entry_gain_has_float_param() {
        let reg = EditRegistry::default();
        let entry = reg.get_entry("gain").unwrap();
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Float { id, .. } if *id == "gain")));
    }

    #[test]
    fn get_entry_trim_has_time_params() {
        let reg = EditRegistry::default();
        let entry = reg.get_entry("trim").unwrap();
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Time { id, .. } if *id == "start")));
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Time { id, .. } if *id == "end")));
    }

    #[test]
    fn get_entry_unknown_returns_none() {
        let reg = EditRegistry::default();
        assert!(reg.get_entry("nonexistent").is_none());
    }
```

Run — expect compile errors because the types don't exist yet:

```
cargo test -p musicum-core audio::registry
```

### Task 2: Add `ParamInfo`, `EditEntry`, `EditType` types

**Files:**
- Modify: `libs/musicum-core/src/audio/registry.rs`

Insert after the `use` lines at the top, before `pub struct EditRegistry`:

```rust
/// Parameter metadata exposed to frontends without importing plugin/processor SDKs.
/// `Action` and `Canvas` plugin parameters are excluded (no persistent value).
pub enum ParamInfo {
    Float {
        id:      &'static str,
        name:    &'static str,
        default: f32,
        min:     f32,
        max:     f32,
        step:    f32,
        unit:    Option<&'static str>,
    },
    Bool  { id: &'static str, name: &'static str, default: bool },
    Time  { id: &'static str, name: &'static str, default: f64 },
    Int   { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

pub enum EditType { Structural, Plugin }

pub struct EditEntry {
    pub id:         String,
    pub name:       &'static str,
    pub edit_type:  EditType,
    pub parameters: Vec<ParamInfo>,
}
```

### Task 3: Implement `list_entries` and `get_entry`

**Files:**
- Modify: `libs/musicum-core/src/audio/registry.rs`

Add a new `impl EditRegistry` block after `impl Default for EditRegistry { ... }`:

```rust
impl EditRegistry {
    /// Return all registered processors and plugins as frontend-safe entries.
    pub fn list_entries(&self) -> Vec<EditEntry> {
        let mut entries = Vec::new();

        for (id, entry) in &self.structural {
            let d = (entry.descriptor)();
            let parameters = d.parameters.iter().map(|p| match p {
                ParameterDescriptor::Time { id, name, default } =>
                    ParamInfo::Time { id, name, default: *default },
                ParameterDescriptor::Int { id, name, default, min, max } =>
                    ParamInfo::Int { id, name, default: *default, min: *min, max: *max },
            }).collect();
            entries.push(EditEntry {
                id: id.clone(),
                name: d.name,
                edit_type: EditType::Structural,
                parameters,
            });
        }

        for (id, entry) in &self.plugins {
            let d = (entry.descriptor)();
            let parameters = d.parameters.iter().filter_map(|p| match p {
                PluginParameter::Float { id, name, default, min, max, step, unit, .. } =>
                    Some(ParamInfo::Float {
                        id, name,
                        default: *default, min: *min, max: *max, step: *step,
                        unit: if unit.is_empty() { None } else { Some(unit) },
                    }),
                PluginParameter::Bool { id, name, default, .. } =>
                    Some(ParamInfo::Bool { id, name, default: *default }),
                PluginParameter::Action { .. } | PluginParameter::Canvas { .. } => None,
            }).collect();
            entries.push(EditEntry {
                id: id.clone(),
                name: d.name,
                edit_type: EditType::Plugin,
                parameters,
            });
        }

        entries
    }

    /// Look up a single entry by processor or plugin ID.
    pub fn get_entry(&self, id: &str) -> Option<EditEntry> {
        self.list_entries().into_iter().find(|e| e.id == id)
    }
}
```

The `use` block at the top of registry.rs already imports `PluginRegistry`; also add `PluginParameter` to the import:

```rust
use audio_plugin_sdk::{PluginEntry, PluginParameter, PluginRegistry};
use structural_processor_sdk::{processor::ParameterDescriptor, Registry as StructuralRegistry};
```

(Remove `PluginEntry` from the `use audio_plugin_sdk::PluginEntry` inside `impl Default` since it's now at the top.)

### Task 4: Run the tests — expect green

```
cargo test -p musicum-core audio::registry
```

Expected output: all 8 tests pass (3 original + 5 new).

### Task 5: Re-export new types

**Files:**
- Modify: `libs/musicum-core/src/audio/mod.rs`
- Modify: `libs/musicum-core/src/lib.rs`

In `audio/mod.rs`, change:

```rust
pub use registry::EditRegistry;
```

to:

```rust
pub use registry::{EditEntry, EditRegistry, EditType, ParamInfo};
```

In `lib.rs`, change:

```rust
pub use audio::{structural_edits_from, EditRegistry, PlaybackEngine};
```

to:

```rust
pub use audio::{structural_edits_from, EditEntry, EditRegistry, EditType, ParamInfo, PlaybackEngine};
```

### Task 6: Run clippy — expect clean

```
cargo clippy -p musicum-core
```

Expected: zero warnings or errors.

---

## Pass 2 — Extract processor_chain.rs

### Task 7: Create `processor_chain.rs`

**Files:**
- Create: `libs/musicum-core/src/audio/processor_chain.rs`

This file contains everything decode-related that currently lives in `player.rs`. Copy the constants, `PluginHandle`, and the two free functions verbatim, then add `build_plugin_handles`. The only new import compared to `player.rs` is `super::player::PlaybackState` (added in Task 8).

```rust
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use audio_plugin_sdk::PluginProcessor;
use structural_processor_sdk::{
    chain::{build_chain, StructuralEdit},
    AudioSource,
};
use uuid::Uuid;

use super::registry::EditRegistry;
use super::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};

pub(super) const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
pub(super) const CHUNK_SAMPLES:   usize = 4_096;

pub(super) struct PluginHandle {
    pub uuid:      Uuid,
    pub enabled:   AtomicBool,
    pub processor: Mutex<Box<dyn PluginProcessor>>,
}

/// Instantiate one `PluginHandle` per plugin edit (enabled and disabled alike).
pub(super) fn build_plugin_handles(
    edits: &[ProcessorEdit],
    registry: &EditRegistry,
) -> Vec<Arc<PluginHandle>> {
    let mut handles = Vec::new();
    for edit in edits {
        if let EditKind::Plugin { plugin_id, params } = &edit.kind {
            if let Some(entry) = registry.plugins.get(plugin_id) {
                let mut instance = (entry.create)();
                for (id, &val) in params {
                    instance.set_parameter(id, val);
                }
                handles.push(Arc::new(PluginHandle {
                    uuid:      edit.uuid,
                    enabled:   AtomicBool::new(edit.enabled),
                    processor: Mutex::new(instance),
                }));
            } else {
                eprintln!("warning: unknown plugin '{plugin_id}' — skipped");
            }
        }
    }
    handles
}

pub(super) fn build_fresh_chain(
    path: &Path,
    edits: &[StructuralEdit],
    registry: &structural_processor_sdk::Registry,
) -> Result<Box<dyn AudioSource>> {
    let source = Box::new(FileAudioSource::new(path)?);
    Ok(build_chain(source, edits, registry))
}

pub(super) fn decode_loop(
    path: PathBuf,
    structural_edits: Vec<StructuralEdit>,
    plugin_handles: Vec<Arc<PluginHandle>>,
    state: Arc<super::player::PlaybackState>,
) {
    let registry = structural_processors::registry();

    let mut chain = match build_fresh_chain(&path, &structural_edits, &registry) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("decode init error: {e}");
            state.finished.store(true, Ordering::Relaxed);
            return;
        }
    };

    let sample_rate = state.sample_rate;
    let ch = {
        let probe = FileAudioSource::new(&path);
        probe.map(|s| s.channels()).unwrap_or(2)
    } as usize;

    let mut cursor_secs = 0.0_f64;
    let total_secs = state.total_frames.load(Ordering::Relaxed) as f64 / sample_rate as f64;

    loop {
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target) = req.take() {
                match build_fresh_chain(&path, &structural_edits, &registry) {
                    Ok(c) => {
                        chain = c;
                        cursor_secs = target;
                        let frame_pos = (target * sample_rate as f64) as u64;
                        state.position.store(frame_pos, Ordering::Relaxed);
                        if let Ok(mut buf) = state.buffer.lock() { buf.clear(); }
                        for handle in &plugin_handles {
                            if let Ok(mut p) = handle.processor.lock() { p.reset(); }
                        }
                    }
                    Err(e) => eprintln!("seek chain rebuild error: {e}"),
                }
            }
        }

        if state.paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        if cursor_secs >= total_secs {
            while state.buffer.lock().map(|b| b.len()).unwrap_or(1) > 0 {
                thread::sleep(Duration::from_millis(5));
            }
            state.position.store(0, Ordering::Relaxed);
            if !state.looping.load(Ordering::Relaxed) {
                state.paused.store(true, Ordering::Relaxed);
            }
            chain = match build_fresh_chain(&path, &structural_edits, &registry) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("decode rebuild error: {e}");
                    state.finished.store(true, Ordering::Relaxed);
                    return;
                }
            };
            cursor_secs = 0.0;
            continue;
        }

        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let mut samples = chain.read_at(cursor_secs, CHUNK_SAMPLES);
        if samples.is_empty() {
            while state.buffer.lock().map(|b| b.len()).unwrap_or(1) > 0 {
                thread::sleep(Duration::from_millis(5));
            }
            state.position.store(0, Ordering::Relaxed);
            if !state.looping.load(Ordering::Relaxed) {
                state.paused.store(true, Ordering::Relaxed);
            }
            chain = match build_fresh_chain(&path, &structural_edits, &registry) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("decode rebuild error: {e}");
                    state.finished.store(true, Ordering::Relaxed);
                    return;
                }
            };
            cursor_secs = 0.0;
            continue;
        }

        for handle in &plugin_handles {
            if !handle.enabled.load(Ordering::Relaxed) { continue; }
            if let Ok(mut plugin) = handle.processor.lock() {
                plugin.process(&mut samples, ch, sample_rate as f32, cursor_secs);
            }
        }

        cursor_secs += samples.len() as f64 / (sample_rate as f64 * ch as f64);
        if let Ok(mut buf) = state.buffer.lock() {
            buf.extend(&samples);
        }
    }
}
```

Note: `VecDeque`, `AtomicU64`, and `Duration` are imported here because they're used in the loop body via `state`. `Duration` is also needed for the `thread::sleep` calls.

### Task 8: Declare the new module and give `PlaybackState` `pub(super)` visibility

**Files:**
- Modify: `libs/musicum-core/src/audio/mod.rs`
- Modify: `libs/musicum-core/src/audio/player.rs`

In `audio/mod.rs`, add `mod processor_chain;` at the top:

```rust
mod processor_chain;
pub mod player;
pub mod registry;
pub mod source;
// ... rest unchanged
```

In `player.rs`, change the `PlaybackState` declaration from:

```rust
struct PlaybackState {
```

to:

```rust
pub(super) struct PlaybackState {
```

### Task 9: Trim `player.rs` — remove moved items, import from `processor_chain`

**Files:**
- Modify: `libs/musicum-core/src/audio/player.rs`

**9a — Update imports.** Replace the existing import block at the top of `player.rs`:

```rust
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use structural_processor_sdk::chain::{chain_output_duration, StructuralEdit};
use uuid::Uuid;

use super::processor_chain::{build_plugin_handles, decode_loop, PluginHandle, BUFFER_CAPACITY};
use crate::audio::registry::EditRegistry;
use crate::audio::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};
```

Removed vs. original: `audio_plugin_sdk::PluginProcessor`, `build_chain`, `AudioSource`, `Duration`, `VecDeque` (the buffer lives in `PlaybackState`; its type is declared here but the capacity constant moves to `processor_chain`).

Wait — `VecDeque` is still used in `PlaybackState` (`buffer: Mutex<VecDeque<f32>>`), so keep it:

```rust
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use structural_processor_sdk::chain::{chain_output_duration, StructuralEdit};
use uuid::Uuid;

use super::processor_chain::{build_plugin_handles, decode_loop, PluginHandle, BUFFER_CAPACITY};
use crate::audio::registry::EditRegistry;
use crate::audio::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};
```

**9b — Remove constants.** Delete these two lines (now in `processor_chain.rs`):

```rust
const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
const CHUNK_SAMPLES: usize = 4_096;
```

**9c — Remove `PluginHandle` struct definition.** Delete the entire block:

```rust
struct PluginHandle {
    uuid:      Uuid,
    enabled:   AtomicBool,
    processor: Mutex<Box<dyn PluginProcessor>>,
}
```

**9d — Simplify `PlaybackEngine::new`.** Replace the inline plugin-handle-building loop:

```rust
        // Build plugin handles (only enabled plugins get an instance)
        let mut plugin_handles: Vec<Arc<PluginHandle>> = Vec::new();
        for edit in edits {
            if let EditKind::Plugin { plugin_id, params } = &edit.kind {
                if let Some(entry) = registry.plugins.get(plugin_id) {
                    let mut instance = (entry.create)();
                    for (id, &val) in params {
                        instance.set_parameter(id, val);
                    }
                    plugin_handles.push(Arc::new(PluginHandle {
                        uuid:      edit.uuid,
                        enabled:   AtomicBool::new(edit.enabled),
                        processor: Mutex::new(instance),
                    }));
                } else {
                    eprintln!("warning: unknown plugin '{plugin_id}' — skipped");
                }
            }
        }
```

with the single call:

```rust
        let plugin_handles = build_plugin_handles(edits, registry);
```

**9e — Remove `build_fresh_chain` and `decode_loop` free functions.** Delete both function definitions (lines 272–388 in the original). They now live in `processor_chain.rs`.

### Task 10: Run the full core test suite — expect green

```
cargo test -p musicum-core
```

Expected: all existing tests pass unchanged (the `PlaybackEngine` tests in `player.rs` exercise the same behaviour through the same public API).

### Task 11: Run clippy — expect clean

```
cargo clippy -p musicum-core
```

---

## Pass 3 — CLI cleanup

### Task 12: Remove SDK deps from CLI

**Files:**
- Modify: `apps/cli/Cargo.toml`

Remove these three lines:

```toml
audio-plugin-sdk         = { path = "../../libs/audio-plugin-sdk" }
structural-processors    = { path = "../../libs/structural-processors" }
structural-processor-sdk = { path = "../../libs/structural-processor-sdk" }
```

### Task 13: Rewrite `processors.rs` to use `list_entries`

**Files:**
- Modify: `apps/cli/src/commands/processors.rs`

Full replacement:

```rust
use clap::{Args, Subcommand};
use musicum_core::{EditRegistry, EditType, ParamInfo};
use serde::Serialize;

use crate::output::{print_json, print_table};

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

#[derive(Serialize)]
struct ProcessorListEntry {
    id:         String,
    #[serde(rename = "type")]
    kind:       String,
    name:       String,
    parameters: Vec<String>,
}

pub fn run(args: ProcessorsArgs) {
    match args.command {
        ProcessorsCommand::List { json } => {
            let registry = EditRegistry::default();
            let mut entries: Vec<ProcessorListEntry> = registry
                .list_entries()
                .into_iter()
                .map(|e| {
                    let kind = match e.edit_type {
                        EditType::Structural => "structural",
                        EditType::Plugin     => "audio-plugin",
                    }
                    .to_string();
                    let parameters = e
                        .parameters
                        .iter()
                        .map(|p| match p {
                            ParamInfo::Float { id, default, .. } => format!("{id}={default} (float)"),
                            ParamInfo::Bool  { id, default, .. } => format!("{id}={} (bool)", *default as u8),
                            ParamInfo::Time  { id, default, .. } => format!("{id}={default} (time)"),
                            ParamInfo::Int   { id, default, .. } => format!("{id}={default} (int)"),
                        })
                        .collect();
                    ProcessorListEntry { id: e.id, kind, name: e.name.to_string(), parameters }
                })
                .collect();

            entries.sort_by(|a, b| a.id.cmp(&b.id));

            if json {
                print_json(&entries);
            } else if entries.is_empty() {
                println!("No processors registered.");
            } else {
                print_table(
                    "processors",
                    &["ID", "TYPE", "NAME", "PARAMETERS"],
                    entries
                        .iter()
                        .map(|e| vec![
                            e.id.clone(),
                            e.kind.clone(),
                            e.name.clone(),
                            e.parameters.join(", "),
                        ])
                        .collect(),
                );
            }
        }
    }
}
```

### Task 14: Final verify — full workspace clippy and core tests

```
cargo clippy --all
```

Expected: zero errors, zero warnings about unused imports.

```
cargo test -p musicum-core
```

Expected: all tests pass.

### Task 15: Smoke test the CLI

```
cargo run -p musicum-cli -- processors list
cargo run -p musicum-cli -- processors list --json
```

Expected: table and JSON output identical to before, with all 10 entries (4 structural + 6 plugins) sorted alphabetically.
