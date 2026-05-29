# Collection Player Implementation Plan

**Goal:** Add `musicum play --collection <slug>` to play all clips in a collection in order, with skip-forward/skip-backward navigation.

**Architecture:** A new `PlaybackQueue` struct in `musicum-core` wraps an ordered `Vec<QueueItem>` and the active `PlaybackEngine`; engine teardown/creation on skip handles all lifecycle. Single-clip play becomes a one-item queue — no separate code paths. The CLI `run_player` function accepts a `PlaybackQueue` instead of a bare `PlaybackEngine`.

**Tech Stack:** Rust, ratatui 0.29, crossterm (KeyModifiers::SHIFT), cpal/symphonia via existing `PlaybackEngine`, SeaORM for collection/clip queries.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| **Create** | `libs/musicum-core/src/audio/queue.rs` | `QueueItem`, `PlaybackQueue` struct + all navigation logic |
| **Modify** | `libs/musicum-core/src/audio/mod.rs` | `pub mod queue` + re-export `PlaybackQueue`, `QueueItem` |
| **Modify** | `libs/musicum-core/src/lib.rs` | Re-export `PlaybackQueue`, `QueueItem` at crate root |
| **Modify** | `apps/cli/src/commands/play.rs` | New `--collection` flag, queue construction, updated TUI |
| **Modify** | `apps/cli/src/main.rs` | Add `collection` arg to `Play` subcommand |

---

## Task 1: `QueueItem` and `PlaybackQueue` — skeleton + constructor

**Files:**
- Create: `libs/musicum-core/src/audio/queue.rs`

### Step 1.1 — Write a failing test for `PlaybackQueue::new`

Add to the bottom of the new file (don't implement the struct yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::registry::EditRegistry;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    fn temp_wav(frames: usize, sample_rate: u32) -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let spec = WavSpec { channels: 1, sample_rate, bits_per_sample: 32,
                             sample_format: SampleFormat::Float };
        let mut w = WavWriter::create(tmp.path(), spec).unwrap();
        for i in 0..frames { w.write_sample(i as f32 / frames as f32).unwrap(); }
        w.finalize().unwrap();
        tmp
    }

    #[test]
    fn new_single_item_sets_index_zero() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem {
            title: "track".to_string(),
            path: tmp.path().to_str().unwrap().to_string(),
            edits: vec![],
        }];
        let queue = PlaybackQueue::new(items, registry).unwrap();
        assert_eq!(queue.current_index(), 0);
        assert_eq!(queue.total(), 1);
        assert_eq!(queue.current_title(), "track");
    }

    #[test]
    fn new_empty_items_returns_error() {
        let registry = Arc::new(EditRegistry::default());
        let result = PlaybackQueue::new(vec![], registry);
        assert!(result.is_err());
    }
}
```

### Step 1.2 — Run the test; confirm it does not compile

```
cargo test -p musicum-core audio::queue 2>&1 | head -20
```

Expected: compile error `cannot find … QueueItem` / `PlaybackQueue`.

### Step 1.3 — Implement the struct and `new`

```rust
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::audio::player::PlaybackEngine;
use crate::audio::registry::EditRegistry;
use crate::edit::ProcessorEdit;

pub struct QueueItem {
    pub title: String,
    pub path:  String,
    pub edits: Vec<ProcessorEdit>,
}

pub struct PlaybackQueue {
    items:         Vec<QueueItem>,
    current_index: usize,
    engine:        PlaybackEngine,
    registry:      Arc<EditRegistry>,
}

impl PlaybackQueue {
    pub fn new(items: Vec<QueueItem>, registry: Arc<EditRegistry>) -> Result<Self> {
        if items.is_empty() {
            return Err(anyhow!("PlaybackQueue requires at least one item"));
        }
        let engine = PlaybackEngine::new(
            Path::new(&items[0].path),
            &items[0].edits,
            &registry,
        )?;
        engine.play();
        Ok(Self { items, current_index: 0, engine, registry })
    }

