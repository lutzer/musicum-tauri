# Plugin/Processor Registry Refactor

## Goals

1. Remove all direct plugin/processor SDK dependencies from `apps/cli` — the CLI only imports `musicum-core`.
2. Add a unified `ParamInfo`/`EditEntry` interface to `musicum-core` that exposes full parameter metadata without leaking SDK types; this interface is reusable by the future Tauri GUI.
3. Extract processor/plugin logic from `audio/player.rs` into a new `audio/processor_chain.rs`, so `player.rs` is purely about playback state and controls.
4. Keep the existing live-update API (`set_edit_param`, `set_edit_enabled`) unchanged on `PlaybackEngine`.

---

## Part 1 — Unified parameter types in `musicum-core`

Add to `libs/musicum-core/src/audio/registry.rs`:

```rust
pub enum ParamInfo {
    Float { id: &'static str, name: &'static str, default: f32,
            min: f32, max: f32, step: f32, unit: Option<&'static str> },
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

`EditRegistry` gains two new methods:

```rust
impl EditRegistry {
    /// Return all registered processors and plugins as frontend-safe entries.
    pub fn list_entries(&self) -> Vec<EditEntry>;

    /// Look up a single entry by processor/plugin ID.
    pub fn get_entry(&self, id: &str) -> Option<EditEntry>;
}
```

Both methods convert SDK types (`PluginParameter`, `ParameterDescriptor`) into `ParamInfo` internally. `Action` and `Canvas` plugin parameters have no persistent value and are excluded from `ParamInfo`. `unit` is `None` when the SDK unit string is empty.

Re-export `ParamInfo`, `EditEntry`, `EditType` from `libs/musicum-core/src/lib.rs`.

---

## Part 2 — New file: `audio/processor_chain.rs`

Move the following out of `player.rs` into a new `libs/musicum-core/src/audio/processor_chain.rs`:

- `PluginHandle` struct (private to the `audio` module):
  ```rust
  pub(super) struct PluginHandle {
      pub uuid:      Uuid,
      pub enabled:   AtomicBool,
      pub processor: Mutex<Box<dyn PluginProcessor>>,
  }
  ```

- `build_plugin_handles(edits: &[ProcessorEdit], registry: &EditRegistry) -> Vec<Arc<PluginHandle>>`  
  Instantiates one `PluginHandle` per plugin edit (enabled or disabled — both get a handle so they can be toggled live).

- `build_fresh_chain(path: &Path, edits: &[StructuralEdit], registry: &StructuralRegistry) -> Result<Box<dyn AudioSource>>`  
  Constructs a `FileAudioSource` and folds structural edits into a chain.

- `decode_loop(path: PathBuf, structural_edits: Vec<StructuralEdit>, plugin_handles: Vec<Arc<PluginHandle>>, state: Arc<PlaybackState>)`  
  The full decode/plugin-apply loop, unchanged in behavior.

`player.rs` imports these from `processor_chain` and delegates accordingly. `PlaybackState` stays in `player.rs` and is given `pub(super)` visibility so `processor_chain.rs` can hold an `Arc<PlaybackState>` in `decode_loop`.

---

## Part 3 — CLI dependency cleanup

Remove from `apps/cli/Cargo.toml`:
- `audio-plugin-sdk`
- `structural-processors`
- `structural-processor-sdk`

Update `apps/cli/src/commands/processors.rs`:
- Remove imports of `PluginParameter` and `ParameterDescriptor`
- Replace the manual registry iteration and match arms with `registry.list_entries()`, mapping `ParamInfo` variants to display strings

---

## Files changed

| File | Change |
|---|---|
| `libs/musicum-core/src/audio/registry.rs` | Add `ParamInfo`, `EditEntry`, `EditType`, `list_entries()`, `get_entry()` |
| `libs/musicum-core/src/audio/processor_chain.rs` | New — `PluginHandle`, `build_plugin_handles`, `build_fresh_chain`, `decode_loop` |
| `libs/musicum-core/src/audio/player.rs` | Remove `PluginHandle`, `build_fresh_chain`, `decode_loop`; import from `processor_chain` |
| `libs/musicum-core/src/audio/mod.rs` | Declare `processor_chain` module |
| `libs/musicum-core/src/lib.rs` | Re-export `ParamInfo`, `EditEntry`, `EditType` |
| `apps/cli/Cargo.toml` | Remove three SDK dependencies |
| `apps/cli/src/commands/processors.rs` | Use `registry.list_entries()` instead of SDK types |

---

## What does NOT change

- SDK crates (`audio-plugin-sdk`, `structural-processor-sdk`) are unchanged — all their types are still used by plugin/processor implementations and by `musicum-core` internally.
- Live-update API on `PlaybackEngine` (`set_edit_param`, `set_edit_enabled`) is unchanged.
- `ProcessorEdit` / `EditKind` in `musicum-core` are unchanged.
- No DB or sidecar format changes.
