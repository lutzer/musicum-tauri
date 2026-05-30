# Sync Speedup Implementation Plan

**Goal:** Eliminate full-file reads on every sync by adding an mtime+size fast-skip gate, and replace the full-file-in-RAM hash with a streaming SHA-256.

**Architecture:** Two new columns (`mtime`, `size_bytes`) are added to the `file` entity. On sync, each file is stat()-checked first; if mtime and size match the stored values the file is skipped entirely. Only changed or new files proceed to hash+probe+sidecar. The SHA-256 implementation is replaced with a BufReader loop. The redundant pre-scan walk is removed.

**Tech Stack:** Rust, SeaORM 1, SQLite, sha2, walkdir, indicatif

---

## File Map

| File | Change |
|---|---|
| `libs/musicum-core/src/db/entities/file.rs` | Add `mtime: String` and `size_bytes: i64` fields |
| `libs/musicum-core/src/db/schema.rs` | Bump `SCHEMA_VERSION` from 2 → 3 |
| `libs/musicum-core/src/services/sync_service.rs` | Fast-skip gate, streaming hash, remove `count_audio_files` |
| `apps/cli/src/commands/sync.rs` | Spinner-only progress (remove count walk) |
| `libs/musicum-core/tests/db_schema.rs` | Update 4 `file::ActiveModel` literals to include new fields |
| `libs/musicum-core/src/services/file_service.rs` | Update 1 test-helper `file::ActiveModel` literal |
| `libs/musicum-core/src/services/clip_service.rs` | Update 1 test-helper `file::ActiveModel` literal |
| `libs/musicum-core/src/services/collection_service.rs` | Update 1 test-helper `file::ActiveModel` literal |

---

### Task 1: Add `mtime` and `size_bytes` to the `file` entity

**Files:**
- Modify: `libs/musicum-core/src/db/entities/file.rs`

Add two fields to `Model` between `hash` and `created_at`:

```rust
pub hash: String,
pub mtime: String,
pub size_bytes: i64,
pub created_at: String,
pub updated_at: String,
```

Full file after change:

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "file")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub name: String,
    pub path: String,
    pub duration: f64,
    pub sample_rate: i32,
    pub channels: i32,
    pub mime_type: String,
    pub hash: String,
    pub mtime: String,
    pub size_bytes: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::file_metadata::Entity")]
    FileMetadata,
    #[sea_orm(has_many = "super::file_attachment::Entity")]
    FileAttachment,
    #[sea_orm(has_many = "super::clip::Entity")]
    Clip,
}

impl ActiveModelBehavior for ActiveModel {}
```

**Verify:** `cargo build -p musicum-core` should fail with missing-field errors in every place that constructs `file::ActiveModel` — that is expected and correct at this stage.

---

### Task 2: Bump `SCHEMA_VERSION`

**Files:**
- Modify: `libs/musicum-core/src/db/schema.rs`

Change:
```rust
pub const SCHEMA_VERSION: u32 = 2;
```
to:
```rust
pub const SCHEMA_VERSION: u32 = 3;
```

This causes `db::connect()` to drop and recreate all tables on the next run — the one-time full sync is expected behaviour.

---

### Task 3: Fix all `file::ActiveModel` literals that no longer compile

Every place that constructs a `file::ActiveModel` struct literal must now include `mtime` and `size_bytes`. There are six locations outside `sync_service.rs` (handled in Task 4):

**Files:**
- Modify: `libs/musicum-core/tests/db_schema.rs` (4 literals, lines ~49, 89, 142, 190)
- Modify: `libs/musicum-core/src/services/file_service.rs` (1 test helper, ~line 145)
- Modify: `libs/musicum-core/src/services/clip_service.rs` (1 test helper, ~line 194)
- Modify: `libs/musicum-core/src/services/collection_service.rs` (1 test helper, ~line 274)

For every `file::ActiveModel { ... }` literal in those files, add these two fields after `hash`:

```rust
mtime: Set(String::new()),
size_bytes: Set(0),
```

These are test helpers that insert dummy rows — zero/empty values are fine since the tests don't exercise sync logic.

**Verify:** `cargo build -p musicum-core` compiles (sync_service.rs will still error until Task 4).

---

### Task 4: Rewrite `sync_service.rs` — streaming hash + fast-skip + remove `count_audio_files`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs`

#### 4a — Replace `file_hash` with a streaming implementation

Replace the existing function (lines 184–188):

