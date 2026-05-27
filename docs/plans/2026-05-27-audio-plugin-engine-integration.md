# Audio Plugin Engine Integration Implementation Plan

**Goal:** Wire `audio-plugin-sdk` plugins into the live playback engine so a clip's full edit chain (structural processors + audio plugins) plays back, with live parameter updates via a unified UUID API.

**Architecture:** Add a `PluginProcessor` object-safe trait + registry to `audio-plugin-sdk`, introduce a unified `ProcessorEdit`/`EditKind` data model in `musicum-core`, and extend `PlaybackEngine` to run a plugin chain after the structural chain on every decoded chunk. Plugin handles (`Arc<PluginHandle>`) are shared between the main thread (for parameter updates) and the decode thread (for processing).

**Tech Stack:** Rust, `audio-plugin-sdk`, `structural-processor-sdk`, `musicum-core`, `uuid`, `serde`/`serde_json`

---

## File Map

| Path | Status | Responsibility |
|------|--------|----------------|
| `libs/audio-plugin-sdk/src/plugin.rs` | Modify | Add `PluginProcessor` trait + blanket impl |
| `libs/audio-plugin-sdk/src/registry.rs` | **Create** | `PluginEntry` vtable + `PluginRegistry` type alias |
| `libs/audio-plugin-sdk/src/lib.rs` | Modify | Re-export `PluginProcessor`, `PluginEntry`, `PluginRegistry` |
| `libs/musicum-core/src/edit.rs` | **Create** | `ProcessorEdit`, `EditKind`, serde migration helper |
| `libs/musicum-core/src/audio/registry.rs` | **Create** | `EditRegistry` + `EditRegistry::default()` |
| `libs/musicum-core/src/audio/mod.rs` | Modify | Add `structural_edits_from`; remove `sidecar_entries_to_edits` |
| `libs/musicum-core/src/audio/player.rs` | Modify | New constructor, plugin loop, seek reset, `set_edit_param`, `set_edit_enabled` |
| `libs/musicum-core/src/sidecar.rs` | Modify | `ClipSidecar.processors → Vec<ProcessorEdit>`; add migration fallback |
| `libs/musicum-core/src/services/clip_service.rs` | Modify | Use `ProcessorEdit` throughout |
| `libs/musicum-core/src/services/preset_service.rs` | Modify | Use `ProcessorEdit` throughout |
| `libs/musicum-core/src/lib.rs` | Modify | Re-export `ProcessorEdit`, `EditKind`, `EditRegistry` |
| `apps/cli/src/commands/play.rs` | Modify | Use `ProcessorEdit`; new `PlaybackEngine::new` signature |
| `apps/cli/src/commands/export.rs` | Modify | Derive structural edits from `ProcessorEdit` slice |

---

## Tasks

### Task 1: Add `PluginProcessor` trait to `audio-plugin-sdk`

`PluginProcessor` is object-safe (`Send`-able) so it can be boxed as `Box<dyn PluginProcessor>`. The existing `AudioPlugin` trait has `Sized` bounds (via static methods) and cannot be used that way. A blanket impl bridges the two.

**Files:**
- Modify: `libs/audio-plugin-sdk/src/plugin.rs`

#### Step 1.1 — Write the failing test

Append to `libs/audio-plugin-sdk/src/tests.rs` (or add inline at bottom of `plugin.rs`):

```rust
#[cfg(test)]
mod plugin_processor_tests {
    use super::*;
    use crate::plugin::PluginProcessor;

    struct DummyPlugin { gain: f32 }
    impl AudioPlugin for DummyPlugin {
        fn descriptor() -> &'static crate::parameters::PluginDescriptor {
            use crate::parameters::{PluginDescriptor, PluginMode};
            static D: PluginDescriptor = PluginDescriptor {
                id: "dummy", name: "Dummy", version: "0",
                mode: PluginMode::Realtime, parameters: &[],
            };
            &D
        }
        fn new() -> Self { DummyPlugin { gain: 1.0 } }
        fn set_parameter(&mut self, id: &str, v: f32) { if id == "gain" { self.gain = v; } }
        fn get_parameter(&self, id: &str) -> f32 { if id == "gain" { self.gain } else { 0.0 } }
    }

    #[test]
    fn blanket_impl_set_get() {
        // DummyPlugin: AudioPlugin + Send → implements PluginProcessor via blanket
        let mut p: Box<dyn PluginProcessor> = Box::new(DummyPlugin::new());
        p.set_parameter("gain", 2.0);
        assert_eq!(p.get_parameter("gain"), 2.0);
    }

    #[test]
    fn blanket_impl_process_delegates() {
        let mut p: Box<dyn PluginProcessor> = Box::new(DummyPlugin::new());
        p.set_parameter("gain", 0.0);
        let mut buf = vec![1.0_f32; 4];
        // DummyPlugin.process is a no-op (default), so buf unchanged
        p.process(&mut buf, 1, 44100.0, 0.0);
        assert!(buf.iter().all(|&s| s == 1.0));
    }

    #[test]
    fn reset_is_callable() {
        let mut p: Box<dyn PluginProcessor> = Box::new(DummyPlugin::new());
        p.reset(); // default no-op; must not panic
    }
}
```

#### Step 1.2 — Run the test (expect compile error: `PluginProcessor` not found)

```
cargo test -p audio-plugin-sdk 2>&1 | head -20
```

Expected: error `cannot find trait PluginProcessor`.

#### Step 1.3 — Add `PluginProcessor` trait + blanket impl to `plugin.rs`

Append **below** the existing `AudioPlugin` trait definition and **above** the `implement_plugin!` macro:

```rust
/// Object-safe, `Send`-able runtime interface for audio plugins.
///
/// Used by `PlaybackEngine` to process audio through a plugin chain at runtime.
/// Every concrete type that implements `AudioPlugin + Send` automatically
/// implements this trait via the blanket impl below.
pub trait PluginProcessor: Send {
    /// Process `samples` in-place (interleaved f32, `channels` channels, `sample_rate` Hz).
    /// `timestamp_secs` is the track-relative position of the first sample.
    fn process(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        sample_rate: f32,
        timestamp_secs: f64,
    );

    /// Set a parameter by string ID. Unknown IDs are silently ignored.
    fn set_parameter(&mut self, id: &str, value: f32);

    /// Get a parameter by string ID. Returns `0.0` for unknown IDs.
    fn get_parameter(&self, id: &str) -> f32;

    /// Return the current render snapshot as raw bytes.
    fn render_snapshot(&self) -> &[u8];

    /// Clear internal state (delay lines, reverb tails) on seek. Default: no-op.
    fn reset(&mut self) {}
}

/// Every `T: AudioPlugin + Send` automatically implements `PluginProcessor`.
impl<T: AudioPlugin + Send> PluginProcessor for T {
    fn process(&mut self, samples: &mut [f32], channels: usize, sample_rate: f32, ts: f64) {
        AudioPlugin::process(self, samples, channels, sample_rate, ts);
    }
    fn set_parameter(&mut self, id: &str, value: f32) {
        AudioPlugin::set_parameter(self, id, value);
    }
    fn get_parameter(&self, id: &str) -> f32 {
        AudioPlugin::get_parameter(self, id)
    }
    fn render_snapshot(&self) -> &[u8] {
        AudioPlugin::render_snapshot(self)
    }
}
```

