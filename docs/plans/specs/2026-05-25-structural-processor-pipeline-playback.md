# Structural Processor Pipeline — CLI Playback Integration

**Date:** 2026-05-25
**Status:** Approved

## Background

The `PlaybackEngine` in `musicum-core/src/audio/player.rs` already accepts `&[Edit]` and runs `build_chain` in its decode loop. The structural processor SDK is fully implemented. The gap: `apps/cli/src/commands/play.rs` always passes `&[]`, so clip processors are never applied during playback.

## Goal

Wire the clip's stored structural processors into the player so they alter the live audio stream, and show the active processors in the player TUI.

## Scope

**One file changes:** `apps/cli/src/commands/play.rs`

No changes to `musicum-core`, the structural processor SDK, or any processor crates.

---

## Data Flow

`resolve_path` is renamed `resolve_target` and returns `(PathBuf, Vec<Edit>)` instead of just `PathBuf`.

| Target type | Path | Edits |
|---|---|---|
| File slug or literal path | File path as before | `vec![]` |
| Clip slug | Parent file path as before | Converted from clip processors |

Conversion from `clip.processors` (a JSON `String` in the DB) to `Vec<Edit>`:

1. Deserialize as `Vec<sidecar::ProcessorEntry>`.
2. Filter to `ProcessorEntry::Structural` variants only (skip `AudioPlugin`).
3. For each, construct `Edit { processor_id: processor.id, enabled, params }` where `params` is built by iterating the `processor.params` JSON object and coercing each value to `f64` via `Value::as_f64()` — non-numeric values are silently skipped.

The edits are passed to `PlaybackEngine::new(&path, &edits)` instead of `&[]`.

---

## Player TUI

### Layout — with active processors (height = 4)

```
my-clip.wav  ▶  Playing
████████████████░░░░░░░░  00:12 / 00:45
trim start=0.20s end=0.00s  cut from=1.00s to=2.50s
[p] pause  [←/→] seek 5s  [q] quit
```

### Layout — without processors (height = 3)

```
my-file.wav  ▶  Playing
████████████████░░░░░░░░  00:12 / 00:45
[p] pause  [←/→] seek 5s  [q] quit
```

- `PLAYER_HEIGHT` is computed at call time: 4 when the processor display string is non-empty, 3 otherwise.
- The processor row is dim gray, no prefix label.
- Only **enabled** processors are shown (disabled entries are excluded during conversion).
- All param values formatted as `{:.2}s` — correct for all current processors (trim, cut, crop, slice all have time params).
- The display string is computed once in `run()` before `run_player` is called.

---

## Implementation

### New helpers in `play.rs`

```rust
fn sidecar_entries_to_edits(entries: &[ProcessorEntry]) -> Vec<Edit> {
    entries.iter().filter_map(|e| {
        if let ProcessorEntry::Structural { enabled, processor, .. } = e {
            let params = processor.params.as_object()
                .map(|obj| obj.iter()
                    .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                    .collect())
                .unwrap_or_default();
            Some(Edit { processor_id: processor.id.clone(), enabled: *enabled, params })
        } else { None }
    }).collect()
}

fn format_processor_display(edits: &[Edit]) -> String {
    edits.iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let mut params: Vec<String> = e.params.iter()
                .map(|(k, v)| format!("{}={:.2}s", k, v))
                .collect();
            params.sort(); // stable display order
            format!("{} {}", e.processor_id, params.join(" "))
        })
        .collect::<Vec<_>>()
        .join("  ")
}
```

### Updated signatures

```rust
async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<Edit>)>

fn run_player(engine: PlaybackEngine, processor_display: String) -> Result<()>

fn draw(f: &mut Frame, engine: &PlaybackEngine, processor_display: &str)
```

### `run()` changes

```rust
let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
let processor_display = format_processor_display(&edits);
let engine = PlaybackEngine::new(&path, &edits)?;
engine.play();
run_player(engine, processor_display)
```

### New imports

```rust
use musicum_core::sidecar::ProcessorEntry;
use structural_processor_sdk::chain::Edit;
```

---

## Out of Scope

- Applying a preset chain during playback (no `--preset` flag).
- Registry-based param type formatting (Time vs Int suffix distinction) — all current processor params are time values, so `{:.2}s` is correct for all.
- Audio plugin chain in the player — separate spec.
