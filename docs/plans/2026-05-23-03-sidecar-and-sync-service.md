# Sidecar + Sync Service Implementation Plan

**Goal:** Implement sidecar file reading/writing (the `.musicum.json` and `.musicum-preset.json` formats), plus a sync service that walks a library directory, parses sidecars, and upserts everything into the DB. Tests use real WAV files generated with `hound`.

**Architecture:** `sidecar.rs` owns the JSON types and read/write helpers. `sync_service.rs` drives a full directory walk, calls `sidecar`, decodes audio metadata with symphonia, and upserts `file`, `file_metadata`, and `clip` rows. `file_service.rs` and `clip_service.rs` expose thin CRUD wrappers used by the CLI and (later) Tauri commands.

**Tech Stack:** `walkdir` for directory traversal, `symphonia` for audio probe (duration, sample rate, channels), `serde_json` for sidecar I/O, `sha2` + `hex` for file hashing, `hound` (dev-dep) for synthesising test WAV files.

**Prerequisite:** Plan 02 complete — DB layer compiles and all entity tests pass.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `libs/musicum-core/src/sidecar.rs` | Sidecar types + read/write |
| Create | `libs/musicum-core/src/services/file_service.rs` | CRUD for `file` + `file_metadata` |
| Create | `libs/musicum-core/src/services/clip_service.rs` | CRUD for `clip` |
| Create | `libs/musicum-core/src/services/collection_service.rs` | CRUD for `collection` + `collection_clip` |
| Create | `libs/musicum-core/src/services/preset_service.rs` | CRUD for `preset` |
| Create | `libs/musicum-core/src/services/sync_service.rs` | Library walk + sidecar sync |
| Modify | `libs/musicum-core/src/services/mod.rs` | Declare service modules |
| Modify | `libs/musicum-core/src/lib.rs` | Declare `sidecar` module |
| Modify | `libs/musicum-core/Cargo.toml` | Add `sha2`, `hex` dependencies |
| Create | `libs/musicum-core/tests/common/mod.rs` | Test helpers (WAV generator) |
| Create | `libs/musicum-core/tests/sync_service.rs` | Integration tests with real WAV files |

---

### Task 1: Add missing dependencies

**Step 1.1** — Add to `[dependencies]` in `libs/musicum-core/Cargo.toml`:
```toml
sha2 = "0.10"
hex  = "0.4"
```

**Step 1.2** — Verify:
```bash
cargo check -p musicum-core
# Expected: Compiling sha2 ... Compiling hex ... Finished
```

---

### Task 2: Implement sidecar types and I/O

The sidecar format has three variants: audio-file sidecars (`.musicum.json` next to each audio file), collection sidecars (`collections/{slug}.musicum.json`), and preset sidecars (`presets/{slug}.musicum-preset.json`). All live under `<library_dir>/.musicum/`.