#### Step 1.4 — Run tests

```
cargo test -p audio-plugin-sdk
```

Expected: all tests pass, including the new `plugin_processor_tests`.

---

### Task 2: Add `PluginEntry` + `PluginRegistry` to `audio-plugin-sdk`

Mirrors `structural_processor_sdk::ProcessorEntry` / `Registry`. `PluginEntry::of<T>()` is the ergonomic constructor.

**Files:**
- Create: `libs/audio-plugin-sdk/src/registry.rs`

#### Step 2.1 — Write the failing test

Add at bottom of `libs/audio-plugin-sdk/src/registry.rs` (create the file with test module first):

```rust
use std::collections::HashMap;
use crate::parameters::PluginDescriptor;
use crate::plugin::PluginProcessor;

/// Vtable entry for one audio plugin. Static fn pointers allow instantiation
/// and descriptor queries without a concrete type in scope.
pub struct PluginEntry {
    pub descriptor: fn() -> &'static PluginDescriptor,
    pub create:     fn() -> Box<dyn PluginProcessor>,
}

impl PluginEntry {
    /// Build a `PluginEntry` from any `T: AudioPlugin + Send + 'static`.
    pub fn of<T: crate::plugin::AudioPlugin + Send + 'static>() -> Self {
        Self {
            descriptor: T::descriptor,
            create:     || Box::new(T::new()),
        }
    }
}

/// Registry of all known audio plugins. Built once at startup.
pub type PluginRegistry = HashMap<String, PluginEntry>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioPlugin, FloatParam, PluginDescriptor, PluginMode, PluginParameter};

    struct TinyPlugin;
    static TINY_PARAMS: [PluginParameter; 0] = [];
    static TINY_DESC: PluginDescriptor = PluginDescriptor {
        id: "tiny", name: "Tiny", version: "0",
        mode: PluginMode::Realtime, parameters: &TINY_PARAMS,
    };
    impl AudioPlugin for TinyPlugin {
        fn descriptor() -> &'static PluginDescriptor { &TINY_DESC }
        fn new() -> Self { TinyPlugin }
        fn set_parameter(&mut self, _: &str, _: f32) {}
        fn get_parameter(&self, _: &str) -> f32 { 0.0 }
    }

    #[test]
    fn plugin_entry_of_creates_instance() {
        let entry = PluginEntry::of::<TinyPlugin>();
        let mut instance = (entry.create)();
        instance.set_parameter("x", 1.0); // must not panic
        assert_eq!((entry.descriptor)().id, "tiny");
    }

    #[test]
    fn plugin_registry_lookup() {
        let mut reg: PluginRegistry = HashMap::new();
        reg.insert("tiny".to_string(), PluginEntry::of::<TinyPlugin>());
        assert!(reg.contains_key("tiny"));
        let e = &reg["tiny"];
        assert_eq!((e.descriptor)().id, "tiny");
    }
}
```

#### Step 2.2 — Run the test (expect compile error: module not declared)

```
cargo test -p audio-plugin-sdk 2>&1 | head -10
```

Expected: `file not found for module 'registry'` or similar.

#### Step 2.3 — Declare the module in `lib.rs`

The code above in Step 2.1 IS the production code. Just create the file — the module declaration in lib.rs comes in the next task.

#### Step 2.4 — Run tests

```
cargo test -p audio-plugin-sdk
```

Expected: compile error because `registry` not in `lib.rs` yet. (We'll fix in Task 3.)

---

### Task 3: Update `audio-plugin-sdk/src/lib.rs`

**Files:**
- Modify: `libs/audio-plugin-sdk/src/lib.rs`

#### Step 3.1 — Add the registry module and re-exports

Replace the entire file with:

```rust
mod analyzer;
mod parameters;
mod plugin;
pub mod registry;

pub use analyzer::AudioAnalyzer;
pub use hound;
pub use parameters::{
    BoolParam, FloatParam, ParamMap, ParamResult,
    PluginDescriptor, PluginMode, PluginParameter,
};
pub use plugin::{AudioPlugin, PluginProcessor};
pub use registry::{PluginEntry, PluginRegistry};

