# Structural Processor Pipeline — CLI Playback Implementation Plan

**Goal:** Wire the clip's stored structural processors into the CLI player so they alter the live audio stream and are shown in the player TUI.

**Architecture:** The `PlaybackEngine` already accepts `&[Edit]` and runs `build_chain` in its decode loop — the SDK is complete. The only change is in `play.rs`: rename `resolve_path` → `resolve_target` to also return edits (converted from the clip's sidecar `ProcessorEntry` list), pass them to `PlaybackEngine::new`, and render an optional 4th TUI row showing active processor params.

**Tech Stack:** Rust, ratatui 0.29, `structural_processor_sdk::chain::Edit`, `musicum_core::sidecar::ProcessorEntry`

---

## Files

| Action | File |
|---|---|
| Modify | `apps/cli/src/commands/play.rs` |

No other files change.

---

### Task 1: Add imports and two pure helper functions

`sidecar_entries_to_edits` converts sidecar processor entries to SDK edits.  
`format_processor_display` turns those edits into a human-readable string for the TUI.

Both are pure functions — write them first so they can be tested in isolation.

**Step 1.1 — Add imports at the top of `play.rs`**

Open `apps/cli/src/commands/play.rs`. The existing imports block ends around line 18. Add two new imports:

```rust
use musicum_core::sidecar::ProcessorEntry;
use structural_processor_sdk::chain::Edit;
```

Full imports block should look like:

```rust
use std::{io, path::PathBuf, time::Duration};

use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use musicum_core::{
    audio::PlaybackEngine,
    services::{clip_service, file_service},
    sidecar::ProcessorEntry,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Gauge, Paragraph},
    Frame, Terminal, TerminalOptions, Viewport,
};
use sea_orm::DatabaseConnection;
use structural_processor_sdk::chain::Edit;
```

**Step 1.2 — Write `sidecar_entries_to_edits`**

Add this private function anywhere in `play.rs` (e.g., after the imports):

```rust
fn sidecar_entries_to_edits(entries: &[ProcessorEntry]) -> Vec<Edit> {
    entries
        .iter()
        .filter_map(|e| {
            if let ProcessorEntry::Structural { enabled, processor, .. } = e {
                let params = processor
                    .params
                    .as_object()
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Edit { processor_id: processor.id.clone(), enabled: *enabled, params })
            } else {
                None
            }
        })
        .collect()
}
```

**Step 1.3 — Write `format_processor_display`**

Add this private function directly after `sidecar_entries_to_edits`:

```rust
fn format_processor_display(edits: &[Edit]) -> String {
    edits
        .iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let mut params: Vec<String> = e
                .params
                .iter()
                .map(|(k, v)| format!("{}={:.2}s", k, v))
                .collect();
            params.sort();
            format!("{} {}", e.processor_id, params.join(" "))
        })
        .collect::<Vec<_>>()
        .join("  ")
}
```

**Step 1.4 — Build to confirm the helpers compile**

```
cargo build -p musicum-cli
```

Expected: compiles with no errors (the functions are dead code for now — that's fine).

---

### Task 2: Update `resolve_path` → `resolve_target`

Change the function to return `(PathBuf, Vec<Edit>)` so callers get both the file path and the clip's processor edits.

**Step 2.1 — Replace the entire `resolve_path` function**

Find the current `resolve_path` function (lines ~29-62) and replace it with:

```rust
async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<Edit>)> {
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
        let entries: Vec<ProcessorEntry> = serde_json::from_str(&clip.processors)
            .unwrap_or_default();
        let edits = sidecar_entries_to_edits(&entries);
        return Ok((PathBuf::from(file.path), edits));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok((PathBuf::from(file.path), vec![]));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            let entries: Vec<ProcessorEntry> = serde_json::from_str(&clip.processors)
                .unwrap_or_default();
            let edits = sidecar_entries_to_edits(&entries);
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

Note: `serde_json::from_str(...).unwrap_or_default()` means a corrupt `processors` JSON column silently falls back to no processors, which is the right trade-off (playback still works).

**Step 2.2 — Update `run()` to use `resolve_target`**

Replace the existing `run` function:

```rust
pub async fn run(db: &DatabaseConnection, target: String, force_file: bool, force_clip: bool) -> Result<()> {
    let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
    let processor_display = format_processor_display(&edits);
    let engine = PlaybackEngine::new(&path, &edits)?;
    engine.play();
    run_player(engine, processor_display)
}
```

**Step 2.3 — Build to confirm**

```
cargo build -p musicum-cli
```

Expected: compiler error on `run_player` signature mismatch (it still takes no `processor_display` arg). That's expected — fix it in Task 3.

---

### Task 3: Update `run_player` and `draw` for the conditional 4th row

**Step 3.1 — Replace `run_player`**

Find the existing `run_player` function (currently takes only `engine`) and replace with:

```rust
fn run_player(engine: PlaybackEngine, processor_display: String) -> Result<()> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let height = if processor_display.is_empty() { 3 } else { 4 };
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions { viewport: Viewport::Inline(height) },
    )?;

    loop {
        terminal.draw(|f| draw(f, &engine, &processor_display))?;

        if engine.is_finished() {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('p'), _) => engine.toggle_pause(),
                    (KeyCode::Right, _) => engine.seek(engine.position_secs() + 5.0),
                    (KeyCode::Left, _) => engine.seek((engine.position_secs() - 5.0).max(0.0)),
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
```

Key change: `PLAYER_HEIGHT` constant is gone; `height` is computed inline from whether `processor_display` is empty.

**Step 3.2 — Replace `draw`**

Find the existing `draw` function and replace with:

```rust
fn draw(f: &mut Frame, engine: &PlaybackEngine, processor_display: &str) {
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();

    let show_processors = !processor_display.is_empty();
    let constraints = if show_processors {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    };

    let areas = Layout::vertical(constraints).split(f.area());

    let (status_text, status_color) = if paused {
        ("⏸  Paused", Color::Yellow)
    } else {
        ("▶  Playing", Color::Green)
    };
    let header = Line::from(vec![
        Span::styled(engine.title(), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);
    f.render_widget(Paragraph::new(header), areas[0]);

    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{} / {}", fmt_duration(pos), fmt_duration(dur))),
        areas[1],
    );

    if show_processors {
        f.render_widget(
            Paragraph::new(processor_display)
                .style(Style::default().fg(Color::DarkGray)),
            areas[2],
        );
        f.render_widget(
            Paragraph::new("[p] pause  [←/→] seek 5s  [q] quit")
                .alignment(Alignment::Left)
                .style(Style::default().fg(Color::DarkGray)),
            areas[3],
        );
    } else {
        f.render_widget(
            Paragraph::new("[p] pause  [←/→] seek 5s  [q] quit")
                .alignment(Alignment::Left)
                .style(Style::default().fg(Color::DarkGray)),
            areas[2],
        );
    }
}
```

**Step 3.3 — Remove the old `PLAYER_HEIGHT` constant**

Delete the line:

```rust
const PLAYER_HEIGHT: u16 = 3;
```

**Step 3.4 — Build to confirm everything compiles**

```
cargo build -p musicum-cli
```

Expected: clean build, no warnings about unused variables.

**Step 3.5 — Run clippy**

```
cargo clippy --all
```

Expected: no new warnings.

---

### Task 4: Manual smoke test

**Step 4.1 — Test raw file playback (no processors, 3-row layout)**

```
cargo run -p musicum-cli -- play <any-file-slug-or-path>
```

Expected: 3-row player (title + gauge + hints), plays audio normally.

**Step 4.2 — Test clip with no processors (still 3 rows)**

```
cargo run -p musicum-cli -- play --clip <clip-slug-with-no-processors>
```

Expected: 3-row player, audio unchanged.

**Step 4.3 — Add a trim processor to a clip, then play it**

First add a trim processor (remove 0.5s from start):

```
cargo run -p musicum-cli -- presets add-processor <some-preset> trim
# Or use the clip's own processor list via the presets editor if available
```

If clip processor editing isn't wired to the CLI yet, edit the sidecar JSON directly:

Open the `.musicum.json` sidecar for the parent audio file and add to the clip's `processors` array:

```json
{
  "type": "structural",
  "id": "test-0000-0000-0000-000000000001",
  "enabled": true,
  "processor": {
    "id": "trim",
    "params": { "start": 0.5, "end": 0.0 }
  }
}
```

Then sync and play:

```
cargo run -p musicum-cli -- sync
cargo run -p musicum-cli -- play --clip <clip-slug>
```

Expected:
- 4-row player (title + gauge + `trim start=0.50s end=0.00s` + hints)
- Audio starts 0.5s into the original file (trim applied)
- Duration in gauge is 0.5s shorter than the raw file

**Step 4.4 — Test seek with processor active**

While the player is open (from step 4.3), press `→` to seek forward 5s.

Expected: audio seeks correctly within the trimmed clip's time space (no crash, no stutter beyond normal seek gap).

**Step 4.5 — Test disabled processor is not shown**

Set `"enabled": false` on the trim entry in the sidecar, sync, play.

Expected: 3-row player (processor row absent), audio plays from beginning (processor not applied).

---

## Done

Plan complete and saved to `docs/plans/2026-05-25-structural-processor-pipeline-playback.md`.

- **REQUIRED SUB-SKILL:** Use `/execute-plan` to implement this plan with review checkpoints.