    pub fn engine(&self)     -> &PlaybackEngine     { &self.engine }
    pub fn engine_mut(&mut self) -> &mut PlaybackEngine { &mut self.engine }
    pub fn current_index(&self) -> usize             { self.current_index }
    pub fn total(&self)      -> usize                { self.items.len() }
    pub fn current_title(&self) -> &str              { &self.items[self.current_index].title }
}
```

### Step 1.4 — Run the tests; confirm they pass

```
cargo test -p musicum-core audio::queue::tests::new_single_item_sets_index_zero audio::queue::tests::new_empty_items_returns_error
```

Expected: `test result: ok. 2 passed`.

---

## Task 2: `next`, `prev`, `advance_if_finished`

**Files:**
- Modify: `libs/musicum-core/src/audio/queue.rs`

### Step 2.1 — Add failing tests

```rust
    #[test]
    fn next_advances_index() {
        let tmp1 = temp_wav(4410, 44_100);
        let tmp2 = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![
            QueueItem { title: "a".to_string(), path: tmp1.path().to_str().unwrap().to_string(), edits: vec![] },
            QueueItem { title: "b".to_string(), path: tmp2.path().to_str().unwrap().to_string(), edits: vec![] },
        ];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        let moved = queue.next();
        assert!(moved);
        assert_eq!(queue.current_index(), 1);
        assert_eq!(queue.current_title(), "b");
    }

    #[test]
    fn next_at_last_returns_false() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem { title: "only".to_string(),
                                     path: tmp.path().to_str().unwrap().to_string(),
                                     edits: vec![] }];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        assert!(!queue.next());
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn prev_at_start_with_low_position_returns_false() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem { title: "only".to_string(),
                                     path: tmp.path().to_str().unwrap().to_string(),
                                     edits: vec![] }];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        // position is 0, index is 0: no-op
        assert!(!queue.prev());
    }

    #[test]
    fn prev_at_index_1_moves_back() {
        let tmp1 = temp_wav(4410, 44_100);
        let tmp2 = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![
            QueueItem { title: "a".to_string(), path: tmp1.path().to_str().unwrap().to_string(), edits: vec![] },
            QueueItem { title: "b".to_string(), path: tmp2.path().to_str().unwrap().to_string(), edits: vec![] },
        ];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        queue.next();
        let moved = queue.prev();
        assert!(moved);
        assert_eq!(queue.current_index(), 0);
    }
```

### Step 2.2 — Run; confirm compile/test failures

```
cargo test -p musicum-core audio::queue 2>&1 | tail -20
```

### Step 2.3 — Implement `next`, `prev`, `advance_if_finished`

Add to the `impl PlaybackQueue` block:

```rust
    pub fn next(&mut self) -> bool {
        if self.current_index + 1 >= self.items.len() {
            return false;
        }
        self.current_index += 1;
        self.replace_engine();
        true
    }

    /// If current position > 3 s: seek to 0.
    /// Otherwise go to previous clip if any; if already at 0 with low position: no-op (false).
    pub fn prev(&mut self) -> bool {
        if self.engine.position_secs() > 3.0 {
            self.engine.seek(0.0);
            return true;
        }
        if self.current_index == 0 {
            return false;
        }
        self.current_index -= 1;
        self.replace_engine();
        true
    }

    /// Call once per TUI tick. Returns `true` if the engine was advanced to the next clip.
    /// Returns `false` when the last clip has finished (queue exhausted).
    pub fn advance_if_finished(&mut self) -> bool {
        if !self.engine.is_finished() {
            return false;
        }
        if self.current_index + 1 >= self.items.len() {
            return false;          // last clip done — caller keeps TUI open
        }
        self.current_index += 1;
        self.replace_engine();
        true
    }

    fn replace_engine(&mut self) {
        let item = &self.items[self.current_index];
        if let Ok(eng) = PlaybackEngine::new(Path::new(&item.path), &item.edits, &self.registry) {
            eng.play();
            self.engine = eng;
        }
    }
```

### Step 2.4 — Run all queue tests; confirm they pass

```
cargo test -p musicum-core audio::queue
```

Expected: `test result: ok. 6 passed`.

---

## Task 3: Wire `queue` module into `musicum-core`

**Files:**
- Modify: `libs/musicum-core/src/audio/mod.rs`
- Modify: `libs/musicum-core/src/lib.rs`

### Step 3.1 — Add `pub mod queue` + re-export to `audio/mod.rs`

In `libs/musicum-core/src/audio/mod.rs`, add after `pub mod player;`:

```rust
pub mod queue;
```

And after `pub use player::PlaybackEngine;`:

```rust
pub use queue::{PlaybackQueue, QueueItem};
```

### Step 3.2 — Re-export at crate root in `lib.rs`

Change the `pub use audio::{…}` line to include `PlaybackQueue` and `QueueItem`:

```rust
pub use audio::{structural_edits_from, EditEntry, EditRegistry, EditType, ParamInfo,
                PlaybackEngine, PlaybackQueue, QueueItem};