#[cfg(test)]
mod tests;
```

#### Step 3.2 — Run tests

```
cargo test -p audio-plugin-sdk
```

Expected: all tests pass (including the new registry tests).

---

### Task 4: Add `ProcessorEdit` + `EditKind` to `musicum-core`

Unified data model. Lives in its own `edit.rs` module. Replaces the split between `ProcessorEntry` (sidecar) and `StructuralEdit` (engine input). Includes a migration helper that accepts both old and new JSON formats.

**Files:**
- Create: `libs/musicum-core/src/edit.rs`

#### Step 4.1 — Write the failing test

Create `libs/musicum-core/src/edit.rs`:

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unified edit descriptor for both structural processors and audio plugins.
/// Stored in `ClipSidecar.processors` and passed to `PlaybackEngine`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorEdit {
    pub uuid:    Uuid,
    pub enabled: bool,
    pub kind:    EditKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EditKind {
    Structural {
        processor_id: String,
        #[serde(default)]
        params: HashMap<String, f64>,
    },
    Plugin {
        plugin_id: String,
        #[serde(default)]
        params: HashMap<String, f32>,
    },
}

/// Deserialize `Vec<ProcessorEdit>` from a JSON string, falling back to the
/// legacy `ProcessorEntry` format if the new format fails to parse.
///
/// Old format (written before this change):
/// `[{"type":"structural","id":"<uuid>","enabled":true,"processor":{"id":"trim","params":{...}}}]`
///
/// New format:
/// `[{"uuid":"<uuid>","enabled":true,"kind":{"type":"structural","processor_id":"trim","params":{...}}}]`
pub fn deserialize_processor_edits(json: &str) -> Vec<ProcessorEdit> {
    // Try new format
    if let Ok(edits) = serde_json::from_str::<Vec<ProcessorEdit>>(json) {
        return edits;
    }
    // Fall back to old ProcessorEntry format, then convert
    #[derive(Deserialize)]
    struct OldProcessorRef { id: String, params: serde_json::Value }
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "kebab-case")]
    enum OldEntry {
        Structural { id: String, enabled: bool, processor: OldProcessorRef },
        #[serde(rename = "audio-plugin")]
        AudioPlugin { id: String, enabled: bool, processor: OldProcessorRef },
    }

    fn json_to_f64_map(v: &serde_json::Value) -> HashMap<String, f64> {
        v.as_object()
            .map(|o| o.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect())
            .unwrap_or_default()
    }

    serde_json::from_str::<Vec<OldEntry>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|old| match old {
            OldEntry::Structural { id, enabled, processor } => ProcessorEdit {
                uuid: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                enabled,
                kind: EditKind::Structural {
                    processor_id: processor.id,
                    params: json_to_f64_map(&processor.params),
                },
            },
            OldEntry::AudioPlugin { id, enabled, processor } => ProcessorEdit {
                uuid: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                enabled,
                kind: EditKind::Plugin {
                    plugin_id: processor.id,
                    params: json_to_f64_map(&processor.params)
                        .into_iter()
                        .map(|(k, v)| (k, v as f32))
                        .collect(),
                },
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_structural() -> ProcessorEdit {
        let mut params = HashMap::new();
        params.insert("start".to_string(), 1.0_f64);
        ProcessorEdit {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            enabled: true,
            kind: EditKind::Structural { processor_id: "trim".to_string(), params },
        }
    }

    fn make_plugin() -> ProcessorEdit {
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 0.5_f32);
        ProcessorEdit {
            uuid: Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap(),
            enabled: false,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }
    }

    #[test]
    fn roundtrip_structural() {
        let edit = make_structural();
        let json = serde_json::to_string(&edit).unwrap();
        let back: ProcessorEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(edit, back);
    }

    #[test]
    fn roundtrip_plugin() {
        let edit = make_plugin();
        let json = serde_json::to_string(&edit).unwrap();
        let back: ProcessorEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(edit, back);
    }

    #[test]
    fn deserialize_new_format_vec() {
        let edits = vec![make_structural(), make_plugin()];
        let json = serde_json::to_string(&edits).unwrap();
        let result = deserialize_processor_edits(&json);
        assert_eq!(result, edits);
    }

    #[test]
    fn deserialize_old_structural_format() {
        // Old sidecar JSON (written by previous code)
        let old_json = r#"[
            {
                "type": "structural",
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "enabled": true,
                "processor": {"id": "trim", "params": {"start": 1.0}}
            }
        ]"#;
        let result = deserialize_processor_edits(old_json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(result[0].enabled, true);
        match &result[0].kind {
            EditKind::Structural { processor_id, params } => {
                assert_eq!(processor_id, "trim");
                assert_eq!(params["start"], 1.0);
            }
            _ => panic!("expected Structural"),
        }
    }

    #[test]
    fn deserialize_old_audio_plugin_format() {
        let old_json = r#"[
            {
                "type": "audio-plugin",
                "id": "660e8400-e29b-41d4-a716-446655440001",
                "enabled": false,
                "processor": {"id": "gain", "params": {"gain": 0.5}}
            }
        ]"#;
        let result = deserialize_processor_edits(old_json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].enabled, false);
        match &result[0].kind {
            EditKind::Plugin { plugin_id, params } => {
                assert_eq!(plugin_id, "gain");
                assert!((params["gain"] - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected Plugin"),
        }
    }

    #[test]
    fn deserialize_empty_json() {
        assert_eq!(deserialize_processor_edits("[]"), vec![]);
    }

    #[test]
    fn deserialize_garbage_returns_empty() {
        assert_eq!(deserialize_processor_edits("not json"), vec![]);
    }
}
```

#### Step 4.2 — Run the test (expect compile error: `edit` not in lib)

```
cargo test -p musicum-core 2>&1 | head -10
```

Expected: `file not found for module 'edit'`.

#### Step 4.3 — Declare the module in `musicum-core/src/lib.rs`

Add `pub mod edit;` to the top of `libs/musicum-core/src/lib.rs`:

```rust
pub mod audio;
pub mod config;
pub mod db;
pub mod edit;        // ← add this
pub mod error;
pub mod services;
pub mod sidecar;

pub use error::ServiceError;
```

#### Step 4.4 — Run tests

```
cargo test -p musicum-core -- edit
```

Expected: all 7 edit tests pass.

---

### Task 5: Add `EditRegistry` to `musicum-core`

Central registry, created once at app startup, passed to `PlaybackEngine::new`. `EditRegistry::default()` registers all built-in processors and plugins.

**Files:**
- Create: `libs/musicum-core/src/audio/registry.rs`

#### Step 5.1 — Write the failing test

Create `libs/musicum-core/src/audio/registry.rs`:

```rust
use audio_plugin_sdk::PluginRegistry;
use structural_processor_sdk::Registry as StructuralRegistry;

/// Combined registry of all known structural processors and audio plugins.
/// Pass an instance to `PlaybackEngine::new`.
pub struct EditRegistry {
    pub structural: StructuralRegistry,
    pub plugins:    PluginRegistry,
}

impl Default for EditRegistry {
    /// Registers all built-in structural processors and audio plugins.
    fn default() -> Self {
        use audio_plugin_sdk::PluginEntry;

        let mut structural = structural_processors::registry();

        let mut plugins = PluginRegistry::new();
        plugins.insert("gain".into(),        PluginEntry::of::<gain::GainPlugin>());
        plugins.insert("reverb".into(),      PluginEntry::of::<reverb::ReverbPlugin>());
        plugins.insert("pan".into(),         PluginEntry::of::<pan::PanPlugin>());
        plugins.insert("normalize".into(),   PluginEntry::of::<normalize::NormalizePlugin>());
        plugins.insert("level-meter".into(), PluginEntry::of::<level_meter::LevelMeterPlugin>());
        plugins.insert("oscilloscope".into(),PluginEntry::of::<oscilloscope::OscilloscopePlugin>());

        Self { structural, plugins }
    }
}

// Suppress unused import warning in structural registry re-init
fn _use_structural(r: &mut StructuralRegistry) { let _ = r; }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_all_structural_processors() {
        let reg = EditRegistry::default();
        for id in ["trim", "cut", "slice", "crop"] {
            assert!(reg.structural.contains_key(id), "missing structural '{id}'");
        }
    }

    #[test]
    fn default_registry_has_all_plugins() {
        let reg = EditRegistry::default();
        for id in ["gain", "reverb", "pan", "normalize", "level-meter", "oscilloscope"] {
            assert!(reg.plugins.contains_key(id), "missing plugin '{id}'");
        }
    }

    #[test]
    fn plugin_entry_create_works() {
        let reg = EditRegistry::default();
        let entry = &reg.plugins["gain"];
        let instance = (entry.create)();
        // GainPlugin default gain=1.0
        assert_eq!(instance.get_parameter("gain"), 1.0);
    }
}
```

> **Note on plugin struct names:** Verify the public struct names by checking each plugin crate's `lib.rs`. For example, `gain::GainPlugin`, `reverb::ReverbPlugin`, `pan::PanPlugin`, `normalize::NormalizePlugin`, `level_meter::LevelMeterPlugin`, `oscilloscope::OscilloscopePlugin`. Adjust if the actual names differ (they follow the same pattern based on `gain/src/lib.rs`).

#### Step 5.2 — Run the test (expect compile error)

