# CLI Player Processing Pipeline — Design Spec

**Date:** 2026-05-24
**Status:** Approved
**Scope:** Add a two-pass processing pipeline (structural chain + audio-plugin chain) to the CLI player. Covers data-model changes, new audio modules in `musicum-core`, and the updated `play` command.

---

## Background

The CLI player (`apps/cli/src/commands/play.rs`) currently plays raw audio files via a minimal `PlaybackEngine` with no processing. Clips store an ordered `processors` JSON array, but it is never read during playback. This spec describes how to wire up that array into a real pipeline.

---

## Processing Model

Two-pass, applied in this order regardless of array interleaving:

1. **Structural pass** — all enabled `"structural"` entries, in array order, applied offline to the decoded audio buffer. Produces the final sample space the player reads from.
2. **Plugin pass** — all enabled `"audio-plugin"` entries, in array order, applied in real-time inside the CPAL audio callback.

Unknown `processor.id` values are silently skipped in both passes, consistent with the existing `apply_chain` behaviour.

---

## Data Model

### `ProcessorEntry` (replaces the current flat struct in `sidecar.rs`)

```rust
pub struct ProcessorEntry {
    #[serde(rename = "type")]
    pub kind: String,        // "structural" | "audio-plugin"
    pub id: String,          // UUID — stable instance identifier
    pub enabled: bool,
    pub processor: ProcessorDef,
}

pub struct ProcessorDef {
    pub id: String,          // matches SDK descriptor id: "trim", "gain", etc.
    pub params: serde_json::Value,
}
```

JSON shape:

```json
[
  {
    "type": "structural",
    "id": "a1b2c3d4-0000-0000-0000-000000000001",
    "enabled": true,
    "processor": { "id": "trim", "params": { "start_ms": 200, "end_ms": 0 } }
  },
  {
    "type": "audio-plugin",
    "id": "e5f6a7b8-0000-0000-0000-000000000002",
    "enabled": true,
    "processor": { "id": "gain", "params": { "gain": 0.8 } }
  }
]
```

- `ProcessorEntry.id` is the per-instance UUID (used for `SetParam` addressing and UI correlation).
- `ProcessorDef.id` is the processor kind, matching the SDK's `ProcessorDescriptor.id` / `PluginDescriptor.id`.

All callers that currently construct or deserialise the flat `ProcessorEntry` must be updated: `clip_service`, `preset_service`, and sidecar read/write helpers.

---

## Module Structure

`libs/musicum-core/src/audio/` becomes:

```
audio/
├── mod.rs              # pub use PlaybackEngine; re-exports
├── decoder.rs          # decode_file(path) → (Vec<f32>, AudioInfo)
├── structural_chain.rs # StructuralChain — cursor over structurally-processed audio
├── plugin_chain.rs     # PluginChain + make_plugin() registry
└── engine.rs           # PlaybackEngine — rtrb ring buffer, decode thread, cpal stream
```

The existing `player.rs` is replaced by `engine.rs`. `decoder.rs` is shared between the playback engine and the future caching pipeline.

`musicum-core/Cargo.toml` gains direct dependencies on all six plugin crates (already listed in the greenfield spec):

```toml
plugin-gain        = { path = "../audio-plugins/gain" }
plugin-reverb      = { path = "../audio-plugins/reverb" }
plugin-pan         = { path = "../audio-plugins/pan" }
plugin-normalize   = { path = "../audio-plugins/normalize" }
plugin-level-meter = { path = "../audio-plugins/level-meter" }
plugin-oscilloscope = { path = "../audio-plugins/oscilloscope" }
```

---

## `decoder.rs`

```rust
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub total_frames: u64,
}

pub fn decode_file(path: &Path) -> Result<(Vec<f32>, AudioInfo)>
```

Decodes any Symphonia-supported format to an interleaved `f32` buffer. Used by `PlaybackEngine::new` and the future `cache_clip` pipeline.

---

## `structural_chain.rs`

`StructuralChain` owns the structurally-processed audio buffer and exposes a sequential read cursor.

**Construction** (`StructuralChain::new(samples, sample_rate, channels, entries)`):
1. Filters `entries` to enabled `"structural"` items in array order.
2. Converts each entry's `processor.params` to the `HashMap<String, f64>` format the SDK expects.
3. Calls `structural_processor_sdk::chain::apply_chain` → produces a new `Vec<f32>`.
4. Stores result and initialises frame cursor at 0.

**Interface:**

```rust
pub fn read_frames(&mut self, n: usize) -> &[f32]
pub fn seek_to_secs(&mut self, secs: f64)
pub fn total_frames(&self) -> usize
pub fn is_exhausted(&self) -> bool
pub fn sample_rate(&self) -> u32
pub fn channels(&self) -> u16
```