**Step 2.1** — Create `libs/musicum-core/src/sidecar.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::ServiceError;

// ── Processor entry (shared by clips and presets) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub enabled: bool,
    pub params: serde_json::Value,
}

// ── Audio-file sidecar ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSidecar {
    pub version: u32,
    pub metadata: FileMetadataSidecar,
    #[serde(default)]
    pub attachments: Vec<AttachmentSidecar>,
    #[serde(default)]
    pub clips: Vec<ClipSidecar>,
}

impl FileSidecar {
    pub fn default_for_file() -> Self {
        FileSidecar {
            version: 1,
            metadata: FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMetadataSidecar {
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub rating: Option<i32>,
    pub color: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentSidecar {
    pub uuid: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSidecar {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub processors: Vec<ProcessorEntry>,
}

// ── Collection sidecar ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSidecar {
    pub version: u32,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub clips: Vec<String>,
}

// ── Preset sidecar ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetSidecar {
    pub version: u32,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub processors: Vec<ProcessorEntry>,
}

// ── Read/write helpers ────────────────────────────────────────────────────

pub fn read_file_sidecar(audio_path: &Path) -> Result<FileSidecar, ServiceError> {
    let sidecar_path = sidecar_path_for_audio(audio_path);
    if !sidecar_path.exists() {
        return Ok(FileSidecar::default_for_file());
    }
    let text = std::fs::read_to_string(&sidecar_path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn write_file_sidecar(audio_path: &Path, sidecar: &FileSidecar) -> Result<(), ServiceError> {
    let sidecar_path = sidecar_path_for_audio(audio_path);
    let json = serde_json::to_string_pretty(sidecar)?;
    std::fs::write(&sidecar_path, json)?;
    Ok(())
}

pub fn sidecar_path_for_audio(audio_path: &Path) -> std::path::PathBuf {
    let stem = audio_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    audio_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}.musicum.json"))
}

pub fn read_collection_sidecars(library_dir: &Path) -> Result<Vec<CollectionSidecar>, ServiceError> {
    let dir = library_dir.join(".musicum").join("collections");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let text = std::fs::read_to_string(&path)?;
            let sc: CollectionSidecar = serde_json::from_str(&text)?;
            result.push(sc);
        }
    }
    Ok(result)
}

pub fn write_collection_sidecar(
    library_dir: &Path,
    sc: &CollectionSidecar,
) -> Result<(), ServiceError> {
    let dir = library_dir.join(".musicum").join("collections");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.musicum.json", sc.slug));
    let json = serde_json::to_string_pretty(sc)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn read_preset_sidecars(library_dir: &Path) -> Result<Vec<PresetSidecar>, ServiceError> {
    let dir = library_dir.join(".musicum").join("presets");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let text = std::fs::read_to_string(&path)?;
            let sc: PresetSidecar = serde_json::from_str(&text)?;
            result.push(sc);
        }
    }
    Ok(result)
}

pub fn write_preset_sidecar(library_dir: &Path, sc: &PresetSidecar) -> Result<(), ServiceError> {
    let dir = library_dir.join(".musicum").join("presets");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.musicum-preset.json", sc.slug));
    let json = serde_json::to_string_pretty(sc)?;
    std::fs::write(&path, json)?;
    Ok(())
}
```

---

### Task 3: Implement the sync service

The sync service walks the library directory, finds all audio files (WAV, MP3, FLAC, OGG, AIFF), reads or creates their sidecars, probes audio metadata with symphonia, and upserts DB rows. It also syncs collections and presets from their sidecar directories.

