# Sync Verbose Output Implementation Plan

**Goal:** Replace the silent `SyncStats` struct with a `SyncReport` that records per-item names for every change category, and update the CLI to print them.

**Architecture:** `SyncReport` holds six `Vec<String>` fields (one per change category). Upsert helpers return `bool` to indicate actual data changes. `sync_presets` receives `&mut SyncReport`. The CLI iterates each vec and prints labelled lines, then a summary.

**Tech Stack:** Rust, SeaORM, ratatui CLI (`apps/cli`), musicum-core services.

---

## Files

| Action | Path |
|--------|------|
| Modify | `libs/musicum-core/src/services/sync_service.rs` |
| Modify | `libs/musicum-core/tests/sync_service.rs` |
| Modify | `apps/cli/src/commands/sync.rs` |

---

### Task 1: Replace `SyncStats` with `SyncReport`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:15-20`

Replace the `SyncStats` struct (lines 15–20) with:

```rust
#[derive(Debug, Default)]
pub struct SyncReport {
    pub files_added:      Vec<String>,
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,
    pub presets_added:    Vec<String>,
    pub presets_updated:  Vec<String>,
}
```

Change the return type on line 25:
```rust
pub async fn sync_library(
    db: &DatabaseConnection,
    library_dir: &str,
) -> Result<SyncReport, ServiceError> {
```

Change the local variable on line 27:
```rust
let mut report = SyncReport::default();
```

In the removal loop (lines 71–80), replace `stats.removed += 1` with:
```rust
let display = Path::new(removed_path)
    .file_stem()
    .unwrap_or_default()
    .to_string_lossy()
    .to_string();
// (inside the if let Some block, after delete_file_cascade)
report.files_removed.push(display);
```

Replace the `sync_collections` and `sync_presets` calls at lines 83–84:
```rust
sync_collections(db, lib_path).await?;
sync_presets(db, lib_path, &mut report).await?;
```

Change the final return:
```rust
Ok(report)
```

---

### Task 2: Update `upsert_file_metadata` to return `bool`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:251-272`

Change signature and body:

```rust
async fn upsert_file_metadata(
    db: &DatabaseConnection,
    file_id: &str,
    meta: &crate::sidecar::FileMetadataSidecar,
) -> Result<bool, ServiceError> {
    let existing = file_metadata::Entity::find_by_id(file_id).one(db).await?;
    let model = file_metadata::ActiveModel {
        file_id: Set(file_id.to_string()),
        bpm: Set(meta.bpm),
        key: Set(meta.key.clone()),
        rating: Set(meta.rating),
        color: Set(meta.color.clone()),
        notes: Set(meta.notes.clone()),
        tags: Set(meta.tags.clone()),
    };
    let changed = if let Some(ex) = existing {
        let differs = ex.bpm != meta.bpm
            || ex.key != meta.key
            || ex.rating != meta.rating
            || ex.color != meta.color
            || ex.notes != meta.notes
            || ex.tags != meta.tags;
        if differs {
            model.update(db).await?;
        }
        differs
    } else {
        model.insert(db).await?;
        true
    };
    Ok(changed)
}
```

---

### Task 3: Update `upsert_clips` to return `bool`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:274-322`

Change signature and body to only write when data differs, returning `true` if anything changed:

```rust
async fn upsert_clips(
    db: &DatabaseConnection,
    file_id: &str,
    clip_sidecars: &[ClipSidecar],
) -> Result<bool, ServiceError> {
    let mut any_changed = false;
    for cs in clip_sidecars {
        let processors_json = serde_json::to_string(&cs.processors)?;
        let existing = clip::Entity::find()
            .filter(clip::Column::Slug.eq(&cs.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(ex) = existing {
            let differs = ex.title != cs.title
                || ex.processors != processors_json
                || ex.notes != cs.notes;
            if differs {
                clip::ActiveModel {
                    id: Set(ex.id.clone()),
                    slug: Set(cs.slug.clone()),
                    file_id: Set(file_id.to_string()),
                    title: Set(cs.title.clone()),
                    processors: Set(processors_json),
                    cached: Set(ex.cached.clone()),
                    cached_path: Set(ex.cached_path.clone()),
                    duration: Set(ex.duration),
                    notes: Set(cs.notes.clone()),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
                .update(db)
                .await?;
                any_changed = true;
            }
        } else {
            clip::ActiveModel {
                id: Set(Uuid::new_v4().to_string()),
                slug: Set(cs.slug.clone()),
                file_id: Set(file_id.to_string()),
                title: Set(cs.title.clone()),
                processors: Set(processors_json),
                cached: Set("no_cache".into()),
                cached_path: Set(None),
                duration: Set(None),
                notes: Set(cs.notes.clone()),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
            any_changed = true;
        }
    }
    Ok(any_changed)
}
```

