# Processor List Editor Implementation Plan

**Goal:** Replace the read-only-param preset editor with a full processor list manager (add, delete, reorder, toggle, edit params) shared between presets and clips.

**Architecture:** A generic `processor_list_editor` module owns all TUI state and rendering. It accepts a typed async save callback so it knows nothing about presets or clips. `presets_editor` becomes a thin wrapper; a new `clips edit` subcommand is a second thin wrapper.

**Tech Stack:** ratatui 0.29, crossterm 0.28, tokio async, structural_processors registry for default params, musicum-core services for persistence.

---

## File Map

| File | Change |
|------|--------|
| `apps/cli/src/commands/processor_list_editor.rs` | **Create** — shared editor TUI |
| `apps/cli/src/commands/presets_editor.rs` | **Replace** — thin wrapper only |
| `apps/cli/src/commands/clips.rs` | **Modify** — add `Edit` subcommand |
| `apps/cli/src/commands/mod.rs` | **Modify** — register new module |
| `libs/musicum-core/src/services/preset_service.rs` | **Modify** — add `update_preset_processors_full` |

---

## Task 1: Add `update_preset_processors_full` to preset_service

**Files:**
- Modify: `libs/musicum-core/src/services/preset_service.rs`

The existing `update_preset_processors` only writes the DB (`_library_dir` is ignored).
The editor needs a single call that writes sidecar + DB for presets (clips already have
`update_clip_processors` which does both).

**Step 1.1** — Add the function after the existing `update_preset_processors` (after line 148):

```rust
pub async fn update_preset_processors_full(
    db: &DatabaseConnection,
    library_dir: &str,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError> {
    let lib = Path::new(library_dir);
    let mut sc = sidecar::read_preset_sidecar(lib, slug)?;
    sc.processors = processors.clone();
    sidecar::write_preset_sidecar(lib, &sc)?;
    update_preset_processors(db, library_dir, slug, processors).await
}
```

**Step 1.2** — Run `cargo clippy -p musicum-core` and confirm no warnings.

---

## Task 2: Create `processor_list_editor.rs`

**Files:**
- Create: `apps/cli/src/commands/processor_list_editor.rs`

This is the main body of work. Write the complete file as specified below.

### 2.1 — Type aliases and imports

```rust
use std::{future::Future, io, pin::Pin};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;

pub type SaveResult<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;
pub type SaveFn<'a> = Box<dyn Fn(Vec<ProcessorEntry>) -> SaveResult<'a> + 'a>;
```

### 2.2 — State types

```rust
#[derive(Clone)]
struct ParamRow {
    key: String,
    value: serde_json::Value,
}

#[derive(PartialEq)]
enum Pane {
    Processors,
    Params,
}

#[derive(PartialEq)]
enum Mode {
    Normal,
    Picking,  // processor-type picker overlay open
    Editing,  // param value input box open
}

struct EditorState {
    title: String,
    processors: Vec<ProcessorEntry>,
    available_types: Vec<(String, String)>, // (id, display_name) sorted by id
    proc_state: ListState,
    param_state: ListState,
    active_pane: Pane,
    mode: Mode,
    picker_idx: usize,
    edit_buf: String,
    status_msg: Option<String>,
}
```

### 2.3 — `EditorState::new`

