# Processor List Editor — Design Spec

**Date:** 2026-05-25
**Status:** Approved

## Overview

Extend processor editing in the CLI to support full list management (add, remove,
reorder, toggle) in addition to the existing value-editing capability. A shared
editor component handles both presets and clips via a save callback, eliminating
duplication.

## Goals

- Add, delete, and reorder structural processors in a preset or clip
- Toggle processors enabled/disabled
- Open the same editor for clips via `musicum clips edit <slug>`
- Keep all existing parameter-editing behaviour intact

## Non-goals

- Audio plugin support (deferred)
- Interactive clip picker (pass slug directly on CLI)
- Undo/redo
- Confirmation dialogs on delete

---

## Architecture

### Shared editor module

**File:** `apps/cli/src/commands/processor_list_editor.rs`

Public entry point:

```rust
pub fn run(
    title: &str,
    initial_processors: Vec<ProcessorEntry>,
    save: Box<dyn Fn(Vec<ProcessorEntry>) -> Result<()>>,
) -> Result<()>
```

`save` is called after every mutation (add, delete, reorder, toggle, param edit).
The editor is unaware of whether it is editing a preset or a clip; all
persistence knowledge lives in the caller.

### Preset entry point

**File:** `apps/cli/src/commands/presets_editor.rs` (refactored)

Loads the preset by slug, then calls:

```rust
processor_list_editor::run(
    &format!("Preset: {}", slug),
    preset.processors,
    Box::new(move |processors| {
        preset_service::update_preset_processors(&db, &slug, processors)
    }),
)
```

### Clip entry point

**File:** `apps/cli/src/commands/clips.rs` (new `edit` subcommand)

CLI invocation: `musicum clips edit <slug>`

Loads the clip by slug, then calls:

```rust
processor_list_editor::run(
    &format!("Clip: {}", slug),
    clip.processors,
    Box::new(move |processors| {
        clip_service::update_clip_processors(&db, &slug, processors)
    }),
)
```

Errors (clip not found, save failure) print to stderr and exit non-zero,
consistent with other subcommands.

---

## UI Layout

Two-pane layout unchanged (45 % / 55 % split):

- **Left pane — Processors:** list of processor entries (type, UUID prefix,
  enabled indicator)
- **Right pane — Params:** key-value pairs for the selected processor

A **processor picker overlay** appears modally when the user presses `a`. It is
a centered box listing the four available structural processor types with a
one-line description each. Pressing Esc closes it without adding anything.

---

## Keybindings

### Both panes

| Key | Action |
|-----|--------|
| `Tab` | Switch active pane |
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `q` / `Ctrl+C` | Quit |

### Processor pane (active)

| Key | Action |
|-----|--------|
| `a` | Open processor picker overlay |
| `d` | Delete selected processor |
| `Shift+↑` | Move selected processor up in the chain |
| `Shift+↓` | Move selected processor down in the chain |
| `Space` | Toggle enabled/disabled |

### Processor picker overlay

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate processor types |
| `Enter` | Add selected type with default params, close overlay |
| `Esc` | Cancel, close overlay |

### Params pane (active)

| Key | Action |
|-----|--------|
| `Enter` | Enter edit mode for selected param |
| `Esc` | Cancel edit |
| `Enter` (in edit mode) | Confirm edit and save |

---

## Processor Picker

Available structural processor types and their descriptions shown in the overlay:

| ID | Description |
|----|-------------|
| `trim` | Remove time from start and/or end |
| `crop` | Keep only a section between two points |
| `cut` | Remove a middle section, concatenating before and after |
| `slice` | Divide into N equal slices and select one |

Default parameter values are read from each processor's `ProcessorDescriptor`
at runtime. The new entry is inserted after the currently selected processor (or
at position 0 if the list is empty). A new UUID is generated for the instance
`id` field.

---

## Save Behaviour

`save(processors)` is called after every mutation:

- Adding a processor
- Deleting a processor
- Reordering (each move triggers one save call)
- Toggling enabled
- Confirming a param edit

This keeps DB and sidecar always in sync with the in-memory state. The editor
does not buffer changes; each action is immediately persisted.

---

## Services

`clip_service::update_clip_processors` already exists. No new service methods
are required.

`preset_service::update_preset_processors` already exists.

The only service-layer change needed is to confirm that `update_clip_processors`
writes to both the DB `processors` column and the clip's `.musicum.json` sidecar
file. If it currently only updates the DB, add the sidecar write to match the
preset equivalent.

---

## Files Changed

| File | Change |
|------|--------|
| `apps/cli/src/commands/processor_list_editor.rs` | New — shared editor |
| `apps/cli/src/commands/presets_editor.rs` | Refactored to thin wrapper |
| `apps/cli/src/commands/clips.rs` | Add `edit` subcommand |
| `apps/cli/src/commands/mod.rs` | Register new module |
| `apps/cli/src/main.rs` | Wire `clips edit` into CLI arg parser |