```rust
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

`sha2::Sha256` already implements the incremental `Update` trait — no new dependency needed. The `Sha256::digest()` one-liner is replaced by `Sha256::new()` + `hasher.update()` + `hasher.finalize()`. Peak memory drops from O(file_size) to O(64 KB).

#### 4b — Add a helper to read filesystem mtime as an RFC 3339 string

Add this helper below `file_hash`:

```rust
fn file_mtime(path: &Path) -> Result<(String, i64), ServiceError> {
    let meta = std::fs::metadata(path)?;
    let modified = meta.modified()?;
    let size = meta.len() as i64;
    let dt: chrono::DateTime<chrono::Utc> = modified.into();
    Ok((dt.to_rfc3339(), size))
}
```

#### 4c — Add the fast-skip gate in `sync_library`

The inner loop currently looks like:

```rust
let path_str = path.to_string_lossy().to_string();
existing_paths.remove(&path_str);

let hash = file_hash(path)?;
let sc = sidecar::read_file_sidecar(path)?;
let audio_info = probe_audio(path)?;

upsert_file(db, path, &path_str, &hash, &audio_info, &sc, &mut report).await?;
on_progress();
```

Replace it with:

```rust
let path_str = path.to_string_lossy().to_string();
existing_paths.remove(&path_str);

let (mtime, size_bytes) = file_mtime(path)?;

// Fast-skip: if mtime and size match what's in the DB, nothing has changed.
let existing = file::Entity::find()
    .filter(file::Column::Path.eq(&path_str))
    .one(db)
    .await?;

if let Some(ref ex) = existing {
    if ex.mtime == mtime && ex.size_bytes == size_bytes {
        on_progress();
        continue;
    }
}

let hash = file_hash(path)?;
let sc = sidecar::read_file_sidecar(path)?;
let audio_info = probe_audio(path)?;