```

### Step 3.3 — Confirm the library builds

```
cargo build -p musicum-core
```

Expected: no errors.

---

## Task 4: Add `--collection` flag and queue construction to the CLI

**Files:**
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/commands/play.rs`

### Step 4.1 — Add `collection` arg to `Play` subcommand in `main.rs`

Current block (lines 38–50):

```rust
    Play {
        /// Slug or file path to play
        target: String,
        /// Resolve target as a file slug (skips clip lookup)
        #[arg(long, conflicts_with = "clip")]
        file: bool,
        /// Resolve target as a clip slug (skips file lookup)
        #[arg(long, conflicts_with = "file")]
        clip: bool,
        /// Start playback with looping enabled
        #[arg(long = "loop")]
        loop_mode: bool,
    },
```

Replace with:

```rust
    Play {
        /// Slug or file path to play (omit when using --collection)
        target: Option<String>,
        /// Play all clips in a collection by slug
        #[arg(long, conflicts_with_all = ["file", "clip"])]
        collection: Option<String>,
        /// Resolve target as a file slug (skips clip lookup)
        #[arg(long, conflicts_with = "clip")]
        file: bool,
        /// Resolve target as a clip slug (skips file lookup)
        #[arg(long, conflicts_with = "file")]
        clip: bool,
        /// Start playback with looping enabled
        #[arg(long = "loop")]
        loop_mode: bool,
    },
```

### Step 4.2 — Update the `Play` match arm in `main.rs`

Find the dispatch (around line 86):

```rust
            Commands::Play { target, file, clip, loop_mode } => {
                commands::play::run(&db, target, file, clip, loop_mode).await?
            }
```

Replace with:

```rust
            Commands::Play { target, collection, file, clip, loop_mode } => {
                commands::play::run(&db, target, collection, file, clip, loop_mode).await?
            }
```

### Step 4.3 — Update `play.rs`: imports and `run` signature

Add `collection_service` and `PlaybackQueue`/`QueueItem` to the imports:

```rust
use musicum_core::{
    audio::structural_edits_from,
    deserialize_processor_edits,
    edit::ProcessorEdit,
    EditRegistry, PlaybackEngine, PlaybackQueue, QueueItem, StructuralEdit,
    services::{clip_service, collection_service, file_service},
};
use std::sync::Arc;
```

Change `run` signature:

```rust
pub async fn run(
    db: &DatabaseConnection,
    target: Option<String>,
    collection: Option<String>,
    force_file: bool,
    force_clip: bool,
    loop_mode: bool,
) -> Result<()> {
```

### Step 4.4 — Implement collection branch in `run`

Replace the current `run` body with:

```rust
pub async fn run(
    db: &DatabaseConnection,
    target: Option<String>,
    collection: Option<String>,
    force_file: bool,
    force_clip: bool,
    loop_mode: bool,
) -> Result<()> {
    let registry = Arc::new(EditRegistry::default());

    if let Some(slug) = collection {
        let (col, clips) = collection_service::get_collection_with_clips(db, &slug)
            .await
            .map_err(|_| anyhow!("no collection with slug '{slug}'"))?;

        if clips.is_empty() {
            return Err(anyhow!("collection '{slug}' has no clips"));
        }

        let mut items: Vec<QueueItem> = Vec::with_capacity(clips.len());
        for clip in &clips {
            let file = file_service::get_file_by_id(db, &clip.file_id)
                .await
                .map_err(|_| anyhow!("file not found for clip '{}'", clip.slug))?;
            let edits = deserialize_processor_edits(&clip.processors);
            items.push(QueueItem {
                title: clip.title.clone(),
                path:  file.path.clone(),
                edits,
            });
        }

        let queue = PlaybackQueue::new(items, Arc::clone(&registry))?;
        if loop_mode { queue.engine().toggle_loop(); }
        return run_player(queue, Some(col.title));
    }

    let target = target.ok_or_else(|| anyhow!("provide a target or --collection <slug>"))?;
    let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
    let structural = structural_edits_from(&edits);
    let processor_display = format_processor_display(&structural);

    let item = QueueItem {
        title: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        path:  path.to_string_lossy().to_string(),
        edits,
    };
    let queue = PlaybackQueue::new(vec![item], Arc::clone(&registry))?;
    if loop_mode { queue.engine().toggle_loop(); }
    run_player(queue, None)
}
```

