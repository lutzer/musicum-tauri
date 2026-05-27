# Remove Clip Caching Fields & Function

**Date:** 2026-05-27  
**Status:** Approved

## Overview

Remove the clip-caching feature entirely: drop the `cached` and `cached_path` columns from the `clip` DB entity, strip all references across services and the CLI, and bump the schema version to trigger a clean DB rebuild.

No dedicated `cache_clip` CLI command exists — the removal is purely field- and logic-level.

## Motivation

Caching was previously planned as a background operation to pre-render processed clips to MP3 via ffmpeg. The feature is no longer needed and the fields add noise to every `ActiveModel` construction, the detail view, and list displays.

## Changes

### 1. DB Entity — `libs/musicum-core/src/db/entities/clip.rs`
- Remove `pub cached: String`
- Remove `pub cached_path: Option<String>`

### 2. Schema version — `libs/musicum-core/src/db/schema.rs`
- Bump `SCHEMA_VERSION` from `1` → `2`
- This drops and recreates all tables on next startup; the DB rebuilds from sidecars (expected behavior per project policy)

### 3. `libs/musicum-core/src/services/clip_service.rs`
- Remove `cached: Set(...)` and `cached_path: Set(...)` from:
  - `create_clip` ActiveModel
  - `update_clip_processors` ActiveModel
  - `set_clip_notes` ActiveModel
- Remove "Delete cached audio file if present" block in `delete_clip` (the `std::fs::remove_file` call)
- Remove `cached`/`cached_path` from the inline test `setup()` helper

### 4. `libs/musicum-core/src/services/sync_service.rs`
- Remove `cached: Set(...)` and `cached_path: Set(...)` from both `ActiveModel` constructions (the update-existing and insert-new paths)

### 5. `libs/musicum-core/src/services/file_service.rs`
- In `delete_file`: remove the "Delete cached clip audio files from disk" loop (`for c in &clips { if let Some(ref cp) = c.cached_path { … } }`)
- Keep the clips query (`clip::Entity::find()…`) since `clip_count` is still used

### 6. CLI — `apps/cli/src/commands/clips.rs`
- Remove `[{}]` cache status badge from the two list-view format strings
- Remove `Field("cached", clip.cached.clone())` from the detail view
- Remove `Field("cached_path", ...)` from the detail view

### 7. CLI — `apps/cli/src/commands/files.rs`
- Remove `[{}]` cache status badge from the clips-under-file list format string

### 8. Tests — `libs/musicum-core/tests/clip_service.rs`
- Remove `assert_eq!(clip.cached, "no_cache")` assertion

## Out of Scope
- Sidecar structs (`ClipSidecar`) — no `cached`/`cached_path` fields exist there
- Any future re-introduction of caching (separate feature, separate spec)

## Verification
- `cargo clippy --all` passes with no errors or warnings
- `cargo test -p musicum-core` passes
- `cargo build` succeeds