**Step 3.1** — Create `libs/musicum-core/src/services/sync_service.rs`:
```rust
use std::path::{Path, PathBuf};

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use slug::slugify;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::db::entities::{clip, collection, collection_clip, file, file_metadata, preset};
use crate::sidecar::{self, ClipSidecar, FileSidecar};
use crate::ServiceError;

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif"];

#[derive(Debug, Default)]
pub struct SyncStats {
    pub added: u32,
    pub updated: u32,
    pub removed: u32,
}

pub async fn sync_library(
    db: &DatabaseConnection,
    library_dir: &str,
) -> Result<SyncStats, ServiceError> {
    let lib_path = Path::new(library_dir);
    let mut stats = SyncStats::default();

    // 1. Collect all current file paths in the DB for removal detection
    let existing_files = file::Entity::find().all(db).await?;
    let mut existing_paths: std::collections::HashSet<String> =
        existing_files.iter().map(|f| f.path.clone()).collect();

    // 2. Walk the library directory for audio files
    for entry in WalkDir::new(lib_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip .musicum hidden directory
        if path.components().any(|c| c.as_os_str() == ".musicum") {
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        existing_paths.remove(&path_str);

        let hash = file_hash(path)?;
        let sc = sidecar::read_file_sidecar(path)?;
        let audio_info = probe_audio(path)?;

        upsert_file(db, path, &path_str, &hash, &audio_info, &sc, &mut stats).await?;
    }

    // 3. Mark removed files (paths no longer on disk)
    for removed_path in &existing_paths {
        file::Entity::delete_many()
            .filter(file::Column::Path.eq(removed_path.as_str()))
            .exec(db)
            .await?;
        stats.removed += 1;
    }

    // 4. Sync collections and presets from their sidecar directories
    sync_collections(db, lib_path).await?;
    sync_presets(db, lib_path).await?;

    Ok(stats)
}

// ── Audio probing ─────────────────────────────────────────────────────────

pub struct AudioInfo {
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub mime_type: String,
}

pub fn probe_audio(path: &Path) -> Result<AudioInfo, ServiceError> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let src = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| ServiceError::InvalidInput(format!("audio probe failed: {e}")))?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| ServiceError::InvalidInput("no audio track found".into()))?;

    let params = &track.codec_params;

    let sample_rate = params.sample_rate.unwrap_or(44100);
    let channels = params
        .channels
        .map(|c| c.count() as u32)
        .unwrap_or(1);

    let duration = if let Some(frames) = params.n_frames {
        frames as f64 / sample_rate as f64
    } else if let Some(tb) = params.time_base {
        let ts = track.codec_params.start_ts;
        ts as f64 * tb.numer as f64 / tb.denom as f64
    } else {
        0.0
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav")
        .to_lowercase();
    let mime_type = match ext.as_str() {
        "mp3"  => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg"  => "audio/ogg",
        "aiff" | "aif" => "audio/aiff",
        _      => "audio/wav",
    };

    Ok(AudioInfo {
        duration,
        sample_rate,
        channels,
        mime_type: mime_type.to_string(),
    })
}

fn file_hash(path: &Path) -> Result<String, ServiceError> {
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

// ── Upsert helpers ────────────────────────────────────────────────────────

async fn upsert_file(
    db: &DatabaseConnection,
    path: &Path,
    path_str: &str,
    hash: &str,
    audio: &AudioInfo,
    sc: &FileSidecar,
    stats: &mut SyncStats,
) -> Result<(), ServiceError> {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let slug = slugify(&name);
    let now = chrono::Utc::now().to_rfc3339();

    // Find existing file by path
    let existing = file::Entity::find()
        .filter(file::Column::Path.eq(path_str))
        .one(db)
        .await?;

    let file_id = if let Some(existing_model) = existing {
        if existing_model.hash == hash {
            // File unchanged — still sync clips from sidecar
            upsert_clips(db, &existing_model.id, &sc.clips).await?;
            return Ok(());
        }
        // File changed (hash differs) — update
        file::ActiveModel {
            id: Set(existing_model.id.clone()),
            slug: Set(slug),
            name: Set(name),
            path: Set(path_str.to_string()),
            duration: Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels: Set(audio.channels as i32),
            mime_type: Set(audio.mime_type.clone()),
            hash: Set(hash.to_string()),
            created_at: Set(existing_model.created_at.clone()),
            updated_at: Set(now),
        }
        .update(db)
        .await?;

        upsert_file_metadata(db, &existing_model.id, &sc.metadata).await?;
        stats.updated += 1;
        existing_model.id
    } else {
        // New file
        let id = Uuid::new_v4().to_string();

        file::ActiveModel {
            id: Set(id.clone()),
            slug: Set(slug),
            name: Set(name),
            path: Set(path_str.to_string()),
            duration: Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels: Set(audio.channels as i32),
            mime_type: Set(audio.mime_type.clone()),
            hash: Set(hash.to_string()),
            created_at: Set(now.clone()),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;

        upsert_file_metadata(db, &id, &sc.metadata).await?;
        stats.added += 1;

        // Write back default sidecar if it didn't exist
        let _ = sidecar::write_file_sidecar(path, sc);

        id
    };

    upsert_clips(db, &file_id, &sc.clips).await?;
    Ok(())
}

async fn upsert_file_metadata(
    db: &DatabaseConnection,
    file_id: &str,
    meta: &crate::sidecar::FileMetadataSidecar,
) -> Result<(), ServiceError> {
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
    if existing.is_some() {
        model.update(db).await?;
    } else {
        model.insert(db).await?;
    }
    Ok(())
}

async fn upsert_clips(
    db: &DatabaseConnection,
    file_id: &str,
    clip_sidecars: &[ClipSidecar],
) -> Result<(), ServiceError> {
    for cs in clip_sidecars {
        let processors_json = serde_json::to_string(&cs.processors)?;
        let existing = clip::Entity::find()
            .filter(clip::Column::Slug.eq(&cs.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(ex) = existing {
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
        }
    }
    Ok(())
}

async fn sync_collections(
    db: &DatabaseConnection,
    library_dir: &Path,
) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_collection_sidecars(library_dir)?;
    for sc in sidecars {
        let existing = collection::Entity::find()
            .filter(collection::Column::Slug.eq(&sc.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        let col_id = if let Some(ex) = existing {
            collection::ActiveModel {
                id: Set(ex.id.clone()),
                slug: Set(sc.slug.clone()),
                title: Set(sc.title.clone()),
                description: Set(sc.description.clone()),
                background_path: Set(None),
                created_at: Set(ex.created_at.clone()),
                updated_at: Set(now),
            }
            .update(db)
            .await?;
            ex.id
        } else {
            let id = Uuid::new_v4().to_string();
            collection::ActiveModel {
                id: Set(id.clone()),
                slug: Set(sc.slug.clone()),
                title: Set(sc.title.clone()),
                description: Set(sc.description.clone()),
                background_path: Set(None),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
            id
        };

        // Re-sync clip membership (position = index in sidecar clips array)
        collection_clip::Entity::delete_many()
            .filter(collection_clip::Column::CollectionId.eq(&col_id))
            .exec(db)
            .await?;

        for (pos, clip_slug) in sc.clips.iter().enumerate() {
            if let Some(c) = clip::Entity::find()
                .filter(clip::Column::Slug.eq(clip_slug.as_str()))
                .one(db)
                .await?
            {
                let _ = collection_clip::ActiveModel {
                    id: Set(Uuid::new_v4().to_string()),
                    collection_id: Set(col_id.clone()),
                    clip_id: Set(c.id.clone()),
                    position: Set(pos as i32),
                }
                .insert(db)
                .await;
            }
        }
    }
    Ok(())
}

async fn sync_presets(db: &DatabaseConnection, library_dir: &Path) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_preset_sidecars(library_dir)?;
    for sc in sidecars {
        let processors_json = serde_json::to_string(&sc.processors)?;
        let existing = preset::Entity::find()
            .filter(preset::Column::Slug.eq(&sc.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(ex) = existing {
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
        }
    }
    Ok(())
}
```

