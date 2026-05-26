# Sync Verbose Output — Design Spec

## Problem

The `sync` command prints a single summary line (`added: X, updated: Y, removed: Z`) that only counts audio file changes. Two categories of change are completely silent:

1. **Sidecar-only updates** — when an audio file's hash is unchanged but its `.musicum.json` sidecar has new or modified metadata/clips, the DB is updated but nothing is reported.
2. **Preset sync results** — `sync_presets` inserts and updates presets with no output at all.

## Goal

Per-item verbose output showing every meaningful change, followed by a compact summary line.

## Data model

Replace `SyncStats` in `libs/musicum-core/src/services/sync_service.rs` with:

```rust
pub struct SyncReport {
    pub files_added:      Vec<String>,  // file stem (display name)
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,  // hash unchanged, sidecar data differed
    pub presets_added:    Vec<String>,  // preset title
    pub presets_updated:  Vec<String>,
}
```

`sync_library` returns `Result<SyncReport, ServiceError>`.

## Change detection

### Sidecar changes

`upsert_file_metadata` and `upsert_clips` each return `Result<bool, ServiceError>` — `true` if the data written to the DB differed from what was already there.

- `upsert_file_metadata`: compare bpm, key, rating, color, notes, tags against the existing DB record before upserting.
- `upsert_clips`: changed if any clip slug is new, or if an existing clip's title, processors, or notes differ from the sidecar.

In `upsert_file`, when `existing_model.hash == hash`, call both helpers. If either returns `true`, push the file's display name to `report.sidecars_updated`.

### Preset changes

`sync_presets` accepts `&mut SyncReport`. When `existing.is_none()` → push title to `presets_added`. Otherwise compare title, description, and processors JSON; push to `presets_updated` only if any field differs.

### Audio file changes

Unchanged from current logic: new file → `files_added`, hash differs → `files_updated`, path gone → `files_removed`.

## CLI output format (`apps/cli/src/commands/sync.rs`)

Print items in this order: removed, updated, added, sidecar, presets (new), presets (updated). Omit any empty category from both the list and the summary.

```
Syncing library: /music
  [removed] old-sample
  [updated] snare
  [new]     kick-drum
  [sidecar] bass
  [preset]  reverb-heavy (new)
  [preset]  reverb-medium (updated)
Done — 1 added, 1 updated, 1 removed, 1 sidecar, 2 presets
```

If no changes: `Done — nothing changed`

Summary terms used: `added`, `updated`, `removed`, `sidecar` (count), `preset` / `presets` (combined added + updated count).

## Out of scope

- Preset removal detection (no removal logic exists today)
- Collection sync reporting (not requested)
- Per-clip granularity in sidecar output