upsert_file(db, path, &path_str, &hash, &mtime, size_bytes, &audio_info, &sc, existing, &mut report).await?;
on_progress();
```

Note: `existing` is now passed into `upsert_file` to avoid a second DB lookup inside it (the function was already doing `Entity::find().filter(path)`).

#### 4d — Update `upsert_file` signature and body

Change the signature to accept the already-fetched `existing` row and the new `mtime`/`size_bytes`:

```rust
async fn upsert_file(
    db: &DatabaseConnection,
    path: &Path,
    path_str: &str,
    hash: &str,
    mtime: &str,
    size_bytes: i64,
    audio: &AudioInfo,
    sc: &FileSidecar,
    existing: Option<file::Model>,
    report: &mut SyncReport,
) -> Result<(), ServiceError> {
```

Remove the internal `Entity::find()` call (lines 210–213) since `existing` is now passed in.

Add `mtime` and `size_bytes` to both `file::ActiveModel` literals inside `upsert_file`:

For the update branch (hash differs):
```rust
file::ActiveModel {
    id:         Set(existing_model.id.clone()),
    slug:       Set(slug),
    name:       Set(name.clone()),
    path:       Set(path_str.to_string()),
    duration:   Set(audio.duration),
    sample_rate: Set(audio.sample_rate as i32),
    channels:   Set(audio.channels as i32),
    mime_type:  Set(audio.mime_type.clone()),
    hash:       Set(hash.to_string()),
    mtime:      Set(mtime.to_string()),
    size_bytes: Set(size_bytes),
    created_at: Set(existing_model.created_at.clone()),
    updated_at: Set(now),
}
.update(db)
.await?;
```

For the insert branch (new file):
```rust
file::ActiveModel {
    id:         Set(id.clone()),
    slug:       Set(slug),
    name:       Set(name.clone()),
    path:       Set(path_str.to_string()),
    duration:   Set(audio.duration),
    sample_rate: Set(audio.sample_rate as i32),
    channels:   Set(audio.channels as i32),
    mime_type:  Set(audio.mime_type.clone()),
    hash:       Set(hash.to_string()),
    mtime:      Set(mtime.to_string()),
    size_bytes: Set(size_bytes),
    created_at: Set(now.clone()),
    updated_at: Set(now),
}
.insert(db)
.await?;
```

Also remove the early-return branch for `existing_model.hash == hash` (lines 216–222). With the fast-skip gate in place, we only reach `upsert_file` when mtime/size changed, so the hash comparison is the only remaining guard. Keep it — if the mtime was touched but contents are identical (e.g. `touch` command), the hash comparison skips the update correctly. The sidecar re-read still happens in this case; that's acceptable.

Actually, keep the early-return for hash match but update it to also sync mtime/size if they drifted (a `touch` scenario):

```rust
if existing_model.hash == hash {
    // Contents unchanged. Update mtime/size if they drifted (e.g. after `touch`).
    if existing_model.mtime != mtime || existing_model.size_bytes != size_bytes {
        file::ActiveModel {
            id:         Set(existing_model.id.clone()),
            mtime:      Set(mtime.to_string()),
            size_bytes: Set(size_bytes),
            // all other fields unchanged
            slug:       Set(existing_model.slug.clone()),
            name:       Set(existing_model.name.clone()),
            path:       Set(existing_model.path.clone()),
            duration:   Set(existing_model.duration),
            sample_rate: Set(existing_model.sample_rate),
            channels:   Set(existing_model.channels),
            mime_type:  Set(existing_model.mime_type.clone()),
            hash:       Set(existing_model.hash.clone()),
            created_at: Set(existing_model.created_at.clone()),
            updated_at: Set(existing_model.updated_at.clone()),
        }
        .update(db)
        .await?;
    }
    let meta_changed = upsert_file_metadata(db, &existing_model.id, &sc.metadata).await?;
    let clips_changed = upsert_clips(db, &existing_model.id, &sc.clips).await?;
    if meta_changed || clips_changed {
        report.sidecars_updated.push(name);
    }
    return Ok(());
}
```

#### 4e — Remove `count_audio_files`

Delete the entire `count_audio_files` function (lines 15–32). It is only called from `apps/cli/src/commands/sync.rs`.

**Verify:** `cargo build -p musicum-core` compiles cleanly.

---

### Task 5: Update `sync.rs` CLI command — spinner only

**Files:**
- Modify: `apps/cli/src/commands/sync.rs`

Remove the `count_audio_files` call and the conditional progress bar / spinner logic. Replace with a single spinner that ticks on every file processed.

Full file after change:

```rust
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use musicum_core::config::LibraryPaths;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, paths: &LibraryPaths) -> Result<()> {
    println!("Syncing library: {}", paths.library_dir.display());

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {pos} files scanned  {elapsed_precise}")
            .unwrap(),
    );

    let pb_tick = pb.clone();
    let report = sync_service::sync_library(db, paths, move || pb_tick.inc(1)).await?;

    pb.finish_and_clear();

    for name in &report.files_removed    { println!("  [removed] {name}"); }
    for name in &report.files_updated    { println!("  [updated] {name}"); }
    for name in &report.files_added      { println!("  [new]     {name}"); }
    for name in &report.sidecars_updated { println!("  [sidecar] {name}"); }

    let fa = report.files_added.len();
    let fu = report.files_updated.len();
    let fr = report.files_removed.len();
    let su = report.sidecars_updated.len();

    let mut parts: Vec<String> = Vec::new();
    if fa > 0 { parts.push(format!("{fa} added")); }
    if fu > 0 { parts.push(format!("{fu} updated")); }
    if fr > 0 { parts.push(format!("{fr} removed")); }
    if su > 0 { parts.push(format!("{su} sidecar")); }

    if parts.is_empty() {
        println!("Done — nothing changed");
    } else {
        println!("Done — {}", parts.join(", "));
    }

    Ok(())
}
```

**Verify:** `cargo build` compiles. `cargo clippy --all` passes with no warnings.

---

### Task 6: Run the test suite

```bash
cargo test -p musicum-core
```

Expected: all tests pass. Key tests to watch:
- `db_schema::schema_reset_on_version_bump` — verifies that the version bump drops tables.
- `db_schema::insert_and_query_file_with_metadata` — verifies the new schema round-trips.
- `file_service` tests — verify file CRUD still works with the new columns.

If any `file::ActiveModel` literal in test code fails to compile, add `mtime: Set(String::new()), size_bytes: Set(0),` to it.

---

### Task 7: Smoke-test the sync command

```bash
cargo run -p musicum-cli -- sync
```

Run it twice on an unchanged library. Expected behaviour:

- First run: all files are processed (mtime/size not yet stored → fast-skip cannot fire). Summary shows additions.
- Second run: completes in < 1 second. Summary shows "nothing changed".

Then modify one audio file (e.g. `touch some-file.wav`) and run again. Expected: only that file is re-processed.

---

### Task 8: Lint

```bash
cargo clippy --all
```

Fix any warnings before considering the work done.