---

### Task 4: Implement thin CRUD services

These are stateless functions used by the CLI and (later) Tauri commands.

**Step 4.1** — Create `libs/musicum-core/src/services/file_service.rs`:
```rust
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::db::entities::{file, file_metadata};
use crate::ServiceError;

pub async fn list_files(db: &DatabaseConnection) -> Result<Vec<file::Model>, ServiceError> {
    Ok(file::Entity::find()
        .order_by_asc(file::Column::Name)
        .all(db)
        .await?)
}

pub async fn get_file_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<file::Model, ServiceError> {
    file::Entity::find()
        .filter(file::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("file '{slug}'")))
}

pub async fn get_file_metadata(
    db: &DatabaseConnection,
    file_id: &str,
) -> Result<Option<file_metadata::Model>, ServiceError> {
    Ok(file_metadata::Entity::find_by_id(file_id).one(db).await?)
}
```

**Step 4.2** — Create `libs/musicum-core/src/services/clip_service.rs`:
```rust
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::db::entities::clip;
use crate::ServiceError;

pub async fn list_clips_for_file(
    db: &DatabaseConnection,
    file_id: &str,
) -> Result<Vec<clip::Model>, ServiceError> {
    Ok(clip::Entity::find()
        .filter(clip::Column::FileId.eq(file_id))
        .order_by_asc(clip::Column::Title)
        .all(db)
        .await?)
}

pub async fn get_clip_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<clip::Model, ServiceError> {
    clip::Entity::find()
        .filter(clip::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{slug}'")))
}
```

**Step 4.3** — Create `libs/musicum-core/src/services/collection_service.rs`:
```rust
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::db::entities::collection;
use crate::ServiceError;

pub async fn list_collections(
    db: &DatabaseConnection,
) -> Result<Vec<collection::Model>, ServiceError> {
    Ok(collection::Entity::find()
        .order_by_asc(collection::Column::Title)
        .all(db)
        .await?)
}

pub async fn get_collection_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<collection::Model, ServiceError> {
    collection::Entity::find()
        .filter(collection::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("collection '{slug}'")))
}
```