---

### Task 4: Update `upsert_file` to use `SyncReport`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:169-249`

Change the signature to accept `&mut SyncReport`:

```rust
async fn upsert_file(
    db: &DatabaseConnection,
    path: &Path,
    path_str: &str,
    hash: &str,
    audio: &AudioInfo,
    sc: &FileSidecar,
    report: &mut SyncReport,
) -> Result<(), ServiceError> {
```

In the `if existing_model.hash == hash` branch (currently lines 193–197), replace the early return with:

```rust
if existing_model.hash == hash {
    let meta_changed = upsert_file_metadata(db, &existing_model.id, &sc.metadata).await?;
    let clips_changed = upsert_clips(db, &existing_model.id, &sc.clips).await?;
    if meta_changed || clips_changed {
        report.sidecars_updated.push(name);
    }
    return Ok(());
}
```

In the "file changed (hash differs)" branch, replace `stats.updated += 1` with:
```rust
report.files_updated.push(name.clone());
```

In the "new file" branch, replace `stats.added += 1` with:
```rust
report.files_added.push(name.clone());
```

Update the `upsert_file` call in `sync_library` (line 67) to pass `&mut report` instead of `&mut stats`.

---

### Task 5: Update `sync_presets` to track changes

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:428-465`

Change signature:

```rust
async fn sync_presets(
    db: &DatabaseConnection,
    library_dir: &Path,
    report: &mut SyncReport,
) -> Result<(), ServiceError> {
```

In the `if let Some(ex) = existing` branch, only update and push to `presets_updated` when data actually differs:

```rust
if let Some(ex) = existing {
    let differs = ex.title != sc.title
        || ex.description != sc.description
        || ex.processors != processors_json;
    if differs {
        preset::ActiveModel {
            id: Set(ex.id.clone()),
            slug: Set(sc.slug.clone()),
            title: Set(sc.title.clone()),
            description: Set(sc.description.clone()),
            processors: Set(processors_json),
            created_at: Set(ex.created_at.clone()),
            updated_at: Set(now),
        }
        .update(db)
        .await?;
        report.presets_updated.push(sc.title.clone());
    }
} else {
    preset::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set(sc.slug.clone()),
        title: Set(sc.title.clone()),
        description: Set(sc.description.clone()),
        processors: Set(processors_json),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;
    report.presets_added.push(sc.title.clone());
}
```

---

### Task 6: Fix existing tests to compile

**Files:**
- Modify: `libs/musicum-core/tests/sync_service.rs`

All tests that read `SyncStats` fields need updating — `added` → `files_added.len()`, etc.:

| Test | Old assertion | New assertion |
|------|--------------|---------------|
| `sync_discovers_wav_file` line 23 | `stats.added == 1` | `stats.files_added.len() == 1` |
| `sync_discovers_wav_file` line 24 | `stats.removed == 0` | `stats.files_removed.is_empty()` |
| `sync_idempotent_on_unchanged_file` line 119 | `s1.added == 1` | `s1.files_added.len() == 1` |
| `sync_idempotent_on_unchanged_file` line 124 | `s2.added == 0` | `s2.files_added.is_empty()` |
| `sync_idempotent_on_unchanged_file` line 125 | `s2.updated == 0` | `s2.files_updated.is_empty()` |
| `sync_idempotent_on_unchanged_file` line 126 | `s2.removed == 0` | `s2.files_removed.is_empty()` |
| `sync_detects_removed_files` line 150 | `s2.removed == 1` | `s2.files_removed.len() == 1` |
| `sync_walks_subdirectories` line 167 | `stats.added == 3` | `stats.files_added.len() == 3` |

Run:
```
cargo test -p musicum-core
```
Expected: all existing tests pass.

---

### Task 7: Add new tests for `SyncReport` change tracking

**Files:**
- Modify: `libs/musicum-core/tests/sync_service.rs`

Add three new tests at the end of the file:

```rust
#[tokio::test]
async fn report_tracks_sidecar_metadata_update() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    // Modify sidecar metadata without touching the audio file
    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    let report = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(report.sidecars_updated, vec!["bass"], "sidecar change should be reported");
    assert!(report.files_added.is_empty());
    assert!(report.files_updated.is_empty());
}