```
cargo test -p musicum-core 2>&1 | head -20
```

Expected: `file not found for module 'registry'` or undeclared struct names.

#### Step 5.3 — Declare the module in `audio/mod.rs`

Add `pub mod registry;` to `libs/musicum-core/src/audio/mod.rs`.

#### Step 5.4 — Check actual plugin struct names

```bash
grep "pub struct" libs/audio-plugins/*/src/lib.rs
```

Update the `PluginEntry::of` calls in `registry.rs` to match the actual struct names.

#### Step 5.5 — Run tests

```
cargo test -p musicum-core -- registry
```

Expected: all 3 registry tests pass.

---

### Task 6: Migrate `sidecar.rs` — replace `ProcessorEntry` in `ClipSidecar`

`ClipSidecar.processors` changes from `Vec<ProcessorEntry>` to `Vec<ProcessorEdit>`.
`ProcessorEntry` is kept as a private legacy type for migration only (not re-exported).

**Files:**
- Modify: `libs/musicum-core/src/sidecar.rs`

#### Step 6.1 — Write the failing test

Add to the bottom of `sidecar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::edit::{EditKind, ProcessorEdit};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn write_read_sidecar_with_processor_edits() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();

        let mut params = HashMap::new();
        params.insert("start".to_string(), 1.5_f64);
        let edit = ProcessorEdit {
            uuid: Uuid::new_v4(),
            enabled: true,
            kind: EditKind::Structural { processor_id: "trim".to_string(), params },
        };

        let sc = FileSidecar {
            version: 1,
            metadata: FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![ClipSidecar {
                slug: "c".to_string(),
                title: "C".to_string(),
                notes: String::new(),
                processors: vec![edit.clone()],
            }],
        };

        write_file_sidecar(&audio, &sc).unwrap();
        let loaded = read_file_sidecar(&audio).unwrap();
        assert_eq!(loaded.clips[0].processors[0], edit);
    }

    #[test]
    fn read_sidecar_with_old_processor_entry_format() {
        // Simulate a sidecar file written before this change
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        let old_json = r#"{
            "version": 1,
            "metadata": {},
            "clips": [{
                "slug": "c", "title": "C", "notes": "",
                "processors": [{
                    "type": "structural",
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "enabled": true,
                    "processor": {"id": "trim", "params": {"start": 2.0}}
                }]
            }]
        }"#;
        let sidecar_path = audio.with_file_name("test.wav.musicum.json");
        std::fs::write(&sidecar_path, old_json).unwrap();

        let sc = read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.clips.len(), 1);
        assert_eq!(sc.clips[0].processors.len(), 1);
        let edit = &sc.clips[0].processors[0];
        assert_eq!(edit.uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
        match &edit.kind {
            EditKind::Structural { processor_id, params } => {
                assert_eq!(processor_id, "trim");
                assert_eq!(params["start"], 2.0);
            }
            _ => panic!("expected Structural"),
        }
    }
}
```

#### Step 6.2 — Run the test (expect compile error)

```
cargo test -p musicum-core -- sidecar 2>&1 | head -20
```

Expected: type mismatch — `ClipSidecar.processors` is still `Vec<ProcessorEntry>`.

#### Step 6.3 — Update `sidecar.rs`

Change `ClipSidecar.processors` type and add a migration deserializer.

In `sidecar.rs`, add the import:
```rust
use crate::edit::{deserialize_processor_edits, ProcessorEdit};
```

Change `ClipSidecar` to:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSidecar {
    pub slug:  String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// Processor and plugin edits for this clip.
    /// Deserialized with migration support for old `ProcessorEntry` format.
    #[serde(default, deserialize_with = "deserialize_clip_processors")]
    pub processors: Vec<ProcessorEdit>,
}

fn deserialize_clip_processors<'de, D>(d: D) -> Result<Vec<ProcessorEdit>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Capture raw JSON array value, then run migration-aware helper
    let raw = serde_json::Value::deserialize(d)?;
    let json_str = raw.to_string();
    Ok(deserialize_processor_edits(&json_str))
}
```

Remove (or keep private) the old `ProcessorEntry` and `ProcessorRef` types. If any service code still refers to `ProcessorEntry`, it will fail to compile — those sites are fixed in Tasks 12–13.

> **Keep** `ProcessorEntry` + `ProcessorRef` in the file for now, but **do not** `pub use` them from `lib.rs`. We'll remove remaining usages in subsequent tasks.

#### Step 6.4 — Run tests

```
cargo test -p musicum-core -- sidecar
```

Expected: both sidecar tests pass.

---

### Task 7: Update `musicum-core/src/audio/mod.rs`

Replace `sidecar_entries_to_edits` (which converted `ProcessorEntry` → `StructuralEdit`) with `structural_edits_from` (which converts `ProcessorEdit` → `StructuralEdit`). This function is still needed by `export_service`.

**Files:**
- Modify: `libs/musicum-core/src/audio/mod.rs`

#### Step 7.1 — Write the failing test

Add to `audio/mod.rs` inline tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{EditKind, ProcessorEdit};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn structural_edits_from_filters_plugins() {
        let edits = vec![
            ProcessorEdit {
                uuid: Uuid::new_v4(), enabled: true,
                kind: EditKind::Structural {
                    processor_id: "trim".to_string(),
                    params: [("start".to_string(), 1.0)].into(),
                },
            },
            ProcessorEdit {
                uuid: Uuid::new_v4(), enabled: true,
                kind: EditKind::Plugin {
                    plugin_id: "gain".to_string(),
                    params: HashMap::new(),
                },
            },
        ];
        let structural = structural_edits_from(&edits);
        assert_eq!(structural.len(), 1);
        assert_eq!(structural[0].processor_id, "trim");
    }

    #[test]
    fn structural_edits_from_preserves_enabled_flag() {
        let edit = ProcessorEdit {
            uuid: Uuid::new_v4(), enabled: false,
            kind: EditKind::Structural {
                processor_id: "cut".to_string(),
                params: HashMap::new(),
            },
        };
        let structural = structural_edits_from(&[edit]);
        assert_eq!(structural[0].enabled, false);
    }
}
```

#### Step 7.2 — Run the test (expect compile errors)

```
cargo test -p musicum-core -- audio::tests 2>&1 | head -20
```

Expected: `structural_edits_from` not found.

#### Step 7.3 — Replace `audio/mod.rs`

```rust
pub mod player;
pub mod registry;
pub mod source;

use crate::edit::{EditKind, ProcessorEdit};
use structural_processor_sdk::chain::StructuralEdit;

pub use player::PlaybackEngine;
pub use registry::EditRegistry;
pub use source::FileAudioSource;

/// Extract structural edits from a `ProcessorEdit` slice.
/// Plugin edits are silently ignored. Used by `export_service`.
pub fn structural_edits_from(edits: &[ProcessorEdit]) -> Vec<StructuralEdit> {
    edits
        .iter()
        .filter_map(|e| {
            if let EditKind::Structural { processor_id, params } = &e.kind {
                Some(StructuralEdit {
                    processor_id: processor_id.clone(),
                    enabled: e.enabled,
                    params: params.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // ... (tests from step 7.1)
}
```

