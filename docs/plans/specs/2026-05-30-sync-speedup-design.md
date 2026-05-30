# Sync Speedup Design

**Date:** 2026-05-30  
**Status:** Approved

## Problem

`musicum sync` is slow because it reads every audio file in full on every run — even when nothing has changed. The culprit is `file_hash` in `sync_service.rs`, which calls `std::fs::read(path)` to load the entire file into RAM before computing a SHA-256. For a 500-file library averaging 50 MB per file, that is ~25 GB of disk reads per sync regardless of changes.

Secondary issues:
- The directory is walked twice: once in `count_audio_files` (to size the progress bar) and again in `sync_library`.
- When hashing is unavoidable, the full-file read causes an O(file_size) memory spike.

## Goals

- Sync of an unchanged library should complete in under 1 second (reduced to one `stat()` call per file).
- Sync after partial changes should only do expensive work on the files that actually changed.
- Peak memory during hashing should be constant regardless of file size.

## Non-Goals

- Parallelism (not worth the complexity for a 200–1000 file library).
- Detecting externally-edited sidecars without touching the audio file. External sidecar edits require a `touch` of the audio file to be picked up on next sync. This is an accepted limitation.

## Design

### 1. Schema change

Add two columns to the `file` entity (`libs/musicum-core/src/db/entities/file.rs`):

| Column | ORM type | Meaning |
|---|---|---|
| `mtime` | `String` (RFC 3339) | mtime of the audio file at the time of last successful sync |
| `size_bytes` | `i64` | file size in bytes at the time of last successful sync |

Both columns are non-nullable with empty-string / 0 defaults so the schema migration (drop + recreate) produces valid rows on upgrade.

`SCHEMA_VERSION` in `libs/musicum-core/src/db/schema.rs` is bumped by 1. This drops and recreates all tables, triggering a one-time full sync after upgrade — expected and acceptable.

### 2. Fast-skip gate in sync_library

In the inner loop of `sync_library`, after the DB lookup for the existing file, insert a cheap pre-check before any expensive I/O:

```
stat(path)  →  mtime_str, size_bytes
if existing_row exists
   AND existing_row.mtime == mtime_str
   AND existing_row.size_bytes == size_bytes
→  skip (no hash, no probe, no sidecar read, no DB write)
   call on_progress() and continue
```

If the check fails (new file, or mtime/size differs), proceed with the existing hash → probe → sidecar → upsert flow, and store the new mtime and size_bytes in the upserted row.

The DB lookup is already performed unconditionally, so the mtime/size comparison adds zero extra queries.

**Known limitation:** If a `.musicum.json` sidecar is edited externally (outside musicum commands) without the audio file's mtime/size changing, the change will not be picked up on sync. Users must `touch` the audio file to force a re-sync of that entry.

### 3. Streaming SHA-256

Replace the current `file_hash` implementation:

```rust
// Current — reads entire file into RAM
fn file_hash(path: &Path) -> Result<String, ServiceError> {
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

// New — streams file through hasher in 64 KB chunks
fn file_hash(path: &Path) -> Result<String, ServiceError> {
    use std::io::{BufReader, Read};
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
```

No new dependencies — `sha2` already exposes the incremental `Update` trait. Peak memory for hashing is O(64 KB) regardless of file size.

### 4. Eliminate the double walk

Remove `count_audio_files` from `sync_service.rs` (it has no callers outside `sync.rs`).

In `apps/cli/src/commands/sync.rs`, replace the two-phase (count then sync) pattern with a single spinner that ticks on each file processed. The final summary line already communicates what changed, so the progress bar total is not needed.

## Files Affected

| File | Change |
|---|---|
| `libs/musicum-core/src/db/entities/file.rs` | Add `mtime: String` and `size_bytes: i64` columns |
| `libs/musicum-core/src/db/schema.rs` | Bump `SCHEMA_VERSION` |
| `libs/musicum-core/src/services/sync_service.rs` | Add fast-skip gate, streaming hash, remove `count_audio_files` |
| `apps/cli/src/commands/sync.rs` | Use spinner only (no count), remove `count_audio_files` call |

## Acceptance Criteria

- Running `musicum sync` on an unchanged library takes < 1 second for 200–500 files.
- Running `musicum sync` after adding/modifying files correctly picks up only those files.
- Removed files are still detected (the existing_paths removal-detection loop is unchanged).
- `cargo clippy --all` passes with no warnings.
- `cargo test -p musicum-core` passes.