```rust
impl EditorState {
    fn new(title: String, processors: Vec<ProcessorEntry>) -> Self {
        let registry = structural_processors::registry();
        let mut available_types: Vec<(String, String)> = registry
            .values()
            .map(|e| {
                let d = (e.descriptor)();
                (d.id.to_string(), d.name.to_string())
            })
            .collect();
        available_types.sort_by(|a, b| a.0.cmp(&b.0));

        let mut proc_state = ListState::default();
        if !processors.is_empty() {
            proc_state.select(Some(0));
        }
        Self {
            title,
            processors,
            available_types,
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

### 2.4 — Existing helper methods (carry over from `presets_editor.rs`)

Copy these methods verbatim but replace `preset_slug` references with `title`:
- `selected_proc_index`
- `params_for_selected`
- `instance_id_for_selected`
- `proc_label`  ← unchanged
- `move_up` / `move_down` ← unchanged
- `enter_edit` ← unchanged, but check `self.mode == Mode::Normal`
- `parse_value` ← unchanged (static method)
- `apply_edit_to_processors` ← unchanged

### 2.5 — New mutation methods

Add these to `EditorState`:

```rust
fn add_processor(&mut self, type_id: &str) {
    let registry = structural_processors::registry();
    let Some(entry) = registry.values().find(|e| (e.descriptor)().id == type_id) else {
        return;
    };
    let descriptor = (entry.descriptor)();
    let mut params = serde_json::Map::new();
    for p in descriptor.parameters {
        let (id, val) = match p {
            ParameterDescriptor::Time { id, default, .. } => (id, serde_json::json!(default)),
            ParameterDescriptor::Int { id, default, .. } => (id, serde_json::json!(default)),
        };
        params.insert(id.to_string(), val);
    }
    let new_entry = ProcessorEntry::Structural {
        id: Uuid::new_v4().to_string(),
        enabled: true,
        processor: ProcessorRef {
            id: type_id.to_string(),
            params: serde_json::Value::Object(params),
        },
    };
    let insert_at = self.proc_state.selected().map(|i| i + 1).unwrap_or(0);
    self.processors.insert(insert_at, new_entry);
    self.proc_state.select(Some(insert_at));
}

fn delete_selected(&mut self) {
    let Some(idx) = self.proc_state.selected() else { return };
    if self.processors.is_empty() { return; }
    self.processors.remove(idx);
    let new_sel = if self.processors.is_empty() {
        None
    } else {
        Some(idx.saturating_sub(if idx >= self.processors.len() { 1 } else { 0 }))
    };
    self.proc_state.select(new_sel);
    self.param_state.select(None);
}

fn move_proc_up(&mut self) {
    let Some(idx) = self.proc_state.selected() else { return };
    if idx == 0 { return; }
    self.processors.swap(idx, idx - 1);
    self.proc_state.select(Some(idx - 1));
}

fn move_proc_down(&mut self) {
    let Some(idx) = self.proc_state.selected() else { return };
    if idx + 1 >= self.processors.len() { return; }
    self.processors.swap(idx, idx + 1);
    self.proc_state.select(Some(idx + 1));
}

fn toggle_selected(&mut self) {
    let Some(idx) = self.proc_state.selected() else { return };
    match &mut self.processors[idx] {
        ProcessorEntry::Structural { enabled, .. } => *enabled = !*enabled,
        ProcessorEntry::AudioPlugin { enabled, .. } => *enabled = !*enabled,
    }
}
```

### 2.6 — `run` async function

The main event loop. Replace the existing `run_editor` pattern:

```rust
pub async fn run<'a>(
    title: &str,
    initial_processors: Vec<ProcessorEntry>,
    save: SaveFn<'a>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = EditorState::new(title.to_string(), initial_processors);

    let result = run_loop(&mut terminal, &mut state, &save).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}