#### Step 7.4 — Run tests

```
cargo test -p musicum-core -- audio
```

Expected: both audio tests pass.

---

### Task 8: Update `PlaybackEngine` — new constructor + plugin pipeline

This is the core of the feature. `PlaybackEngine::new` now accepts `&[ProcessorEdit]` and `&EditRegistry`. Plugins are instantiated, params applied, and handles shared with the decode thread. The decode loop applies the plugin chain after the structural chain.

**Files:**
- Modify: `libs/musicum-core/src/audio/player.rs`

#### Step 8.1 — Write the failing test

Add to `player.rs` tests (requires a WAV file fixture — reuse the helper from `source.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::registry::EditRegistry;
    use crate::edit::{EditKind, ProcessorEdit};
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::collections::HashMap;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

    fn write_temp_wav(frames: usize, sample_rate: u32) -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let spec = WavSpec { channels: 1, sample_rate, bits_per_sample: 32, sample_format: SampleFormat::Float };
        let mut w = WavWriter::create(tmp.path(), spec).unwrap();
        for i in 0..frames { w.write_sample(i as f32 / frames as f32).unwrap(); }
        w.finalize().unwrap();
        tmp
    }

    #[test]
    fn new_with_no_edits_creates_engine() {
        let tmp = write_temp_wav(4410, 44_100);
        let reg = EditRegistry::default();
        let engine = PlaybackEngine::new(tmp.path(), &[], &reg);
        assert!(engine.is_ok());
    }

    #[test]
    fn new_with_plugin_edit_creates_engine() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 0.5_f32);
        let edits = vec![ProcessorEdit {
            uuid: Uuid::new_v4(),
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg);
        assert!(engine.is_ok());
    }

    #[test]
    fn set_edit_param_on_plugin_does_not_panic() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let uuid = Uuid::new_v4();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 1.0_f32);
        let edits = vec![ProcessorEdit {
            uuid,
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg).unwrap();
        // Should not panic; takes effect on next decoded chunk
        engine.set_edit_param(uuid, "gain", 0.5);
    }

    #[test]
    fn set_edit_enabled_on_plugin_does_not_panic() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let uuid = Uuid::new_v4();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 1.0_f32);
        let edits = vec![ProcessorEdit {
            uuid,
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg).unwrap();
        engine.set_edit_enabled(uuid, false);
        engine.set_edit_enabled(uuid, true);
    }
}
```

#### Step 8.2 — Run the test (expect compile errors)

```
cargo test -p musicum-core -- player 2>&1 | head -30
```

Expected: `PlaybackEngine::new` still takes `&[StructuralEdit]`, type mismatch.

#### Step 8.3 — Rewrite `player.rs`

Full replacement. Key changes from the original:

1. `PlaybackEngine::new` signature: `(path, edits: &[ProcessorEdit], registry: &EditRegistry)`
2. New `PluginHandle` struct + `plugin_handles` field on `PlaybackEngine`
3. New `structural_snapshot` field for `set_edit_param` on structural edits
4. `decode_loop` receives `plugin_handles: Vec<Arc<PluginHandle>>`
5. Seek code calls `reset()` on all plugin handles
6. Decode loop applies plugin chain after `chain.read_at`

```rust
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use audio_plugin_sdk::PluginProcessor;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use structural_processor_sdk::{
    chain::{build_chain, chain_output_duration, StructuralEdit},
    AudioSource,
};
use uuid::Uuid;

use crate::audio::registry::EditRegistry;
use crate::audio::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};

// ~2 seconds of stereo audio at 48 kHz
const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
const CHUNK_SAMPLES: usize = 4_096;

// ── Plugin handle ─────────────────────────────────────────────────────────────

/// Shared handle for a single plugin instance.
/// Owned by `PlaybackEngine` and also cloned into the decode thread.
struct PluginHandle {
    uuid:      Uuid,
    enabled:   AtomicBool,
    processor: Mutex<Box<dyn PluginProcessor>>,
}

// ── Playback state ────────────────────────────────────────────────────────────

struct PlaybackState {
    paused:       AtomicBool,
    looping:      AtomicBool,
    finished:     AtomicBool,
    seek_request: Mutex<Option<f64>>,
    position:     AtomicU64,
    total_frames: AtomicU64,
    sample_rate:  u32,
    buffer:       Mutex<VecDeque<f32>>,
}

// ── PlaybackEngine ────────────────────────────────────────────────────────────

pub struct PlaybackEngine {
    state:               Arc<PlaybackState>,
    plugin_handles:      Vec<Arc<PluginHandle>>,
    /// Snapshot of structural edits; updated by `set_edit_param` / `set_edit_enabled`
    /// for structural UUIDs. Changes take effect on the next `PlaybackEngine::new`.
    structural_snapshot: Mutex<Vec<ProcessorEdit>>,
    title:               String,
    _stream:             cpal::Stream,
    _decode_thread:      JoinHandle<()>,
}

impl PlaybackEngine {
    /// Create a new playback engine for `path`, applying `edits` via `registry`.
    ///
    /// `edits` is the full edit list (structural + plugin). Structural edits build
    /// the decode chain once; plugin edits are instantiated and applied live per chunk.
    /// Pass `edits: &[]` for raw file playback.
    pub fn new(path: &Path, edits: &[ProcessorEdit], registry: &EditRegistry) -> Result<Self> {
        let source = FileAudioSource::new(path)?;
        let raw_duration = source.duration_secs();
        let sample_rate  = source.sample_rate();
        let channels     = source.channels();

        // Split edits by kind
        let structural_edits: Vec<StructuralEdit> = edits
            .iter()
            .filter_map(|e| {
                if let EditKind::Structural { processor_id, params } = &e.kind {
                    Some(StructuralEdit {
                        processor_id: processor_id.clone(),
                        enabled: e.enabled,
                        params: params.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let output_duration = chain_output_duration(raw_duration, &structural_edits, &registry.structural);
        let total_frames    = (output_duration * sample_rate as f64) as u64;

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

        // Structural snapshot for set_edit_param
        let structural_snapshot: Vec<ProcessorEdit> = edits
            .iter()
            .filter(|e| matches!(e.kind, EditKind::Structural { .. }))
            .cloned()
            .collect();

        // Audio device setup
        let host   = cpal::default_host();
        let device = host.default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let config = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let state = Arc::new(PlaybackState {
            paused:       AtomicBool::new(true),
            looping:      AtomicBool::new(false),
            finished:     AtomicBool::new(false),
            seek_request: Mutex::new(None),
            position:     AtomicU64::new(0),
            total_frames: AtomicU64::new(total_frames),
            sample_rate,
            buffer:       Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
        });

        let state_cb = Arc::clone(&state);
        let ch = channels as usize;
        let stream = device.build_output_stream(
            &config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if state_cb.paused.load(Ordering::Relaxed) {
                    output.fill(0.0);
                    return;
                }
                if let Ok(mut buf) = state_cb.buffer.try_lock() {
                    let n = output.len().min(buf.len());
                    for (out, s) in output[..n].iter_mut().zip(buf.drain(..n)) {
                        *out = s;
                    }
                    state_cb.position.fetch_add((n / ch.max(1)) as u64, Ordering::Relaxed);
                    output[n..].fill(0.0);
                } else {
                    output.fill(0.0);
                }
            },
            |err| eprintln!("audio error: {err}"),
            None,
        ).context("failed to open audio stream")?;
        stream.play().context("failed to start audio stream")?;

        let state_dec        = Arc::clone(&state);
        let path_owned       = path.to_path_buf();
        let struct_owned     = structural_edits;
        let plugin_handles_dec = plugin_handles.clone(); // Arc clones, cheap

        let decode_thread = thread::spawn(move || {
            decode_loop(path_owned, struct_owned, plugin_handles_dec, state_dec);
        });

        let title = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        Ok(Self {
            state,
            plugin_handles,
            structural_snapshot: Mutex::new(structural_snapshot),
            title,
            _stream: stream,
            _decode_thread: decode_thread,
        })
    }

    // ── Playback controls ─────────────────────────────────────────────────────

    pub fn play(&self)         { self.state.paused.store(false, Ordering::Relaxed); }
    pub fn pause(&self)        { self.state.paused.store(true,  Ordering::Relaxed); }
    pub fn toggle_pause(&self) {
        let was = self.state.paused.load(Ordering::Relaxed);
        self.state.paused.store(!was, Ordering::Relaxed);
    }
    pub fn seek(&self, secs: f64) {
        let clamped = secs.clamp(0.0, self.duration_secs());
        if let Ok(mut req) = self.state.seek_request.lock() { *req = Some(clamped); }
    }
    pub fn position_secs(&self) -> f64 {
        self.state.position.load(Ordering::Relaxed) as f64 / self.state.sample_rate as f64
    }
    pub fn duration_secs(&self) -> f64 {
        let frames = self.state.total_frames.load(Ordering::Relaxed);
        if frames == 0 { return 0.0; }
        frames as f64 / self.state.sample_rate as f64
    }
    pub fn toggle_loop(&self) {
        let was = self.state.looping.load(Ordering::Relaxed);
        self.state.looping.store(!was, Ordering::Relaxed);
    }
    pub fn is_paused(&self)   -> bool { self.state.paused.load(Ordering::Relaxed) }
    pub fn is_looping(&self)  -> bool { self.state.looping.load(Ordering::Relaxed) }
    pub fn is_finished(&self) -> bool { self.state.finished.load(Ordering::Relaxed) }
    pub fn title(&self)       -> &str { &self.title }

    // ── Live parameter API ────────────────────────────────────────────────────

    /// Update a parameter on the edit identified by `uuid`.
    ///
    /// - **Plugin UUID:** calls `set_parameter` on the live instance immediately.
    ///   Takes effect within one decoded chunk (~85 ms).
    /// - **Structural UUID:** updates the engine's internal snapshot only.
    ///   Takes effect on the next `PlaybackEngine::new` call (after pause + restart).
    pub fn set_edit_param(&self, uuid: Uuid, param_id: &str, value: f32) {
        // Try plugin handles first (hot path)
        for handle in &self.plugin_handles {
            if handle.uuid == uuid {
                if let Ok(mut p) = handle.processor.lock() {
                    p.set_parameter(param_id, value);
                }
                return;
            }
        }
        // Structural: update snapshot
        if let Ok(mut snapshot) = self.structural_snapshot.lock() {
            for edit in snapshot.iter_mut() {
                if edit.uuid == uuid {
                    if let EditKind::Structural { params, .. } = &mut edit.kind {
                        params.insert(param_id.to_string(), value as f64);
                    }
                    break;
                }
            }
        }
    }

    /// Enable or disable the edit identified by `uuid`.
    ///
    /// - **Plugin UUID:** flips the `AtomicBool` gate; skipped on the next chunk.
    /// - **Structural UUID:** updates the snapshot. Takes effect on the next engine creation.
    pub fn set_edit_enabled(&self, uuid: Uuid, enabled: bool) {
        for handle in &self.plugin_handles {
            if handle.uuid == uuid {
                handle.enabled.store(enabled, Ordering::Relaxed);
                return;
            }
        }
        if let Ok(mut snapshot) = self.structural_snapshot.lock() {
            for edit in snapshot.iter_mut() {
                if edit.uuid == uuid {
                    edit.enabled = enabled;
                    break;
                }
            }
        }
    }
}

// ── Decode helpers ────────────────────────────────────────────────────────────

fn build_fresh_chain(path: &Path, edits: &[StructuralEdit], registry: &structural_processor_sdk::Registry) -> Result<Box<dyn AudioSource>> {
    let source = Box::new(FileAudioSource::new(path)?);
    Ok(build_chain(source, edits, registry))
}

fn decode_loop(
    path: PathBuf,
    structural_edits: Vec<StructuralEdit>,
    plugin_handles: Vec<Arc<PluginHandle>>,
    state: Arc<PlaybackState>,
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
        // ── Seek ──────────────────────────────────────────────────────────────
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target) = req.take() {
                match build_fresh_chain(&path, &structural_edits, &registry) {
                    Ok(c) => {
                        chain = c;
                        cursor_secs = target;
                        let frame_pos = (target * sample_rate as f64) as u64;
                        state.position.store(frame_pos, Ordering::Relaxed);
                        if let Ok(mut buf) = state.buffer.lock() { buf.clear(); }
                        // Reset plugin state (flush delay lines, reverb tails)
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

        // ── End of stream ─────────────────────────────────────────────────────
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
                Err(e) => { eprintln!("decode rebuild error: {e}"); state.finished.store(true, Ordering::Relaxed); return; }
            };
            cursor_secs = 0.0;
            continue;
        }

        // ── Buffer full — wait ────────────────────────────────────────────────
        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        // ── Decode + apply plugins ────────────────────────────────────────────
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
                Err(e) => { eprintln!("decode rebuild error: {e}"); state.finished.store(true, Ordering::Relaxed); return; }
            };
            cursor_secs = 0.0;
            continue;
        }

        // Apply plugin chain in order
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

#### Step 8.4 — Run tests

```
cargo test -p musicum-core -- player
```

Expected: all 4 player tests pass. Fix any compile errors in plugin struct names.

#### Step 8.5 — Build the whole crate

```
cargo build -p musicum-core
```

Expected: clean build (no warnings about unused imports, etc.).

---

### Task 9: Update `musicum-core/src/lib.rs` — re-exports

**Files:**
- Modify: `libs/musicum-core/src/lib.rs`

#### Step 9.1 — Add re-exports

```rust
pub mod audio;
pub mod config;
pub mod db;
pub mod edit;
pub mod error;
pub mod services;
pub mod sidecar;

pub use audio::{EditRegistry, PlaybackEngine, structural_edits_from};
pub use edit::{EditKind, ProcessorEdit, deserialize_processor_edits};
pub use error::ServiceError;
```

#### Step 9.2 — Build

```
cargo build -p musicum-core
```

Expected: clean.

---

### Task 10: Update `clip_service.rs` — use `ProcessorEdit`

`update_clip_processors` currently takes `Vec<ProcessorEntry>`. Change it to `Vec<ProcessorEdit>`. The sidecar JSON now serializes `ProcessorEdit`. The DB column stores the same JSON.

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

#### Step 10.1 — Write the failing test

The existing tests don't call `update_clip_processors`, so add one:

```rust
#[tokio::test]
async fn update_processors_stores_new_format() {
    use crate::edit::{EditKind, ProcessorEdit};
    use std::collections::HashMap;
    use uuid::Uuid;

    let db = test_db().await;
    let dir = tempdir().unwrap();
    let audio = dir.path().join("test.wav");
    std::fs::write(&audio, b"").unwrap();
    setup(&db, &audio).await;

    let mut params = HashMap::new();
    params.insert("gain".to_string(), 0.75_f32);
    let edit = ProcessorEdit {
        uuid: Uuid::new_v4(),
        enabled: true,
        kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
    };

    update_clip_processors(&db, "my-clip", vec![edit.clone()]).await.unwrap();

    let clip = get_clip_by_slug(&db, "my-clip").await.unwrap();
    let loaded: Vec<ProcessorEdit> = serde_json::from_str(&clip.processors).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].uuid, edit.uuid);
}
```

#### Step 10.2 — Run the test (expect compile errors)

```
cargo test -p musicum-core -- clip_service 2>&1 | head -20
```

#### Step 10.3 — Update `clip_service.rs`

Change the imports and function signature:

```rust
// Remove: use crate::sidecar::{self, ClipSidecar, ProcessorEntry};
// Add:
use crate::edit::ProcessorEdit;
use crate::sidecar::{self, ClipSidecar};
```

Change `update_clip_processors`:

```rust
pub async fn update_clip_processors(
    db: &DatabaseConnection,
    clip_slug: &str,
    processors: Vec<ProcessorEdit>,   // ← was Vec<ProcessorEntry>
) -> Result<(), ServiceError> {
    let clip = get_clip_by_slug(db, clip_slug).await?;
    let file = file_service::get_file_by_id(db, &clip.file_id).await?;
    let audio_path = Path::new(&file.path);

    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    let entry = sc
        .clips
        .iter_mut()
        .find(|c| c.slug == clip_slug)
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{clip_slug}' in sidecar")))?;
    entry.processors = processors.clone();
    sidecar::write_file_sidecar(audio_path, &sc)?;

    let processors_json = serde_json::to_string(&processors)?;
    let now = chrono::Utc::now().to_rfc3339();
    clip::ActiveModel {
        id:          Set(clip.id),
        slug:        Set(clip.slug),
        file_id:     Set(clip.file_id),
        title:       Set(clip.title),
        processors:  Set(processors_json),
        duration:    Set(clip.duration),
        notes:       Set(clip.notes),
        created_at:  Set(clip.created_at),
        updated_at:  Set(now),
    }
    .update(db)
    .await?;

    Ok(())
}
```

#### Step 10.4 — Run tests

```
cargo test -p musicum-core -- clip_service
```

Expected: all tests pass.

---

### Task 11: Update `preset_service.rs` — use `ProcessorEdit`

**Files:**
- Modify: `libs/musicum-core/src/services/preset_service.rs`

#### Step 11.1 — Read the full preset_service

```bash
cat libs/musicum-core/src/services/preset_service.rs
```

#### Step 11.2 — Identify all `sidecar::ProcessorEntry` references

These will be in `apply_preset_to_clip` / `update_preset_processors` functions. Change them to use `ProcessorEdit`.

For each occurrence:
- `sidecar::ProcessorEntry::Structural { id, .. } => id.as_str()` → `ProcessorEdit { kind: EditKind::Structural { processor_id, .. }, .. } => processor_id.as_str()`
- `sidecar::ProcessorEntry::AudioPlugin { id, .. } => id.as_str()` → `ProcessorEdit { kind: EditKind::Plugin { plugin_id, .. }, .. } => plugin_id.as_str()`

The `processors: Vec<sidecar::ProcessorEntry>` fields on return types change to `Vec<ProcessorEdit>`.

Specific changes in `apply_preset_to_clip` (lines ~79-117 based on earlier read):

```rust
// Replace imports at top:
use crate::edit::ProcessorEdit;
// Remove: use crate::sidecar; (keep if other sidecar types used)

