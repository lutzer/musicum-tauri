# Collection Player Design

**Date:** 2026-05-29
**Status:** Draft

## Overview

Extend the CLI audio player so that `musicum play --collection <slug>` plays all clips
in a collection in order, with skip-forward and skip-backward navigation between clips.
A new `PlaybackQueue` struct in `musicum-core` manages the clip list and engine lifecycle.
Single-clip play mode becomes a queue of one item — no separate code paths.

---

## PlaybackQueue (`libs/musicum-core/src/audio/queue.rs`)

A `PlaybackQueue` owns the ordered list of clips to play and the active `PlaybackEngine`.
It manages engine creation and teardown as the user navigates between clips.

```rust
pub struct QueueItem {
    pub title: String,
    pub path: String,
    pub edits: Vec<ProcessorEdit>,
}

pub struct PlaybackQueue {
    items: Vec<QueueItem>,
    current_index: usize,
    engine: PlaybackEngine,
    registry: Arc<PluginRegistry>,
}
```

### Key methods

| Method | Behaviour |
|--------|-----------|
| `new(items, registry) -> Result<Self>` | Requires `items` to be non-empty. Creates engine for item 0 and starts playing. |
| `engine(&self) -> &PlaybackEngine` | Exposes the current engine so the TUI can delegate pause/seek/etc. |
| `engine_mut(&mut self) -> &mut PlaybackEngine` | Mutable access for toggle_pause, seek, toggle_loop. |
| `current_index(&self) -> usize` | 0-based index of the clip currently playing. |
| `total(&self) -> usize` | Total number of clips in the queue. |
| `current_title(&self) -> &str` | Title of the current clip. |
| `next(&mut self) -> bool` | Advance to next clip; returns `false` if already at last. Drops old engine, creates new one, starts playing. |
| `prev(&mut self) -> bool` | If current position > 3 s: seek to 0 and return `true`. Otherwise go to previous clip (if any) and return `true`; returns `false` if already at index 0 and position ≤ 3 s (no-op in TUI). |
| `advance_if_finished(&mut self) -> bool` | Called each TUI tick. If `engine.is_finished()` and there is a next clip, advances automatically and returns `true`. Returns `false` when the last clip has finished (queue exhausted — TUI stays open, playback stopped). |

Engine transitions: drop old `PlaybackEngine` (stops its cpal stream + decode thread), then
call `PlaybackEngine::new(path, edits, registry.clone())`. The new engine starts paused = false
(auto-play on skip).

---

## CLI Changes (`apps/cli/src/commands/play.rs`)

### New flag

```
musicum play --collection <slug>
```

Alongside the existing `--file` and `--clip` flags. Resolution logic:

1. `--collection`: call `get_collection_with_clips(slug)`, map each clip to a `QueueItem`
   (resolve each clip's file path via `file_service::get_file_by_id`), build `PlaybackQueue`.
2. `--clip`: same as today — single-item queue.
3. `--file` or auto-resolve: same as today — single-item queue.

`run_player()` signature changes from `(engine: PlaybackEngine, …)` to
`(queue: PlaybackQueue, collection_title: Option<String>, …)`.

### New key bindings

| Key | Action |
|-----|--------|
| `Shift+←` | `queue.prev()` |
| `Shift+→` | `queue.next()` |
| `←` | `engine.seek(pos - 5.0)` — unchanged |
| `→` | `engine.seek(pos + 5.0)` — unchanged |
| `p` | `engine.toggle_pause()` — unchanged |
| `l` | `engine.toggle_loop()` — unchanged |
| `q` / Esc / Ctrl+C | quit — unchanged |

In crossterm, `Shift+←` is `KeyCode::Left` with `KeyModifiers::SHIFT`.

### TUI layout — collection mode

Shown when `collection_title.is_some()` (i.e. `--collection` was used):

```
╔═══════════════════════════════════════╗
║ My Collection                    3/10 ║
║ ▶ Current Clip Title                  ║
║ ████████░░░░░░░░░░░  01:23 / 04:56   ║
║ trim start=1.0s end=10.2s             ║
║ [p] pause  [l] loop  [←/→] seek 5s   ║
║ [S←/S→] prev/next clip   [q] quit    ║
╚═══════════════════════════════════════╝
```

- Row 1: collection title (left) + `{n}/{total}` badge (right, 1-based).
- Row 2: play/pause icon + clip title — same as current single-clip title row.
- Row 3: progress bar — unchanged.
- Row 4: processor summary — unchanged (hidden if no structural edits).
- Row 5: hint bar — base hints.
- Row 6: hint bar — skip hints (only rendered in collection mode).

### TUI layout — single-clip mode

Identical to today: no collection header row, no skip hints row.

### End-of-queue behaviour

When `advance_if_finished()` returns `false` (last clip finished):
- The TUI remains open.
- The title row shows `■ {clip title}` (stopped indicator) instead of `▶` / `⏸`.
- The hint bar still shows `[q] quit`.
- `Shift+←` navigates back to the previous clip (which resumes play from its start).
- All other keys are no-ops (queue is exhausted; no restart-from-beginning).

---

## Data flow

```
musicum play --collection my-mix
      │
      ▼
get_collection_with_clips("my-mix")
      │  returns Collection + Vec<Clip> (ordered by position)
      ▼
for each Clip:
  get_file_by_id(clip.file_id)          → path
  deserialize_processor_edits(clip.processors) → Vec<ProcessorEdit>
  → QueueItem { title, path, edits }
      │
      ▼
PlaybackQueue::new(items, registry)     → engine for item[0], playing
      │
      ▼
run_player(queue, Some("My Mix"), …)    → TUI loop
```

---

## Constraints & non-goals

- **No persistence.** The queue is ephemeral; play order follows collection's `position` field.
- **No shuffle.** Not in scope.
- **No re-ordering in TUI.** The collection's saved order is used as-is.
- **Loop key (`l`) loops the current clip only.** Collection-level looping is not in scope.
- **Structural edits on skip.** Since `PlaybackEngine::new` is called on each skip, structural
  edits always apply correctly — no special handling needed.