**Step 4.4** — Create `libs/musicum-core/src/services/preset_service.rs`:
```rust
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::db::entities::preset;
use crate::ServiceError;

pub async fn list_presets(db: &DatabaseConnection) -> Result<Vec<preset::Model>, ServiceError> {
    Ok(preset::Entity::find()
        .order_by_asc(preset::Column::Title)
        .all(db)
        .await?)
}

pub async fn get_preset_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<preset::Model, ServiceError> {
    preset::Entity::find()
        .filter(preset::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("preset '{slug}'")))
}
```

---

### Task 5: Update module declarations

**Step 5.1** — Replace `libs/musicum-core/src/services/mod.rs`:
```rust
pub mod clip_service;
pub mod collection_service;
pub mod file_service;
pub mod preset_service;
pub mod sync_service;
```

**Step 5.2** — Add `sidecar` to `libs/musicum-core/src/lib.rs`:
```rust
pub mod audio;
pub mod db;
pub mod error;
pub mod services;
pub mod sidecar;

pub use error::ServiceError;
```

---

### Task 6: Write integration tests with real WAV files

Tests generate synthetic WAV files using `hound` (added as a dev-dependency in Plan 01). This gives you a real WAV file on disk — symphonia can decode it, the sync service can hash and probe it, and sidecars are written next to it.

**Step 6.1** — Create `libs/musicum-core/tests/common/mod.rs`:
```rust
use hound::{SampleFormat, WavSpec, WavWriter};
use std::f32::consts::PI;
use std::path::{Path, PathBuf};

/// Write a mono 440 Hz sine wave WAV at `path` (16-bit PCM, 44100 Hz).
/// Duration in seconds. Returns the path for chaining.
pub fn write_sine_wav(path: &Path, duration_secs: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let num_samples = (spec.sample_rate as f32 * duration_secs) as u32;
    for i in 0..num_samples {
        let t = i as f32 / spec.sample_rate as f32;
        let sample = (2.0 * PI * 440.0 * t).sin();
        let pcm = (sample * i16::MAX as f32) as i16;
        writer.write_sample(pcm).unwrap();
    }
    writer.finalize().unwrap();
    path.to_path_buf()
}

/// Write a stereo WAV with white noise at `path` (16-bit PCM, 48000 Hz).
pub fn write_stereo_wav(path: &Path, duration_secs: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let num_samples = (spec.sample_rate as f32 * duration_secs) as u32;
    let mut rng: u32 = 0xdeadbeef;
    for _ in 0..num_samples {
        for _ in 0..2 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let pcm = ((rng as i32) >> 16) as i16;
            writer.write_sample(pcm).unwrap();
        }
    }
    writer.finalize().unwrap();
    path.to_path_buf()
}
```