// Change Vec<sidecar::ProcessorEntry> → Vec<ProcessorEdit> throughout
// Change match arms:
//   sidecar::ProcessorEntry::Structural { id, .. } => id.as_str()
// →  ProcessorEdit { kind: EditKind::Structural { ref processor_id, .. }, .. } => processor_id.as_str()
//   sidecar::ProcessorEntry::AudioPlugin { id, .. } => id.as_str()  
// →  ProcessorEdit { kind: EditKind::Plugin { ref plugin_id, .. }, .. } => plugin_id.as_str()
```

> **Tip:** Read the full file first (`cat libs/musicum-core/src/services/preset_service.rs`), then make targeted edits to each reference. There are no new tests needed here — existing compile-time type checking is sufficient.

#### Step 11.3 — Build

```
cargo build -p musicum-core
```

Expected: clean build.

---

### Task 12: Update CLI `play.rs`

`resolve_target` currently returns `Vec<StructuralEdit>`. Change it to return `Vec<ProcessorEdit>` and pass the full edit list to `PlaybackEngine::new`. The `format_processor_display` function should show structural edits only (plugin display is future work).

**Files:**
- Modify: `apps/cli/src/commands/play.rs`

#### Step 12.1 — Run current build to see errors

```
cargo build -p musicum-cli 2>&1 | head -40
```

After musicum-core changes, this will have errors in `play.rs`.

#### Step 12.2 — Update imports

```rust
// Remove:
//   use musicum_core::audio::{sidecar_entries_to_edits, PlaybackEngine};
//   use musicum_core::sidecar::ProcessorEntry;
//   use structural_processor_sdk::chain::StructuralEdit;

