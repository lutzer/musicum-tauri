# Audio Plugin Engine Integration — Design Spec

**Date:** 2026-05-27
**Status:** Approved

## Overview

Integrate the existing `audio-plugin-sdk` plugins into the live audio engine so that a clip's full edit chain — structural processors followed by audio plugins — is applied during playback. Plugin parameters can be updated while the clip is playing via a unified UUID-addressed API.

## Goals

- Play a clip through its structural processor chain **and** its audio plugin chain.
- Allow plugin parameters to be adjusted live during playback (takes effect within one decoded chunk, ~85 ms).
- Provide a single `set_edit_param(uuid, param_id, value)` method that works for both structural and plugin edits.
- Keep the existing structural chain rebuild behaviour (seek / engine restart); no mid-playback structural rebuilds.

## Non-goals

- WASM / dynamic loading of plugins — all plugins are statically linked in `musicum-core`.
- Live rebuild of the structural chain while playing.
- Caching pipeline (handled separately).

---

## Data Model — `ProcessorEdit`

A new unified edit struct lives in `musicum-core` and replaces `sidecar::ProcessorEntry`.

```rust
pub struct ProcessorEdit {
    pub uuid:    Uuid,     // stable identity; used to address the edit in the engine
    pub enabled: bool,
    pub kind:    EditKind,
}

pub enum EditKind {
    Structural {
        processor_id: String,           // e.g. "trim", "cut"
        params: HashMap<String, f64>,
    },
    Plugin {
        plugin_id: String,              // e.g. "gain", "reverb"
        params: HashMap<String, f32>,
    },
}
```

`ProcessorEdit` derives `Serialize`/`Deserialize`. The sidecar stores `Vec<ProcessorEdit>` directly; existing `ProcessorEntry` is removed. Migration: reading old sidecars maps the old format onto this struct.

---

## Pipeline Order

Structural-first, always:

```
Source file
  └─▶ [Structural chain: Trim → Cut → …]   (AudioSource chain, built once)
        └─▶ [Plugin chain: Gain → Reverb → …]   (applied per decoded chunk)
              └─▶ ring buffer → audio output
```

The ordering within each group respects the original list order. Plugin edits keep their relative order from `Vec<ProcessorEdit>` (structural items are skipped when building the plugin chain, and vice-versa).

---

## New Primitives — `audio-plugin-sdk`

### `PluginProcessor` trait

Object-safe, `Send`-able runtime interface. The existing `AudioPlugin` trait (which has `Sized` + static methods) cannot be used as `Box<dyn …>`.

```rust
pub trait PluginProcessor: Send {
    fn process(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        sample_rate: f32,
        timestamp_secs: f64,
    );
    fn set_parameter(&mut self, id: &str, value: f32);
    fn get_parameter(&self, id: &str) -> f32;
    fn render_snapshot(&self) -> &[u8];
    /// Clear internal delay lines / reverb tails on seek. Default: no-op.
    fn reset(&mut self) {}
}
```

A blanket impl makes every `T: AudioPlugin + Send` automatically implement `PluginProcessor`.

### `PluginEntry` vtable + `PluginRegistry`

Mirrors `structural_processor_sdk::ProcessorEntry` / `Registry`.

```rust
pub struct PluginEntry {
    pub descriptor: fn() -> &'static PluginDescriptor,
    pub create:     fn() -> Box<dyn PluginProcessor>,
}

impl PluginEntry {
    /// Construct from any `T: AudioPlugin + Send + 'static`.
    pub fn of<T: AudioPlugin + Send + 'static>() -> Self { … }
}

pub type PluginRegistry = HashMap<String, PluginEntry>;
```

---

## `EditRegistry` — `musicum-core`

Created once at app startup; passed into the engine constructor.

```rust
pub struct EditRegistry {
    pub structural: structural_processor_sdk::Registry,
    pub plugins:    audio_plugin_sdk::PluginRegistry,
}