#[tokio::test]
async fn report_sidecar_unchanged_is_silent() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("pad.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    // Second sync with no changes at all
    let report = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert!(report.sidecars_updated.is_empty(), "unchanged sidecar should not appear in report");
    assert!(report.files_added.is_empty());
}

#[tokio::test]
async fn report_tracks_preset_added_and_updated() {
    let dir = tempdir().unwrap();

    let mut preset_sc = sidecar::PresetSidecar {
        version: 1,
        slug: "reverb-hall".into(),
        title: "Hall Reverb".into(),
        description: "".into(),
        processors: vec![],
    };
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    let db = setup(dir.path()).await;
    let r1 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(r1.presets_added, vec!["Hall Reverb"]);
    assert!(r1.presets_updated.is_empty());

    // Update the preset sidecar
    preset_sc.description = "large hall".into();
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    let r2 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert!(r2.presets_added.is_empty());
    assert_eq!(r2.presets_updated, vec!["Hall Reverb"]);

    // Third sync — nothing changed
    let r3 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert!(r3.presets_added.is_empty());
    assert!(r3.presets_updated.is_empty());
}
```

Run:
```
cargo test -p musicum-core
```
Expected: all tests pass, including the three new ones.

---

### Task 8: Update CLI sync command output

**Files:**
- Modify: `apps/cli/src/commands/sync.rs`

Replace the entire file with:

```rust
use anyhow::Result;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

use crate::settings::AppSettings;

pub async fn run(db: &DatabaseConnection, settings: &AppSettings) -> Result<()> {
    println!("Syncing library: {}", settings.library_dir);
    let report = sync_service::sync_library(db, &settings.library_dir).await?;

    for name in &report.files_removed {
        println!("  [removed] {name}");
    }
    for name in &report.files_updated {
        println!("  [updated] {name}");
    }
    for name in &report.files_added {
        println!("  [new]     {name}");
    }
    for name in &report.sidecars_updated {
        println!("  [sidecar] {name}");
    }
    for name in &report.presets_added {
        println!("  [preset]  {name} (new)");
    }
    for name in &report.presets_updated {
        println!("  [preset]  {name} (updated)");
    }

    let fa = report.files_added.len();
    let fu = report.files_updated.len();
    let fr = report.files_removed.len();
    let su = report.sidecars_updated.len();
    let pt = report.presets_added.len() + report.presets_updated.len();

    let mut parts: Vec<String> = Vec::new();
    if fa > 0 { parts.push(format!("{fa} added")); }
    if fu > 0 { parts.push(format!("{fu} updated")); }
    if fr > 0 { parts.push(format!("{fr} removed")); }
    if su > 0 { parts.push(format!("{su} sidecar")); }
    if pt > 0 { parts.push(format!("{pt} {}", if pt == 1 { "preset" } else { "presets" })); }

    if parts.is_empty() {
        println!("Done — nothing changed");
    } else {
        println!("Done — {}", parts.join(", "));
    }

    Ok(())
}
```

Run:
```
cargo build
cargo clippy --all
```
Expected: clean build, no clippy warnings.

---

### Task 9: Smoke test end-to-end

Run a real sync against your dev library:
```
cargo run -p musicum-cli -- sync
```

Verify:
- New files show `[new]     <name>`
- Files with sidecar edits show `[sidecar] <name>`
- Preset sidecars show `[preset]  <name> (new)` or `(updated)`
- Re-running immediately prints `Done — nothing changed`
- Summary line omits empty categories
