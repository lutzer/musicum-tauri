# Audio Plugin CLI Integration

**Date:** 2026-05-28
**Status:** Approved

## Goal

Expose audio plugins in the CLI alongside structural processors: in the `processors list` command and in the `processor_list_editor` used by preset and clip edit views.

## Scope

Two files change:

- `apps/cli/src/commands/processors.rs` — `processors list` command
- `apps/cli/src/commands/processor_list_editor.rs` — shared TUI editor

No new crates, no schema changes, no new dependencies. The CLI already depends on `musicum-core`, which exports `EditRegistry::default()` combining both registries.

---

## 1. `processors list` command

### Current behaviour
Iterates `structural_processors::registry()` only. No TYPE column.

### New behaviour
- Use `musicum_core::EditRegistry::default()` to get both registries.
- Build a unified sorted list of all processors and plugins.
- Add a **TYPE** column with values `structural` or `audio-plugin`.
- **PARAMETERS** column: for audio plugins, list only `Float` and `Bool` parameter descriptors (format: `id=default (float|bool)`). `Action` and `Canvas` parameters are omitted (no persistent value).
- JSON output includes a `"type"` field on each entry.

### Table columns
```
ID           TYPE          NAME            PARAMETERS
trim         structural    Trim            start=0.0 (time), end=0.0 (time)
gain         audio-plugin  Gain            gain=1.0 (float)
pan          audio-plugin  Pan             pan=0.0 (float)
```

---

## 2. `processor_list_editor` — available types

### New type
```rust
enum AvailableKind { Structural, Plugin }

struct AvailableType {
    id:   String,
    name: String,
    kind: AvailableKind,
}
```

`EditorState.available_types` changes from `Vec<(String, String)>` to `Vec<AvailableType>`.

### Building the list
`EditorState::new()` calls `EditRegistry::default()` and merges both registries into `available_types`, sorted by `id`. `EditorState` also stores the `PluginRegistry` for later descriptor lookups.

### Picker overlay
Each entry in the "Add Processor" overlay shows a type badge before the name:

```
[structural]  Trim  (trim)
[audio-plugin]  Gain  (gain)
```

---

## 3. `processor_list_editor` — adding processors

### `add_processor(id: &str, kind: AvailableKind)`
- **Structural:** existing logic unchanged — look up `StructuralRegistry`, collect `ParameterDescriptor` defaults, create `EditKind::Structural`.
- **Plugin:** look up `PluginRegistry`, iterate `PluginDescriptor.parameters`:
  - `PluginParameter::Float { id, default, .. }` → insert `(id, default as f32)` into params map
  - `PluginParameter::Bool { id, default, .. }` → insert `(id, if *default { 1.0 } else { 0.0 })`
  - `Action` and `Canvas` → skip (no persistent state)
  - Create `EditKind::Plugin { plugin_id, params }`.

---

## 4. `processor_list_editor` — parameter display

### `ParamRow`
```rust
struct ParamRow {
    key:     String,
    value:   serde_json::Value,
    is_bool: bool,
}
```

### Building param rows for plugins
`params_for_selected()` looks up the plugin descriptor from the stored `PluginRegistry` to determine which keys are Bool. Structural processors have no Bool params so `is_bool` is always `false` for them.

### Rendering Bool params
- Display: `[on]` (cyan) or `[off]` (dark gray) instead of `1` / `0`.
- `Enter` on a Bool param row **toggles** the value in-place and saves immediately — no text-edit mode entered.
- Float params behave exactly as today (Enter → Editing mode → text input).

### Key handling change
In `Mode::Normal`, `(KeyCode::Enter, _)` when `active_pane == Pane::Params`:
1. If selected param is Bool → toggle + save, stay in Normal mode.
2. Otherwise → enter Editing mode (current behaviour).

---

## Non-goals

- No Bool toggle for structural processors (they have no Bool params).
- No per-plugin UI customisation (canvas, action buttons).
- No dynamic plugin loading (WASM plugins are out of scope here).