// Add:
use musicum_core::{
    audio::{structural_edits_from, PlaybackEngine, EditRegistry},
    deserialize_processor_edits,
    edit::{EditKind, ProcessorEdit},
    services::{clip_service, file_service},
};
```

#### Step 12.3 — Update `resolve_target`

```rust
async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<ProcessorEdit>)> {  // ← return type change
    if force_file {
        let file = file_service::get_file_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no file with slug '{target}'"))?;
        return Ok((PathBuf::from(file.path), vec![]));
    }

    if force_clip {
        let clip = clip_service::get_clip_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no clip with slug '{target}'"))?;
        let file = file_service::get_file_by_id(db, &clip.file_id)
            .await
            .map_err(|_| anyhow!("parent file for clip '{target}' not found"))?;
        let edits = deserialize_processor_edits(&clip.processors);
        return Ok((PathBuf::from(file.path), edits));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok((PathBuf::from(file.path), vec![]));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            let edits = deserialize_processor_edits(&clip.processors);
            return Ok((PathBuf::from(file.path), edits));
        }
    }
    let path = PathBuf::from(target);
    if path.exists() {
        return Ok((path, vec![]));
    }
    Err(anyhow!("'{target}' is not a known slug or an existing file path"))
}
```

#### Step 12.4 — Update `run`

```rust
pub async fn run(db: &DatabaseConnection, target: String, force_file: bool, force_clip: bool, loop_mode: bool) -> Result<()> {
    let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
    // Show structural edits in the display (plugins are silent in the TUI for now)
    let structural = structural_edits_from(&edits);
    let processor_display = format_processor_display(&structural);
    let registry = EditRegistry::default();
    let engine = PlaybackEngine::new(&path, &edits, &registry)?;
    if loop_mode { engine.toggle_loop(); }
    engine.play();
    run_player(engine, processor_display)
}
```

`format_processor_display` keeps its current signature `&[StructuralEdit]` — no change needed.

#### Step 12.5 — Build

```
cargo build -p musicum-cli
```

Expected: clean.

---

### Task 13: Update CLI `export.rs` + `export_service.rs`

Export applies structural edits only (plugins are not in scope per spec Non-goals). `resolve_target` in `export.rs` changes to return `Vec<ProcessorEdit>`; `export_audio` keeps `&[StructuralEdit]` but callers now derive structural edits first.

**Files:**
- Modify: `apps/cli/src/commands/export.rs`

#### Step 13.1 — Update imports

```rust
// Remove:
//   use musicum_core::audio::sidecar_entries_to_edits;
//   use musicum_core::sidecar::ProcessorEntry;
//   use structural_processor_sdk::chain::StructuralEdit;