impl EditRegistry {
    /// Registers all built-in structural processors and audio plugins.
    pub fn default() -> Self { … }
}
```

Built-in registrations:

| Kind       | ID               | Crate                  |
|------------|------------------|------------------------|
| Structural | `trim`           | `structural-processors` |
| Structural | `cut`            | `structural-processors` |
| Structural | `slice`          | `structural-processors` |
| Structural | `crop`           | `structural-processors` |
| Plugin     | `gain`           | `audio-plugins/gain`   |
| Plugin     | `reverb`         | `audio-plugins/reverb` |
| Plugin     | `pan`            | `audio-plugins/pan`    |
| Plugin     | `normalize`      | `audio-plugins/normalize` |
| Plugin     | `level-meter`    | `audio-plugins/level-meter` |
| Plugin     | `oscilloscope`   | `audio-plugins/oscilloscope` |

`EditRegistry::default()` replaces the inline `structural_processors::registry()` call that currently lives inside `PlaybackEngine::new`.

---

## Extended `PlaybackEngine`

### Constructor

```rust
pub fn new(
    path: &Path,
    edits: &[ProcessorEdit],
    registry: &EditRegistry,
) -> Result<Self>
```

Replaces the current `new(path, edits: &[StructuralEdit])`.

**Startup sequence:**

1. Collect `Structural` variants in list order → convert to `Vec<StructuralEdit>` → call `build_chain()` as today.
2. Collect `Plugin` variants in list order → for each enabled entry:
   a. Look up `PluginEntry` in `registry.plugins`.
   b. Create instance via `entry.create()`.
   c. Apply stored params: `instance.set_parameter(id, value)` for each param in `EditKind::Plugin.params`.
   d. Wrap in `Arc<Mutex<Box<dyn PluginProcessor>>>`.
   e. Store as `(uuid, enabled, handle)` in `plugin_handles`.
3. Clone the `Arc` handles into the decode thread (same instances, shared ownership).

### Ownership layout

```
PlaybackEngine
  plugin_handles: Vec<(Uuid, AtomicBool, Arc<Mutex<Box<dyn PluginProcessor>>>)>
                                               │
  decode thread also holds clones ─────────────┘
```

`AtomicBool` for `enabled` allows toggling without acquiring the plugin Mutex.

### Unified edit API

```rust
/// Update a parameter by edit UUID.
/// - Plugin UUID  → immediately calls set_parameter on the live instance.
///                  Takes effect on the next decoded chunk (~85 ms).
/// - Structural UUID → updates params in the engine's internal ProcessorEdit
///                     snapshot. No rebuild while playing; takes effect on
///                     the next PlaybackEngine::new call (after pause + restart).
pub fn set_edit_param(&self, uuid: Uuid, param_id: &str, value: f32)

/// Enable or disable an edit by UUID.
/// - Plugin UUID      → AtomicBool flip; skipped in next chunk.
/// - Structural UUID  → stored; takes effect on next engine creation.
pub fn set_edit_enabled(&self, uuid: Uuid, enabled: bool)
```

### Decode loop changes

After `chain.read_at(cursor, CHUNK_SAMPLES)`:

```rust
for (_, enabled, handle) in &plugin_handles {
    if !enabled.load(Ordering::Relaxed) { continue; }
    if let Ok(mut plugin) = handle.lock() {
        plugin.process(&mut samples, channels, sample_rate as f32, cursor_secs);
    }
}
buf.extend(samples);
```

Locking: the main thread only holds the Mutex for the duration of a `set_parameter` call (microseconds). The decode thread holds it for one `process` call per chunk. Contention is negligible.

### Seek behaviour

Unchanged: structural chain is rebuilt via `build_fresh_chain`. Additionally, each plugin handle has `reset()` called to flush delay lines and reverb tails. Parameter values are **not** reset — they survive seek.

```rust
for (_, _, handle) in &plugin_handles {
    if let Ok(mut plugin) = handle.lock() {
        plugin.reset();
    }
}
```

---

## Sidecar migration

`sidecar::ProcessorEntry` (the existing enum) is replaced by `ProcessorEdit`. Existing sidecars on disk use `"type": "structural"` / `"type": "audio-plugin"` tags. A `From<OldProcessorEntry> for ProcessorEdit` conversion handles any sidecars written before this change; new writes use the `ProcessorEdit` format.

---

## File / crate changes summary

| File | Change |
|------|--------|
| `libs/audio-plugin-sdk/src/plugin.rs` | Add `PluginProcessor` trait + blanket impl |
| `libs/audio-plugin-sdk/src/registry.rs` *(new)* | `PluginEntry`, `PluginRegistry` |
| `libs/audio-plugin-sdk/src/lib.rs` | Re-export new types |
| `libs/musicum-core/src/edit.rs` *(new)* | `ProcessorEdit`, `EditKind` |
| `libs/musicum-core/src/audio/registry.rs` *(new)* | `EditRegistry` |
| `libs/musicum-core/src/audio/player.rs` | New constructor, plugin loop, seek reset, `set_edit_param`, `set_edit_enabled` |
| `libs/musicum-core/src/sidecar.rs` | Replace `ProcessorEntry` with `ProcessorEdit`; add migration `From` impl |
| `libs/musicum-core/src/lib.rs` | Re-export `ProcessorEdit`, `EditKind`, `EditRegistry` |
