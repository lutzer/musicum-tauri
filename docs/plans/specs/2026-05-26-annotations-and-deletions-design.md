# Annotations & Deletions

Date: 2026-05-26

## Overview

Add CLI commands to set notes/tags on files and notes on clips, delete files and
clips (with sidecar + DB cleanup), and rename the existing `presets remove`
command to `presets delete` for consistency.

## New CLI commands

All follow the existing `<noun> <verb> <slug>` pattern.

```
musicum files set-notes <slug> <notes>
musicum files set-tags  <slug> <tags>
musicum files delete    <slug> [--delete-audio]

musicum clips set-notes <slug> <notes>
musicum clips delete    <slug>

musicum presets delete  <slug>   # renamed from `presets remove`
```

### Behaviour notes

- `files set-notes` / `files set-tags`: full-replace the notes or tags string.
  Tags are a plain string (e.g. `"kick, loop, 140bpm"`); no parsing or
  delimiters are enforced by the CLI.
- `files delete`: removes the `.musicum.json` sidecar and the file DB entry.
  Cascades to all clips belonging to that file: their cached audio files are
  deleted from disk and their DB entries are removed. With `--delete-audio` the
  audio file itself is also deleted.
- `clips delete`: strips the clip from the sidecar's `clips` array, deletes
  its cached file from disk if present, removes the clip DB entry.
- `presets delete`: identical to the existing `presets remove` — deletes the
  `.musicum-preset.json` sidecar and the preset DB entry. Only the command name
  changes.

## New service functions

All business logic lives in `musicum-core`. Sidecar is written before the DB
update (sidecar is source of truth).

### `file_service`

File and clip sidecar paths are derived from `file.path` (stored on the entity),
so `library_dir` is not needed for these functions.

```rust
pub async fn set_file_notes(
    db: &DatabaseConnection,
    file_slug: &str,
    notes: &str,
) -> Result<(), ServiceError>
```
Updates `FileMetadataSidecar.notes` in the sidecar and `file_metadata.notes`
in the DB. Creates a `file_metadata` row if one does not yet exist.

```rust
pub async fn set_file_tags(
    db: &DatabaseConnection,
    file_slug: &str,
    tags: &str,
) -> Result<(), ServiceError>
```
Same as above but for the `tags` field.

```rust
pub async fn delete_file(
    db: &DatabaseConnection,
    file_slug: &str,
    delete_audio: bool,
) -> Result<(), ServiceError>
```
1. Resolve file by slug.
2. Load all clips for the file; for each clip delete its cached file from disk
   if `clip.cached_path` is set.
3. Delete all clip DB entries for the file.
4. Delete the `.musicum.json` sidecar.
5. Delete the file DB entry.
6. If `delete_audio` is true, delete the audio file from disk.

### `clip_service`

```rust
pub async fn set_clip_notes(
    db: &DatabaseConnection,
    clip_slug: &str,
    notes: &str,
) -> Result<(), ServiceError>
```
Finds the clip's parent file, updates the `notes` field on the matching
`ClipSidecar` entry, writes the sidecar, then updates `clip.notes` in the DB.

```rust
pub async fn delete_clip(
    db: &DatabaseConnection,
    clip_slug: &str,
) -> Result<(), ServiceError>
```
1. Resolve clip + parent file by slug.
2. Delete cached file from disk if `clip.cached_path` is set.
3. Remove the clip entry from the sidecar's `clips` array and write the sidecar.
4. Delete the clip DB entry.

### `preset_service`

`delete_preset` already exists and is unchanged. Only the CLI command is renamed.

## CLI output

All mutating commands print via `print_result`. Examples:

```
files set-notes  →  Set notes  [file: my-kick]
files set-tags   →  Set tags   [file: my-kick]
clips set-notes  →  Set notes  [clip: my-kick-body]
clips delete     →  Deleted clip  [slug: my-kick-body]
files delete     →  Deleted file  [slug: my-kick  clips: 2  audio: deleted | kept]
presets delete   →  Deleted preset  [slug: my-reverb]
```