`read_frames` returns up to `n` frames; may return fewer at end-of-stream. Cursor advances by the number of frames returned. `seek_to_secs` clamps to `[0, duration]`.

---

## `plugin_chain.rs`

### Registry

```rust
pub fn make_plugin(id: &str) -> Option<Box<dyn AudioPlugin>>
```

Static match over all known plugin ids. Returns `None` for unknown ids (silently skipped by `PluginChain::new`).

```
"gain"        → GainPlugin::new()
"reverb"      → ReverbPlugin::new()
"pan"         → PanPlugin::new()
"normalize"   → NormalizePlugin::new()
"level-meter" → LevelMeterPlugin::new()
"oscilloscope"→ OscilloscopePlugin::new()
```

### `PluginChain`

**Construction** (`PluginChain::new(entries, channels, sample_rate)`):
1. Filters `entries` to enabled `"audio-plugin"` items in array order.
2. Calls `make_plugin(processor.id)` for each; skips `None` results.
3. Applies `processor.params` to each instance via `set_parameter`.
4. Stores each instance paired with its outer entry `id` (UUID).

**Interface:**

```rust
pub fn process_buffer(
    &mut self,
    samples: &mut [f32],
    channels: usize,
    sample_rate: f32,
    timestamp_secs: f64,
)

pub fn set_param(&mut self, instance_id: &str, param_id: &str, value: f32)
```

`process_buffer` calls each plugin's `AudioPlugin::process()` in order, mutating the buffer in-place. It is called from the CPAL audio callback and must be allocation-free and lock-free. `set_param` finds the instance by UUID and calls `set_parameter`.

---

## `engine.rs` — `PlaybackEngine`

### Public API (unchanged from current `player.rs`)

```rust
pub fn new(path: &Path, processors: Vec<ProcessorEntry>) -> Result<Self>
pub fn play(&self)
pub fn pause(&self)
pub fn toggle_pause(&self)
pub fn seek(&self, secs: f64)
pub fn position_secs(&self) -> f64
pub fn duration_secs(&self) -> f64
pub fn is_paused(&self) -> bool
pub fn is_finished(&self) -> bool
pub fn title(&self) -> &str
```

### Internal Design

**Shared state** (all behind `Arc`):
- `AtomicU64` position (f64 bits via `f64::to_bits` / `f64::from_bits`)
- `AtomicBool` paused, finished
- `Mutex<Option<f64>>` seek request (same pattern as current player)

**Ring buffers** (rtrb):
- Sample ring buffer: `Producer<f32>` owned by decode thread → `Consumer<f32>` owned by CPAL callback. Capacity: ~2 seconds of stereo audio at 48 kHz (`48_000 * 2 * 2` samples).

**Construction:**
1. `decoder::decode_file(path)` → `(samples, info)`
2. `StructuralChain::new(&samples, info.sample_rate, info.channels, &processors)`
3. `PluginChain::new(&processors, info.channels as usize, info.sample_rate as f32)`
4. Open CPAL stream with `info.sample_rate` and `info.channels`
5. Spawn decode thread

**Decode thread** (owns `StructuralChain`, sample ring `Producer`, and `Mutex<Option<f64>>` seek):
- Checks for pending seek: if set, calls `structural_chain.seek_to_secs`, clears the shared position atomic, clears the ring buffer via a drain loop, resets the flag.
- If paused: sleeps 10 ms, loops.
- If ring buffer is full (back-pressure): sleeps 5 ms, loops.
- Calls `structural_chain.read_frames(CHUNK)`, pushes returned samples to the ring buffer.
- When `is_exhausted()`: sets `finished` atomic, exits.

**CPAL callback** (owns `PluginChain`, sample ring `Consumer`):
- Drains available samples from ring buffer into output buffer.
- Calls `plugin_chain.process_buffer(output, channels, sample_rate, timestamp_secs)`.
- Silences any unfilled output frames if ring buffer is empty.
- Updates position atomic: `position.store(f64::to_bits(new_pos), Relaxed)`.

---

## CLI `play` Command

`resolve_path` is replaced by `resolve_target` which returns both the file path and the processor list:

```rust
async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<ProcessorEntry>)>
```

- File slug or literal path → empty `Vec<ProcessorEntry>`
- Clip slug → deserialise `clip.processors` JSON column → `Vec<ProcessorEntry>`

`run` passes both to `PlaybackEngine::new(path, processors)`. The TUI loop (`run_tui`) is unchanged.

---

## Out of Scope

- `SetParam` command (real-time plugin param changes during playback) — not needed for CLI, added when the Tauri desktop engine is built.
- Virtual-cursor StructuralChain (on-the-fly source seeking without pre-decode) — deferred; the pre-decode approach is sufficient for CLI.
- Caching pipeline (`cache.rs`) — separate spec.
- Waveform generation — separate spec.
