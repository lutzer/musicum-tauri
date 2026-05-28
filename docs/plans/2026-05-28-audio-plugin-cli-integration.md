# Audio Plugin CLI Integration Implementation Plan

**Goal:** Expose audio plugins alongside structural processors in the `processors list` command and the shared `processor_list_editor` TUI used by preset and clip edit views.

**Architecture:** Use `musicum_core::EditRegistry::default()` (already available via the CLI's existing `musicum-core` dependency) to enumerate both structural processors and audio plugins. The `processors list` command gains a TYPE column. The `processor_list_editor` gains plugin support in the picker, plugin-aware param rows with bool toggle rendering, and an updated `add_processor` that builds the correct `EditKind` variant.

**Tech Stack:** Rust, ratatui 0.29, `musicum-core::EditRegistry`, `audio-plugin-sdk` (added as a direct CLI dep for `PluginParameter` / `PluginRegistry` types)

---

## File map

| File | Change |
|------|--------|
| `apps/cli/Cargo.toml` | Add `audio-plugin-sdk` direct dep |
| `apps/cli/src/commands/processors.rs` | Use `EditRegistry`, add TYPE column, list plugins |
| `apps/cli/src/commands/processor_list_editor.rs` | New types, plugin registry in state, updated picker + param display |

---

### Task 1: Add `audio-plugin-sdk` to CLI dependencies

The CLI needs direct access to `PluginParameter` and `PluginRegistry` types which live in `audio-plugin-sdk`. `musicum-core` already depends on it but Rust requires explicit deps to use a crate's types directly.

**Files:**
- Modify: `apps/cli/Cargo.toml`

**Step 1.1** — Open `apps/cli/Cargo.toml`. Add this line to `[dependencies]`, after `musicum-core`:

```toml
audio-plugin-sdk         = { path = "../../libs/audio-plugin-sdk" }
```

**Step 1.2** — Verify it compiles:

```
cargo build -p musicum-cli
```

Expected: no errors.

---

### Task 2: Update `processors list` command

Replace the structural-only list with a unified list from `EditRegistry`. Add a TYPE column.

**Files:**
- Modify: `apps/cli/src/commands/processors.rs`

**Step 2.1** — Replace the entire file with:

```rust
use audio_plugin_sdk::PluginParameter;
use clap::{Args, Subcommand};
use musicum_core::EditRegistry;
use serde::Serialize;
use structural_processor_sdk::processor::ParameterDescriptor;

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
            let mut entries: Vec<ProcessorListEntry> = Vec::new();

            for entry in registry.structural.values() {
                let d = (entry.descriptor)();
                let parameters = d
                    .parameters
                    .iter()
                    .map(|p| match p {
                        ParameterDescriptor::Time { id, default, .. } =>
                            format!("{id}={default} (time)"),
                        ParameterDescriptor::Int { id, default, .. } =>
                            format!("{id}={default} (int)"),
                    })
                    .collect();
                entries.push(ProcessorListEntry {
                    id:   d.id.to_string(),
                    kind: "structural".to_string(),
                    name: d.name.to_string(),
                    parameters,
                });
            }

            for (id, entry) in &registry.plugins {
                let d = (entry.descriptor)();
                let parameters = d
                    .parameters
                    .iter()
                    .filter_map(|p| match p {
                        PluginParameter::Float { id, default, .. } =>
                            Some(format!("{id}={default} (float)")),
                        PluginParameter::Bool { id, default, .. } =>
                            Some(format!("{id}={} (bool)", if *default { 1 } else { 0 })),
                        PluginParameter::Action { .. } | PluginParameter::Canvas { .. } => None,
                    })
                    .collect();
                entries.push(ProcessorListEntry {
                    id:   id.clone(),
                    kind: "audio-plugin".to_string(),
                    name: d.name.to_string(),
                    parameters,
                });
            }

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
                        .map(|e| {
                            vec![
                                e.id.clone(),
                                e.kind.clone(),
                                e.name.clone(),
                                e.parameters.join(", "),
                            ]
                        })
                        .collect(),
                );
            }
        }
    }
}
```

**Step 2.2** — Build and smoke-test:

```
cargo run -p musicum-cli -- processors list
```

Expected output includes both structural rows (`structural`) and plugin rows (`audio-plugin`) in a four-column table, sorted by ID. For example:
```
── processors ────────────────────────...
crop         structural    Crop        ...
cut          structural    Cut         ...
gain         audio-plugin  Gain        gain=1 (float)
level-meter  audio-plugin  Level Meter ...
```

**Step 2.3** — Test JSON output:

```
cargo run -p musicum-cli -- processors list --json
```

Expected: JSON array where each object has `"id"`, `"type"`, `"name"`, `"parameters"` fields.

---

### Task 3: Add `AvailableKind` / `AvailableType` types and update `EditorState`

Introduce the new types, store a `PluginRegistry` in `EditorState`, and rebuild `available_types` from both registries.

**Files:**
- Modify: `apps/cli/src/commands/processor_list_editor.rs`

**Step 3.1** — Add new imports at the top of the file (after the existing `use` block):

```rust
use audio_plugin_sdk::{PluginParameter, PluginRegistry};
use musicum_core::EditRegistry;
```

**Step 3.2** — Add the new types immediately after the `ParamRow` struct (before `Pane`):

```rust
#[derive(Clone, PartialEq)]
enum AvailableKind {
    Structural,
    Plugin,
}

#[derive(Clone)]
struct AvailableType {
    id:   String,
    name: String,
    kind: AvailableKind,
}
```

**Step 3.3** — In the `EditorState` struct, replace:

```rust
    available_types: Vec<(String, String)>,
```

with:

```rust
    available_types: Vec<AvailableType>,
    plugin_registry: PluginRegistry,
```

**Step 3.4** — Replace the entire `EditorState::new()` body:

```rust
    fn new(title: String, processors: Vec<ProcessorEdit>) -> Self {
        let edit_registry = EditRegistry::default();
        let mut available_types: Vec<AvailableType> = Vec::new();

        for entry in edit_registry.structural.values() {
            let d = (entry.descriptor)();
            available_types.push(AvailableType {
                id:   d.id.to_string(),
                name: d.name.to_string(),
                kind: AvailableKind::Structural,
            });
        }

        for (id, entry) in &edit_registry.plugins {
            let d = (entry.descriptor)();
            available_types.push(AvailableType {
                id:   id.clone(),
                name: d.name.to_string(),
                kind: AvailableKind::Plugin,
            });
        }

        available_types.sort_by(|a, b| a.id.cmp(&b.id));
        let plugin_registry = edit_registry.plugins;

        let mut proc_state = ListState::default();
        if !processors.is_empty() {
            proc_state.select(Some(0));
        }
        Self {
            title,
            processors,
            available_types,
            plugin_registry,
            proc_state,
            param_state: ListState::default(),
            active_pane: Pane::Processors,
            mode: Mode::Normal,
            picker_idx: 0,
            edit_buf: String::new(),
            status_msg: None,
        }
    }
```

**Step 3.5** — Verify the struct changes compile (other methods will fail until fixed, that's expected):

```
cargo check -p musicum-cli 2>&1 | head -40
```

Expected: errors only about `available_types` field usage in `add_processor` and `draw_picker_overlay` — not about the struct itself.

---

### Task 4: Update `add_processor` to handle both kinds

**Files:**
- Modify: `apps/cli/src/commands/processor_list_editor.rs`

**Step 4.1** — Replace the entire `add_processor` method:

```rust
    fn add_processor(&mut self, available: &AvailableType) {
        match available.kind {
            AvailableKind::Structural => {
                let registry = structural_processors::registry();
                let Some(entry) = registry.values().find(|e| (e.descriptor)().id == available.id)
                else {
                    return;
                };
                let descriptor = (entry.descriptor)();
                let mut params = std::collections::HashMap::new();
                for p in descriptor.parameters {
                    let (id, val) = match p {
                        ParameterDescriptor::Time { id, default, .. } => (id, *default),
                        ParameterDescriptor::Int { id, default, .. } => (id, *default as f64),
                    };
                    params.insert(id.to_string(), val);
                }
                let new_entry = ProcessorEdit {
                    uuid:    Uuid::new_v4(),
                    enabled: true,
                    kind:    EditKind::Structural {
                        processor_id: available.id.clone(),
                        params,
                    },
                };
                let insert_at = self.proc_state.selected().map(|i| i + 1).unwrap_or(0);
                self.processors.insert(insert_at, new_entry);
                self.proc_state.select(Some(insert_at));
            }
            AvailableKind::Plugin => {
                let Some(entry) = self.plugin_registry.get(&available.id) else {
                    return;
                };
                let descriptor = (entry.descriptor)();
                let mut params = std::collections::HashMap::new();
                for p in descriptor.parameters {
                    match p {
                        PluginParameter::Float { id, default, .. } => {
                            params.insert(id.to_string(), *default);
                        }
                        PluginParameter::Bool { id, default, .. } => {
                            params.insert(id.to_string(), if *default { 1.0_f32 } else { 0.0_f32 });
                        }
                        PluginParameter::Action { .. } | PluginParameter::Canvas { .. } => {}
                    }
                }
                let new_entry = ProcessorEdit {
                    uuid:    Uuid::new_v4(),
                    enabled: true,
                    kind:    EditKind::Plugin {
                        plugin_id: available.id.clone(),
                        params,
                    },
                };
                let insert_at = self.proc_state.selected().map(|i| i + 1).unwrap_or(0);
                self.processors.insert(insert_at, new_entry);
                self.proc_state.select(Some(insert_at));
            }
        }
    }
```

**Step 4.2** — Update the picker's `KeyCode::Enter` handler in `run_loop` (inside `Mode::Picking`). Replace:

```rust
                KeyCode::Enter => {
                    if let Some((type_id, _)) = state.available_types.get(state.picker_idx).cloned() {
                        state.add_processor(&type_id);
                        state.mode = Mode::Normal;
                        match save(state.processors.clone()).await {
                            Ok(_) => state.status_msg = Some(format!("added {type_id}")),
                            Err(e) => state.status_msg = Some(format!("error: {e}")),
                        }
                    }
                }
```

with:

```rust
                KeyCode::Enter => {
                    if let Some(available) = state.available_types.get(state.picker_idx).cloned() {
                        let id = available.id.clone();
                        state.add_processor(&available);
                        state.mode = Mode::Normal;
                        match save(state.processors.clone()).await {
                            Ok(_) => state.status_msg = Some(format!("added {id}")),
                            Err(e) => state.status_msg = Some(format!("error: {e}")),
                        }
                    }
                }
```

**Step 4.3** — Build:

```
cargo build -p musicum-cli
```

Expected: compiles. Picker overlay render may still reference old tuple — fix in Task 6.

---

### Task 5: Update `ParamRow` and `params_for_selected` for bool detection

**Files:**
- Modify: `apps/cli/src/commands/processor_list_editor.rs`

**Step 5.1** — Update the `ParamRow` struct. Replace:

```rust
#[derive(Clone)]
struct ParamRow {
    key:   String,
    value: serde_json::Value,
}
```

with:

```rust
#[derive(Clone)]
struct ParamRow {
    key:     String,
    value:   serde_json::Value,
    is_bool: bool,
}
```

**Step 5.2** — Replace the entire `params_for_selected` method:

```rust
    fn params_for_selected(&self) -> Vec<ParamRow> {
        let idx = match self.selected_proc_index() {
            Some(i) => i,
            None => return vec![],
        };
        match &self.processors[idx].kind {
            EditKind::Structural { params, .. } => params
                .iter()
                .map(|(k, v)| ParamRow {
                    key:     k.clone(),
                    value:   serde_json::json!(v),
                    is_bool: false,
                })
                .collect(),
            EditKind::Plugin { plugin_id, params } => {
                let bool_keys: std::collections::HashSet<&str> = self
                    .plugin_registry
                    .get(plugin_id.as_str())
                    .map(|entry| {
                        let d = (entry.descriptor)();
                        d.parameters
                            .iter()
                            .filter_map(|p| {
                                if let PluginParameter::Bool { id, .. } = p {
                                    Some(*id)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                params
                    .iter()
                    .map(|(k, v)| ParamRow {
                        key:     k.clone(),
                        value:   serde_json::json!(v),
                        is_bool: bool_keys.contains(k.as_str()),
                    })
                    .collect()
            }
        }
    }
```

**Step 5.3** — Build:

```
cargo build -p musicum-cli
```

Expected: compiles cleanly (the `ParamRow` field additions may cause minor issues in the draw functions, fixed in Task 6).

---

### Task 6: Update param rendering and bool toggle key handler

**Files:**
- Modify: `apps/cli/src/commands/processor_list_editor.rs`

**Step 6.1** — In `draw_params`, update the non-editing param list rendering. Find the `ListItem` construction in the `else` branch of `if state.mode == Mode::Editing`. Replace:

```rust
        let items: Vec<ListItem> = params
            .iter()
            .map(|row| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{}: ", row.key)),
                    Span::styled(row.value.to_string(), Style::default().fg(Color::Green)),
                ]))
            })
            .collect();
```

with:

```rust
        let items: Vec<ListItem> = params
            .iter()
            .map(|row| {
                let value_span = if row.is_bool {
                    if row.value.as_f64().unwrap_or(0.0) != 0.0 {
                        Span::styled("[on]", Style::default().fg(Color::Cyan))
                    } else {
                        Span::styled("[off]", Style::default().fg(Color::DarkGray))
                    }
                } else {
                    Span::styled(row.value.to_string(), Style::default().fg(Color::Green))
                };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{}: ", row.key)),
                    value_span,
                ]))
            })
            .collect();
```

**Step 6.2** — Also update the editing-mode param list (the list shown above the input box when `Mode::Editing`). This list uses a manually styled version. Find the items construction inside `if state.mode == Mode::Editing` that renders the param list. Replace:

```rust
        let items: Vec<ListItem> = params
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let style = if Some(i) == selected_param_idx {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}: ", row.key), style),
                    Span::styled(row.value.to_string(), style),
                ]))
            })
            .collect();
```

with:

```rust
        let items: Vec<ListItem> = params
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let key_style = if Some(i) == selected_param_idx {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let val_span = if row.is_bool {
                    if row.value.as_f64().unwrap_or(0.0) != 0.0 {
                        Span::styled("[on]", Style::default().fg(Color::Cyan))
                    } else {
                        Span::styled("[off]", Style::default().fg(Color::DarkGray))
                    }
                } else {
                    Span::styled(row.value.to_string(), key_style)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}: ", row.key), key_style),
                    val_span,
                ]))
            })
            .collect();
```

**Step 6.3** — Update the `(KeyCode::Enter, _)` handler in `Mode::Normal`. Replace:

```rust
                (KeyCode::Enter, _) => {
                    if state.active_pane == Pane::Params {
                        state.enter_edit();
                        state.mode = Mode::Editing;
                    }
                }
```

with:

```rust
                (KeyCode::Enter, _) => {
                    if state.active_pane == Pane::Params {
                        let params = state.params_for_selected();
                        let selected = state.param_state.selected()
                            .and_then(|i| params.get(i))
                            .cloned();
                        if let Some(row) = selected {
                            if row.is_bool {
                                let toggled = if row.value.as_f64().unwrap_or(0.0) != 0.0 {
                                    0.0_f32
                                } else {
                                    1.0_f32
                                };
                                let key = row.key.clone();
                                state.apply_edit_to_processors(&key, serde_json::json!(toggled));
                                match save(state.processors.clone()).await {
                                    Ok(_) => state.status_msg = Some(format!("{key} toggled")),
                                    Err(e) => state.status_msg = Some(format!("error: {e}")),
                                }
                            } else {
                                state.enter_edit();
                                state.mode = Mode::Editing;
                            }
                        }
                    }
                }
```

**Step 6.4** — Build:

```
cargo build -p musicum-cli
```

Expected: compiles. The picker overlay still uses the old tuple field access — fix in Task 7.

---

### Task 7: Update picker overlay rendering

**Files:**
- Modify: `apps/cli/src/commands/processor_list_editor.rs`

**Step 7.1** — Replace the entire `draw_picker_overlay` function:

```rust
fn draw_picker_overlay(f: &mut Frame, state: &EditorState, area: Rect) {
    let popup = centered_rect(60, 60, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = state
        .available_types
        .iter()
        .enumerate()
        .map(|(i, avail)| {
            let name_style = if i == state.picker_idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let badge = match avail.kind {
                AvailableKind::Structural => "[structural]  ",
                AvailableKind::Plugin     => "[audio-plugin]",
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{badge} "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", avail.name), name_style),
                Span::styled(format!("({})", avail.id), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Add Processor ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(list, popup);
}
```

Note the popup width is bumped from 50% to 60% to accommodate the longer `[audio-plugin]` badge.

**Step 7.2** — Full build:

```
cargo build -p musicum-cli
```

Expected: clean build with no errors.

---

### Task 8: Lint and verify

**Step 8.1** — Run clippy:

```
cargo clippy --all
```

Fix any warnings before proceeding.

**Step 8.2** — Manual smoke test of `processors list`:

```
cargo run -p musicum-cli -- processors list
```

Verify: 10 rows total (4 structural + 6 plugins), sorted by ID, TYPE column populated.

**Step 8.3** — Manual smoke test of the editor picker (open any preset or clip edit view, press `a` in the processors pane):

- Picker shows all 10 entries with `[structural]` / `[audio-plugin]` badges
- Selecting a plugin entry and pressing Enter adds an `[audio-plugin]` row to the processors list
- Tabbing to Params shows the plugin's parameters
- Bool params (if any) show `[on]`/`[off]`; pressing Enter toggles them

**Step 8.4** — Run core tests to confirm nothing regressed:

```
cargo test -p musicum-core
```

Expected: all tests pass.