Note: `processor_display` is now computed from edits before building `QueueItem`. Pass it to `run_player` alongside the optional collection title. Adjust the signature accordingly (next step).

### Step 4.5 — Confirm the crate builds (errors expected in `run_player`)

```
cargo build -p musicum-cli 2>&1 | head -30
```

---

## Task 5: Update `run_player` and `draw` for queue + collection TUI

**Files:**
- Modify: `apps/cli/src/commands/play.rs`

### Step 5.1 — Update `run_player` signature and loop

Replace `fn run_player(engine: PlaybackEngine, processor_display: String) -> Result<()>` with:

```rust
fn run_player(mut queue: PlaybackQueue, collection_title: Option<String>) -> Result<()> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());

    // Height: base 3 rows + 1 if processors + 1 collection header + 1 skip hints (collection mode)
    let in_collection = collection_title.is_some();
    let base_height = 3u16
        + u16::from(!compute_processor_display(queue.engine()).is_empty())
        + u16::from(in_collection)    // collection header row
        + u16::from(in_collection);  // skip hints row

    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions { viewport: Viewport::Inline(base_height) },
    )?;

    let mut queue_exhausted = false;

    loop {
        let processor_display = compute_processor_display(queue.engine());
        terminal.draw(|f| draw(f, &queue, &collection_title, &processor_display, queue_exhausted))?;

        if !queue_exhausted && queue.advance_if_finished() {
            // engine replaced; loop continues
        }
        if !queue_exhausted && queue.engine().is_finished() && queue.current_index() + 1 >= queue.total() {
            queue_exhausted = true;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ if queue_exhausted => {}   // all other keys are no-ops when exhausted
                    (KeyCode::Char('p'), _) => queue.engine_mut().toggle_pause(),
                    (KeyCode::Char('l'), _) => queue.engine_mut().toggle_loop(),
                    (KeyCode::Right, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek(pos + 5.0);
                    }
                    (KeyCode::Left, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek((pos - 5.0).max(0.0));
                    }
                    (KeyCode::Right, KeyModifiers::SHIFT) => { queue.next(); }
                    (KeyCode::Left,  KeyModifiers::SHIFT) => {
                        if queue.prev() && queue_exhausted {
                            queue_exhausted = false;  // reactivate after navigating back
                        }
                    }
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

Add the helper that extracts processor display from the current engine's edits (needed because single-clip path no longer passes `processor_display` separately):

```rust
fn compute_processor_display(engine: &PlaybackEngine) -> String {
    // PlaybackEngine exposes no edits list; the display is built in `run` for the single-clip
    // path. For collection mode, structural edits are already baked into each engine.
    // This stub returns empty — the structural summary row is hidden in collection mode.
    // TODO: expose structural snapshot from PlaybackEngine if needed.
    String::new()
}
```

> **Note on `processor_display` in collection vs single-clip mode:**
> The spec says the processor row is unchanged. However `PlaybackEngine` does not expose its structural edit list publicly. The simplest correct approach is:
> - Single-clip mode: compute `processor_display` before constructing `QueueItem`, store it in a wrapper, and pass it through.
> - Collection mode: omit the processor row (set display to empty string).
>
> Revise `run_player` to accept an `Option<String>` for processor_display:

```rust
fn run_player(
    mut queue: PlaybackQueue,
    collection_title: Option<String>,
    processor_display: String,    // empty in collection mode
) -> Result<()> {
```

And update both call sites in `run`:
- Collection path: `run_player(queue, Some(col.title), String::new())`
- Single-clip path: `run_player(queue, None, processor_display)` (using `processor_display` computed earlier from edits)

### Step 5.2 — Update `draw` to handle queue + collection header

Replace `fn draw(f: &mut Frame, engine: &PlaybackEngine, processor_display: &str)` with:

```rust
fn draw(
    f: &mut Frame,
    queue: &PlaybackQueue,
    collection_title: &Option<String>,
    processor_display: &str,
    queue_exhausted: bool,
) {
    let engine = queue.engine();
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();
    let in_collection = collection_title.is_some();

    let show_processors = !processor_display.is_empty();
    let mut constraints = Vec::new();
    if in_collection  { constraints.push(Constraint::Length(1)); } // collection header
    constraints.push(Constraint::Length(1)); // title / status
    constraints.push(Constraint::Length(1)); // progress bar
    if show_processors { constraints.push(Constraint::Length(1)); } // processor summary
    constraints.push(Constraint::Length(1)); // base hints
    if in_collection  { constraints.push(Constraint::Length(1)); } // skip hints

    let areas = Layout::vertical(constraints).split(f.area());
    let mut area_idx = 0usize;

    // Row: collection header (collection mode only)
    if let Some(title) = collection_title {
        let badge = format!("{}/{}", queue.current_index() + 1, queue.total());
        let header = Line::from(vec![
            Span::styled(title.as_str(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(badge, Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(header), areas[area_idx]);
        area_idx += 1;
    }

    // Row: clip title + play state
    let (status_icon, status_color) = if queue_exhausted {
        ("■", Color::DarkGray)
    } else if paused {
        ("⏸", Color::Yellow)
    } else {
        ("▶", Color::Green)
    };
    let title_line = Line::from(vec![
        Span::styled(status_icon, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(engine.title(), Style::default().add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(title_line), areas[area_idx]);
    area_idx += 1;

    // Row: progress bar
    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{} / {}", fmt_duration(pos), fmt_duration(dur))),
        areas[area_idx],
    );
    area_idx += 1;

    // Row: processor summary (optional)
    if show_processors {
        f.render_widget(
            Paragraph::new(processor_display).style(Style::default().fg(Color::DarkGray)),
            areas[area_idx],
        );
        area_idx += 1;
    }

    // Row: base hints
    let loop_color = if engine.is_looping() { Color::Cyan } else { Color::DarkGray };
    let base_hints = Line::from(vec![
        Span::styled("[p] pause  ", Style::default().fg(Color::DarkGray)),
        Span::styled("[l] loop  ", Style::default().fg(loop_color)),
        Span::styled("[←/→] seek 5s  [q] quit", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(base_hints), areas[area_idx]);
    area_idx += 1;

    // Row: skip hints (collection mode only)
    if in_collection {
        let skip_hints = Line::from(vec![
            Span::styled("[S←/S→] prev/next clip", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(skip_hints), areas[area_idx]);
    }
}
```

### Step 5.3 — Build; confirm no errors

```
cargo build -p musicum-cli
```

Expected: clean build.

### Step 5.4 — Lint

```
cargo clippy --all 2>&1 | grep "^error" | head -20
```

Expected: no errors (warnings OK).

---

## Task 6: Integration smoke test (manual)

No automated test for the TUI loop is feasible. Perform these manual checks:

### Step 6.1 — Single-clip play still works

```
cargo run -p musicum-cli -- play <any-clip-slug>
```

Verify: TUI opens, `▶` icon, progress bar moves, `[p]` pauses, `[q]` quits. No collection header, no skip-hints row.

### Step 6.2 — Collection play works

Create a test collection with at least two clips:

```
cargo run -p musicum-cli -- collections add my-test-mix
cargo run -p musicum-cli -- collections add-clip my-test-mix <clip-slug-1>
cargo run -p musicum-cli -- collections add-clip my-test-mix <clip-slug-2>
cargo run -p musicum-cli -- play --collection my-test-mix
```

Verify:
- Collection title row shown, badge reads `1/2`.
- Skip-hints row present.
- `Shift+→` skips to clip 2, badge updates to `2/2`.
- `Shift+←` on clip 2 at position ≤ 3 s goes back to clip 1.
- After clip 2 finishes, badge stays `2/2`, title shows `■ clip-title`, all non-quit keys are no-ops.
- `Shift+←` from exhausted state navigates back to clip 1 and resumes play.
- `[q]` quits.

### Step 6.3 — `--collection` with non-existent slug returns error

```
cargo run -p musicum-cli -- play --collection does-not-exist
```

Expected: `Error: no collection with slug 'does-not-exist'`.

### Step 6.4 — `--collection` on empty collection returns error

```
cargo run -p musicum-cli -- collections add empty-mix
cargo run -p musicum-cli -- play --collection empty-mix
```

Expected: `Error: collection 'empty-mix' has no clips`.

---

## Task 7: Run full test suite

```
cargo test -p musicum-core
```

Expected: all existing tests pass plus the 6 new queue tests.

```
cargo clippy --all
```

Expected: no new errors.

---

**Plan complete and saved to `docs/plans/2026-05-29-collection-player.md`.**

- **REQUIRED SUB-SKILL:** Use execute-plan
- Batch execution with checkpoints for review

ARGUMENTS: docs/plans/2026-05-29-collection-player.md