// Add:
use musicum_core::{
    audio::structural_edits_from,
    deserialize_processor_edits,
    edit::ProcessorEdit,
    services::{clip_service, file_service, export_service::{export_audio, ExportOptions}},
};
use structural_processor_sdk::chain::StructuralEdit;
```

#### Step 13.2 — Update `resolve_target` in `export.rs`

```rust
async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<StructuralEdit>)> {
    if force_file {
        let file = file_service::get_file_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no file with slug '{target}'"))?;
        return Ok((PathBuf::from(file.path), vec![]));
    }

    if force_clip {
        let clip = clip_service::get_clip_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no clip with slug '{target}'"))?;
        let file = file_service::get_file_by_id(db, &clip.file_id)
            .await
            .map_err(|_| anyhow!("parent file for clip '{target}' not found"))?;
        let edits = deserialize_processor_edits(&clip.processors);
        return Ok((PathBuf::from(file.path), structural_edits_from(&edits)));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok((PathBuf::from(file.path), vec![]));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            let edits = deserialize_processor_edits(&clip.processors);
            return Ok((PathBuf::from(file.path), structural_edits_from(&edits)));
        }
    }

    Err(anyhow!("'{}' is not a known file or clip slug", target))
}
```

No changes needed to `export_service.rs` — it still takes `&[StructuralEdit]`.

#### Step 13.3 — Build + test

```
cargo build -p musicum-cli
```

```
cargo test -p musicum-core
```

Expected: clean build, all tests pass.

---

### Task 14: Final cleanup + full test run

#### Step 14.1 — Remove dead `ProcessorEntry` pub items

If `ProcessorEntry` / `ProcessorRef` are still `pub` in `sidecar.rs` but no longer referenced externally, make them private (or `pub(crate)`). The migration logic inside `deserialize_processor_edits` uses local inline types, so the outer definitions are not needed.

Check:
```bash
grep -rn "ProcessorEntry\|ProcessorRef" libs/ apps/ --include="*.rs"
```

Remove all `pub use` / `pub struct` for `ProcessorEntry` and `ProcessorRef` in `sidecar.rs`. Replace with `pub(crate)` if they're used within the crate, or inline them in `deserialize_processor_edits` (already done).

#### Step 14.2 — Run clippy

```
cargo clippy --all 2>&1 | grep -E "^error|^warning"
```

Fix any warnings (unused imports, dead code).

#### Step 14.3 — Run all tests

```
cargo test -p musicum-core
```

Expected output (all pass):
```
test edit::tests::roundtrip_structural ... ok
test edit::tests::roundtrip_plugin ... ok
test edit::tests::deserialize_new_format_vec ... ok
test edit::tests::deserialize_old_structural_format ... ok
test edit::tests::deserialize_old_audio_plugin_format ... ok
test edit::tests::deserialize_empty_json ... ok
test edit::tests::deserialize_garbage_returns_empty ... ok
test audio::registry::tests::default_registry_has_all_structural_processors ... ok
test audio::registry::tests::default_registry_has_all_plugins ... ok
test audio::registry::tests::plugin_entry_create_works ... ok
test audio::tests::structural_edits_from_filters_plugins ... ok
test audio::tests::structural_edits_from_preserves_enabled_flag ... ok
test audio::player::tests::new_with_no_edits_creates_engine ... ok
test audio::player::tests::new_with_plugin_edit_creates_engine ... ok
test audio::player::tests::set_edit_param_on_plugin_does_not_panic ... ok
test audio::player::tests::set_edit_enabled_on_plugin_does_not_panic ... ok
test sidecar::tests::write_read_sidecar_with_processor_edits ... ok
test sidecar::tests::read_sidecar_with_old_processor_entry_format ... ok
test services::clip_service::tests::update_processors_stores_new_format ... ok
... (existing tests)
```

#### Step 14.4 — Smoke test the CLI

If an audio file is available locally:
```bash
cargo run -p musicum-cli -- play <slug-or-path>
```

Expected: playback works, player TUI renders.

---

## Summary of Behavioural Changes

| Behaviour | Before | After |
|-----------|--------|-------|
| `PlaybackEngine::new` signature | `(&Path, &[StructuralEdit])` | `(&Path, &[ProcessorEdit], &EditRegistry)` |
| Audio plugins in playback | No-op (filtered out) | Applied per chunk via `process()` |
| Plugin parameter update | Not possible | `set_edit_param(uuid, id, value)` |
| Seek | Structural chain rebuild | Structural rebuild + plugin `reset()` |
| Sidecar format | `ProcessorEntry` (old) | `ProcessorEdit` (new, with old-format fallback) |
| Export | Structural only (unchanged) | Structural only (unchanged) |