async fn run_loop<'a, B>(
    terminal: &mut Terminal<B>,
    state: &mut EditorState,
    save: &SaveFn<'a>,
) -> Result<()>
where
    B: ratatui::backend::Backend + std::io::Write,
{
    loop {
        terminal.draw(|f| draw(f, state))?;

        let Event::Key(key) = event::read()? else { continue };

        match state.mode {
            Mode::Picking => match key.code {
                KeyCode::Esc => { state.mode = Mode::Normal; }
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.picker_idx > 0 { state.picker_idx -= 1; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.picker_idx + 1 < state.available_types.len() {
                        state.picker_idx += 1;
                    }
                }
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
                _ => {}
            },

            Mode::Editing => match key.code {
                KeyCode::Esc => {
                    state.mode = Mode::Normal;
                    state.edit_buf.clear();
                }
                KeyCode::Backspace => { state.edit_buf.pop(); }
                KeyCode::Char(c) => { state.edit_buf.push(c); }
                KeyCode::Enter => {
                    let params = state.params_for_selected();
                    if let Some(idx) = state.param_state.selected() {
                        if let Some(row) = params.get(idx) {
                            let key = row.key.clone();
                            let value = EditorState::parse_value(&state.edit_buf);
                            let label = format!("{key} = {}", state.edit_buf);
                            state.apply_edit_to_processors(&key, value);
                            state.mode = Mode::Normal;
                            state.edit_buf.clear();
                            match save(state.processors.clone()).await {
                                Ok(_) => state.status_msg = Some(format!("saved {label}")),
                                Err(e) => state.status_msg = Some(format!("error: {e}")),
                            }
                        }
                    }
                }
                _ => {}
            },

            Mode::Normal => match (key.code, key.modifiers) {
                (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                (KeyCode::Tab, _) => {
                    state.active_pane = match state.active_pane {
                        Pane::Processors => {
                            let params = state.params_for_selected();
                            if !params.is_empty() { state.param_state.select(Some(0)); }
                            Pane::Params
                        }
                        Pane::Params => Pane::Processors,
                    };
                    state.status_msg = None;
                }
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => state.move_up(),
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => state.move_down(),
                (KeyCode::Enter, _) => {
                    if state.active_pane == Pane::Params { state.enter_edit(); state.mode = Mode::Editing; }
                }
                // Processor-pane-only actions
                (KeyCode::Char('a'), _) if state.active_pane == Pane::Processors => {
                    state.picker_idx = 0;
                    state.mode = Mode::Picking;
                }
                (KeyCode::Char('d'), _) if state.active_pane == Pane::Processors => {
                    state.delete_selected();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("deleted processor".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Up, KeyModifiers::SHIFT) if state.active_pane == Pane::Processors => {
                    state.move_proc_up();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("moved up".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Down, KeyModifiers::SHIFT) if state.active_pane == Pane::Processors => {
                    state.move_proc_down();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("moved down".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                (KeyCode::Char(' '), _) if state.active_pane == Pane::Processors => {
                    state.toggle_selected();
                    match save(state.processors.clone()).await {
                        Ok(_) => state.status_msg = Some("toggled".to_string()),
                        Err(e) => state.status_msg = Some(format!("error: {e}")),
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}
```

> **Note:** Pattern `(KeyCode::Up, KeyModifiers::SHIFT)` detects Shift+Up in crossterm. This works on most terminals via ANSI escape sequences.

### 2.7 — `draw` and rendering functions

```rust
fn draw(f: &mut Frame, state: &mut EditorState) {
    let area = f.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(outer[0]);

    draw_processors(f, state, panes[0]);
    draw_params(f, state, panes[1]);
    draw_footer(f, state, outer[1]);

    if state.mode == Mode::Picking {
        draw_picker_overlay(f, state, area);
    }
}
```

`draw_processors` — identical to the existing function but use `state.title` instead of `state.preset_slug` in the block title.

`draw_params` — copy verbatim from `presets_editor.rs`. Change the `state.editing` check to `state.mode == Mode::Editing`.

`draw_footer` — three cases:

```rust
fn draw_footer(f: &mut Frame, state: &EditorState, area: Rect) {
    let (text, style) = match state.mode {
        Mode::Editing => (
            " Enter: confirm · Esc: cancel".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Mode::Picking => (
            " Enter: add · Esc: cancel".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Mode::Normal => {
            let hint = if state.active_pane == Pane::Processors {
                " a: add  d: delete  Shift+↑/↓: reorder  Space: toggle  Tab: switch  q: quit"
            } else {
                " Enter: edit  Tab: switch pane  q: quit"
            };
            if let Some(msg) = &state.status_msg {
                (
                    format!(" ✓ {msg}  |  {}", hint.trim_start()),
                    Style::default().fg(Color::Green),
                )
            } else {
                (hint.to_string(), Style::default().fg(Color::DarkGray))
            }
        }
    };
    f.render_widget(Paragraph::new(text).style(style), area);
}
```

`draw_picker_overlay`:

```rust
fn draw_picker_overlay(f: &mut Frame, state: &EditorState, area: Rect) {
    let popup = centered_rect(50, 60, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem> = state
        .available_types
        .iter()
        .enumerate()
        .map(|(i, (id, name))| {
            let style = if i == state.picker_idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{name} "), style),
                Span::styled(format!("({id})"), Style::default().fg(Color::DarkGray)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Add Processor ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(list, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}
```

**Step 2.8** — Run `cargo build -p musicum-cli` after writing the file. Fix any compiler errors before proceeding.

---

## Task 3: Refactor `presets_editor.rs` to thin wrapper

**Files:**
- Modify: `apps/cli/src/commands/presets_editor.rs`

Replace the entire file content:

```rust
use anyhow::Result;
use musicum_core::services::preset_service;
use sea_orm::DatabaseConnection;

use super::processor_list_editor::{run, SaveFn};

pub async fn run_editor(
    db: &DatabaseConnection,
    library_dir: &str,
    preset_slug: &str,
) -> Result<()> {
    let preset = preset_service::get_preset_by_slug(db, preset_slug).await?;
    let processors = serde_json::from_str(&preset.processors).unwrap_or_default();

    let save: SaveFn<'_> = Box::new(|procs| {
        Box::pin(preset_service::update_preset_processors_full(
            db,
            library_dir,
            preset_slug,
            procs,
        ))
    });

    run(&format!("Preset: {preset_slug}"), processors, save).await
}
```

**Step 3.1** — Run `cargo build -p musicum-cli`. Confirm it compiles.

**Step 3.2** — Smoke-test manually: `cargo run -p musicum-cli -- presets edit <some-slug>`. Verify:
- Editor opens and shows existing processors
- Arrow keys and Tab work
- `a` opens the picker overlay
- Picking a type adds a processor and shows "added trim"
- `d` deletes a processor
- Shift+Up / Shift+Down reorders
- Space toggles enabled/disabled
- Enter on a param enters edit mode; confirm saves

---

## Task 4: Add `clips edit` subcommand

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

### 4.1 — Add variant to `ClipsCommand` (after line 44):

```rust
    /// Interactively edit processor chain for a clip
    Edit {
        slug: String,
    },
```

### 4.2 — Add import at top of file (add to existing use block):

```rust
use super::processor_list_editor::{run as run_editor, SaveFn};
```

### 4.3 — Add match arm to `run()` (inside the `match args.command` block, before the closing brace):

```rust
        ClipsCommand::Edit { slug } => {
            let clip = clip_service::get_clip_by_slug(db, &slug).await?;
            let processors = serde_json::from_str(&clip.processors).unwrap_or_default();

            let save: SaveFn<'_> = Box::new(|procs| {
                Box::pin(clip_service::update_clip_processors(db, library_dir, &slug, procs))
            });

            run_editor(&format!("Clip: {slug}"), processors, save).await?;
        }
```

**Step 4.1** — Run `cargo build -p musicum-cli`. Confirm it compiles.

**Step 4.2** — Smoke-test: `cargo run -p musicum-cli -- clips edit <some-slug>`. Verify the editor opens with the clip's current processors and changes persist after exit.

---

## Task 5: Register module and lint

**Files:**
- Modify: `apps/cli/src/commands/mod.rs`

### 5.1 — Add the new module (alphabetically after `play`):

```rust
pub mod processor_list_editor;
```

The full `mod.rs` after the change:

```rust
pub mod clips;
pub mod collections;
pub mod files;
pub mod play;
pub mod preset_list_editor;   // ← if renamed; otherwise:
pub mod processor_list_editor;
pub mod presets;
pub mod presets_editor;
pub mod processors;
pub mod sync;
```

**Step 5.2** — Run `cargo clippy --all` and fix all warnings before moving on.

**Step 5.3** — Run `cargo test -p musicum-core` to confirm no regressions in the core library.

---

## Task 6: Final validation

**Step 6.1** — Full build: `cargo build --all`. Expected: zero errors, zero warnings.

**Step 6.2** — Full test suite: `cargo test --all`. Expected: all tests pass.

**Step 6.3** — End-to-end preset flow:
```
cargo run -p musicum-cli -- presets edit <slug>
```
1. Editor opens with existing processors
2. Press `a` → picker overlay appears with trim/crop/cut/slice
3. Select "trim" → processor added, status shows "added trim"
4. Navigate to params pane → edit "start" → save → status shows "saved start = 0.5"
5. Press `d` to delete a processor → status shows "deleted processor"
6. Press Shift+Down → reorders → status shows "moved down"
7. Press Space → toggles enabled → status shows "toggled"
8. Press `q` → exits cleanly
9. Run `presets show <slug>` → confirm changes persisted

**Step 6.4** — End-to-end clip flow:
```
cargo run -p musicum-cli -- clips edit <slug>
```
Same checklist as 6.3.

**Step 6.5** — Confirm `clips show <slug>` after editing shows updated processor list.
