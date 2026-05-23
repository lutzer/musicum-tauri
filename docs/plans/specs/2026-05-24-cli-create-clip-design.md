# CLI: Create Clip from File

**Date:** 2026-05-24  
**Status:** Approved

## Overview

Add a `musicum clips create <file-slug> <title>` subcommand that creates a new clip entry for an existing audio file. The clip is written to the file's `.musicum.json` sidecar (source of truth) and immediately upserted into the DB — no separate `sync` step required.

## Command Interface

```
musicum clips create <file-slug> <title>
```

- `file-slug` — slug of an existing file in the DB (consistent with `clips list <file-slug>`)
- `title` — human-readable clip title; clip slug is derived as `slugify(title)`

No additional flags for this iteration. Notes and processors can be set by editing the sidecar directly.

## Implementation Flow

1. Look up the file by slug in the DB to get its filesystem `path`.
2. Read the sidecar at `<path>.musicum.json` (returns a default empty sidecar if the file doesn't exist).
3. Derive `clip_slug = slugify(title)`.
4. Error if a clip with `clip_slug` already exists in the sidecar's `clips` array.
5. Append `ClipSidecar { slug: clip_slug, title, notes: "", processors: [] }` to `sidecar.clips`.
6. Write the updated sidecar back to disk.
7. Upsert the new clip into the DB (reuse the existing `upsert_clips` path from sync_service).
8. Print: `Created clip '<clip-slug>' for file '<file-slug>'`

## Error Cases

- File slug not found in DB → `ServiceError::NotFound("file '<slug>'")`
- Clip slug collision (title produces a slug already present in the sidecar) → descriptive error naming the conflicting slug
- Sidecar read/write failures → propagate as IO errors

## Code Changes

| Location | Change |
|---|---|
| `libs/musicum-core/src/services/clip_service.rs` | Add `create_clip(db, library_dir, file_slug, title)` |
| `apps/cli/src/commands/clips.rs` | Add `Create { file_slug, title }` variant + handler |
| `apps/cli/src/main.rs` | Pass `library_dir` into `clips::run` so `create_clip` can locate the sidecar |

`file_service::get_file_by_slug` is already sufficient — no changes needed there.

## Design Decision: Logic in `clip_service`

The creation logic lives in `clip_service` (not split across the CLI handler) so it can be reused from Tauri commands later. `sync_service` already establishes the pattern of mixing sidecar reads with DB writes, so this coupling is consistent with the existing architecture.

## Out of Scope

- `--notes` / `--preset` flags (can be added later)
- Specifying a custom slug (auto-derived from title is sufficient)
- Editing or deleting clips via CLI