**Step 6.2** — Create `libs/musicum-core/tests/sync_service.rs`:
```rust
mod common;

use musicum_core::{db, sidecar, services::sync_service};
use musicum_core::db::entities::{clip, file};
use sea_orm::EntityTrait;
use tempfile::tempdir;

async fn setup(lib_path: &std::path::Path) -> sea_orm::DatabaseConnection {
    db::connect(lib_path.to_str().unwrap()).await.unwrap()
}

#[tokio::test]
async fn sync_discovers_wav_file() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("kick.wav");
    common::write_sine_wav(&wav, 0.5);

    let db = setup(dir.path()).await;
    let stats = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(stats.added, 1, "should have found one new file");
    assert_eq!(stats.removed, 0);

    let files = file::Entity::find().all(&db).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "kick");
    assert_eq!(files[0].channels, 1);
    assert_eq!(files[0].sample_rate, 44100);
    assert!(files[0].duration > 0.4 && files[0].duration < 0.6);
}

#[tokio::test]
async fn sync_creates_sidecar_next_to_audio() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("pad.wav");
    common::write_stereo_wav(&wav, 1.0);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let sidecar_path = dir.path().join("pad.wav.musicum.json");
    assert!(sidecar_path.exists(), "sidecar should be created next to audio file");

    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.version, 1);
    assert!(sc.clips.is_empty());
}

#[tokio::test]
async fn sync_reads_existing_sidecar_with_clips() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("synth.wav");
    common::write_sine_wav(&wav, 2.0);

    // Pre-write a sidecar with one clip
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let sc = sidecar::FileSidecar {
        version: 1,
        metadata: sidecar::FileMetadataSidecar {
            bpm: Some(120.0),
            key: Some("C".into()),
            rating: Some(5),
            color: None,
            notes: "test note".into(),
            tags: "synth,pad".into(),
        },
        attachments: vec![],
        clips: vec![sidecar::ClipSidecar {
            slug: "synth-clean".into(),
            title: "Clean".into(),
            notes: String::new(),
            processors: vec![],
        }],
    };
    std::fs::write(
        &sidecar_path,
        serde_json::to_string_pretty(&sc).unwrap(),
    )
    .unwrap();

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let files = file::Entity::find().all(&db).await.unwrap();
    assert_eq!(files.len(), 1);

    let clips = clip::Entity::find().all(&db).await.unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].slug, "synth-clean");

    let meta = musicum_core::db::entities::file_metadata::Entity::find_by_id(&files[0].id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.bpm, Some(120.0));
    assert_eq!(meta.tags, "synth,pad");
}

#[tokio::test]
async fn sync_idempotent_on_unchanged_file() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("loop.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(dir.path()).await;

    // First sync
    let s1 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s1.added, 1);

    // Second sync — file unchanged
    let s2 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s2.added, 0, "no new files on second sync");
    assert_eq!(s2.updated, 0);
    assert_eq!(s2.removed, 0);

    // DB should still have exactly one file
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn sync_detects_removed_files() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("temp.wav");
    common::write_sine_wav(&wav, 0.3);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    // Delete the file from disk
    std::fs::remove_file(&wav).unwrap();

    let s2 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s2.removed, 1);
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn sync_walks_subdirectories() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("drums")).unwrap();
    common::write_sine_wav(&dir.path().join("drums").join("kick.wav"), 0.1);
    common::write_sine_wav(&dir.path().join("drums").join("snare.wav"), 0.1);
    common::write_sine_wav(&dir.path().join("pad.wav"), 1.0);

    let db = setup(dir.path()).await;
    let stats = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(stats.added, 3, "should find files in subdirectories too");
}

#[tokio::test]
async fn sync_preset_sidecar() {
    let dir = tempdir().unwrap();

    // Write a preset sidecar
    let preset_sc = sidecar::PresetSidecar {
        version: 1,
        slug: "lo-fi".into(),
        title: "Lo-Fi".into(),
        description: "lo-fi preset".into(),
        processors: vec![sidecar::ProcessorEntry {
            kind: "plugin".into(),
            id: "gain".into(),
            enabled: true,
            params: serde_json::json!({ "gain": 0.5 }),
        }],
    };
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let presets = musicum_core::db::entities::preset::Entity::find()
        .all(&db)
        .await
        .unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].slug, "lo-fi");
}
```

**Step 6.3** — Run the tests:
```bash
cargo test -p musicum-core --test sync_service
# Expected: 7 tests pass
```

If `sync_discovers_wav_file` fails with a symphonia decode error, check that the WAV file is properly formed (hound produces valid PCM WAVs that symphonia supports).

If `sync_reads_existing_sidecar_with_clips` panics at `serde_json::from_str`, enable `RUST_LOG=debug` and check the sidecar JSON written to disk looks valid.

---

### Task 7: Verify everything together

**Step 7.1** — Run all musicum-core tests:
```bash
cargo test -p musicum-core
# Expected: All db_schema and sync_service tests pass (≥12 tests total)
```

**Step 7.2** — Quick sanity check — workspace still compiles cleanly:
```bash
cargo check --workspace
```

---

## What's next

Plan 04 builds the CLI binary (`musicum`) that exposes all sync and CRUD operations via `clap` subcommands.
