# Library Structure & Config Redesign Implementation Plan

**Goal:** Replace the flat `.musicum/` hidden-directory layout with explicit `files/`, `catalog/`, and `.generated/` subdirectories, introduce `~/.musicum/config.toml`, make collections/presets database-only (no sidecars), and move all path-resolution logic into `musicum-core`.

**Architecture:** A new `musicum_core::config` module owns `AppSettings` (TOML-deserialised), `LibraryPaths` (derived resolved paths), and the `load()` / template-write logic. Sync walks `files_dir` only — collections and presets are never touched by sync. Preset service becomes fully DB-only. All core services switch from `library_dir: &str` to `catalog_dir: &Path` or `&LibraryPaths`. The CLI `settings.rs` is deleted; `main.rs` calls `musicum_core::config::load()`.

**Tech Stack:** Rust, `toml 0.8` crate (new dep on musicum-core), `serde/serde_derive` (already present), `tempfile` (tests, already present).

---

## File map

| Status | Path | Responsibility |
|---|---|---|
| **New** | `libs/musicum-core/src/config.rs` | `AppSettings`, `LibraryConfig`, `LibraryPaths`, `load()`, `config_path()` |
| Modify | `libs/musicum-core/Cargo.toml` | add `toml = "0.8"` |
| Modify | `libs/musicum-core/src/lib.rs` | expose `pub mod config` |
| Modify | `libs/musicum-core/src/db/mod.rs` | `connect(catalog_dir: &Path)` |
| Modify | `libs/musicum-core/src/sidecar.rs` | remove `CollectionSidecar`, `PresetSidecar`, and all five collection/preset helpers |
| Modify | `libs/musicum-core/src/services/sync_service.rs` | walks `files_dir`; drops `sync_collections`/`sync_presets`; drops `presets_added`/`presets_updated` from `SyncReport` |
| Modify | `libs/musicum-core/src/services/preset_service.rs` | fully DB-only; drop all `library_dir` / sidecar params |
| Modify | `libs/musicum-core/src/services/clip_service.rs` | drop unused `_library_dir: &str` param from `update_clip_processors` |
| Modify | `libs/musicum-core/tests/common/mod.rs` | add `make_paths(base)` helper |
| Modify | `libs/musicum-core/tests/sync_service.rs` | use `make_paths`; remove preset-sidecar tests |
| Modify | `libs/musicum-core/tests/clip_service.rs` | use `make_paths`; update `db::connect` call |
| Modify | `libs/musicum-core/tests/preset_service.rs` | use `test_db()`; remove sidecar assertions; drop dir args |
| **Delete** | `apps/cli/src/settings.rs` | replaced by `musicum_core::config` |
| Modify | `apps/cli/src/main.rs` | use `musicum_core::config::load()`; pass `&paths` |
| Modify | `apps/cli/src/commands/sync.rs` | take `paths: &LibraryPaths`; remove preset summary lines |
| Modify | `apps/cli/src/commands/presets.rs` | take `catalog_dir: &Path`; `AddProcessor`/`RemoveProcessor` read from DB |
| Modify | `apps/cli/src/commands/presets_editor.rs` | drop `library_dir` param |
| Modify | `apps/cli/src/commands/clips.rs` | drop `library_dir` param |
| Modify | `apps/cli/src/commands/collections.rs` | update empty-list message |

---

## Task 1: Add `toml` dependency

**Files:**
- Modify: `libs/musicum-core/Cargo.toml`

In `[dependencies]`, after `hex = "0.4"` add:
```toml
toml        = "0.8"
```

Run:
```
cargo build -p musicum-core
```
Expected: compiles (no code changes yet).

---

## Task 2: Create `musicum-core/src/config.rs`

**Files:**
- Create: `libs/musicum-core/src/config.rs`

```rust
use std::path::{Path, PathBuf};
use serde::Deserialize;
use anyhow::{Context, Result};

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub library: LibraryConfig,
}

#[derive(Debug, Deserialize)]
pub struct LibraryConfig {
    pub dir: String,
    pub files_dir: Option<String>,
    pub catalog_dir: Option<String>,
    pub generated_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LibraryPaths {
    pub library_dir:   PathBuf,
    pub files_dir:     PathBuf,
    pub catalog_dir:   PathBuf,
    pub generated_dir: PathBuf,
}

pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".musicum").join("config.toml")
}

pub fn load() -> Result<AppSettings> {
    let path = config_path();
    if !path.exists() {
        write_default_config(&path)?;
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config from {}", path.display()))?;
    toml::from_str(&text).context("parsing config.toml")
}

fn write_default_config(path: &Path) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let default_dir = format!("{home}/Musik/musicum");
    let template = format!(
        r#"# Musicum configuration

[library]
dir = "{default_dir}"

# Override individual subdirectories (uncomment to customize)
# files_dir = "{default_dir}/files"
# catalog_dir = "{default_dir}/catalog"
# generated_dir = "{default_dir}/.generated"
"#
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, template)
        .with_context(|| format!("writing default config to {}", path.display()))
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(s)
    }
}

impl AppSettings {
    pub fn library_paths(&self) -> LibraryPaths {
        let library_dir = expand_tilde(&self.library.dir);
        let files_dir = self.library.files_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("files"));
        let catalog_dir = self.library.catalog_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("catalog"));
        let generated_dir = self.library.generated_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join(".generated"));
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}

impl LibraryPaths {
    pub fn from_override(library_dir: &str) -> Self {
        let library_dir = expand_tilde(library_dir);
        let files_dir     = library_dir.join("files");
        let catalog_dir   = library_dir.join("catalog");
        let generated_dir = library_dir.join(".generated");
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn library_paths_defaults_from_dir() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: None,
                catalog_dir: None,
                generated_dir: None,
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/mylib/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/mylib/.generated"));
    }

    #[test]
    fn library_paths_respects_overrides() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: Some("/mnt/audio".into()),
                catalog_dir: None,
                generated_dir: Some("/mnt/gen".into()),
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/mnt/audio"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/mnt/gen"));
    }

    #[test]
    fn from_override_ignores_subdirs() {
        let paths = LibraryPaths::from_override("/tmp/override");
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/override/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/override/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/override/.generated"));
    }

    #[test]
    fn load_writes_default_config_if_missing() {
        let dir = tempdir().unwrap();
        std::env::set_var("HOME", dir.path().to_str().unwrap());
        let settings = load().unwrap();
        assert!(config_path().exists(), "default config should be written");
        assert!(settings.library.dir.contains("Musik"));
    }
}
```

---

## Task 3: Expose config module in lib.rs

**Files:**
- Modify: `libs/musicum-core/src/lib.rs`

Add after `pub mod audio;`:
```rust
pub mod config;
```

Run:
```
cargo test -p musicum-core config
```
Expected: all 4 tests pass.

---

## Task 4: Update `db::connect()` signature

**Files:**
- Modify: `libs/musicum-core/src/db/mod.rs`

Replace the `connect` function signature and body opening:
```rust
// Replace this:
pub async fn connect(library_dir: &str) -> Result<DatabaseConnection, ServiceError> {
    let db_path = format!("{library_dir}/.musicum/musicum.db");

    let dir = std::path::Path::new(&db_path).parent().unwrap();
    std::fs::create_dir_all(dir)?;

    let url = format!("sqlite://{db_path}?mode=rwc");

// With this:
pub async fn connect(catalog_dir: &std::path::Path) -> Result<DatabaseConnection, ServiceError> {
    std::fs::create_dir_all(catalog_dir)?;
    let db_path = catalog_dir.join("musicum.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
```

The rest of the function body is unchanged. The compiler will now flag every call site — fix them in subsequent tasks.

---

## Task 5: Strip collection/preset code from `sidecar.rs`

**Files:**
- Modify: `libs/musicum-core/src/sidecar.rs`

Delete the entire `// ── Collection sidecar ───` block (lines defining `CollectionSidecar`).

Delete the entire `// ── Preset sidecar ───` block (lines defining `PresetSidecar`).

Delete these five functions entirely:
- `read_collection_sidecars`
- `write_collection_sidecar`
- `read_preset_sidecars`
- `read_preset_sidecar`
- `write_preset_sidecar`

Keep everything else (`ProcessorRef`, `ProcessorEntry`, `FileSidecar`, `FileMetadataSidecar`, `AttachmentSidecar`, `ClipSidecar`, `read_file_sidecar`, `write_file_sidecar`, `sidecar_path_for_audio`).

Run:
```
cargo build -p musicum-core
```
Expected: compiler errors at call sites in `sync_service.rs` and `preset_service.rs` — fix in the next two tasks.

---

## Task 6: Rewrite `sync_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs`

**1. Update imports** — remove `collection`, `preset`; keep `collection_clip` (used in `delete_file_cascade`):
```rust
// Replace:
use crate::db::entities::{clip, collection, collection_clip, file, file_attachment, file_metadata, preset};
use crate::sidecar::{self, ClipSidecar, FileSidecar};

// With:
use crate::db::entities::{clip, collection_clip, file, file_attachment, file_metadata};
use crate::sidecar::{self, ClipSidecar, FileSidecar};
```

**2. Update `count_audio_files`** — takes `files_dir: &Path`; skip dot-dirs and `catalog`:
```rust
// Replace entire function:
pub fn count_audio_files(files_dir: &Path) -> Result<usize, ServiceError> {
    let count = WalkDir::new(files_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "catalog" { return false; }
            }
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            AUDIO_EXTENSIONS.contains(&ext.as_str())
        })
        .count();
    Ok(count)
}
```

**3. Update `SyncReport`** — drop preset fields:
```rust
// Replace:
pub struct SyncReport {
    pub files_added:      Vec<String>,
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,
    pub presets_added:    Vec<String>,
    pub presets_updated:  Vec<String>,
}

// With:
pub struct SyncReport {
    pub files_added:      Vec<String>,
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,
}
```

**4. Update `sync_library` signature and body**:
```rust
// Replace signature and lib_path line:
pub async fn sync_library(
    db: &DatabaseConnection,
    paths: &crate::config::LibraryPaths,
    on_progress: impl Fn(),
) -> Result<SyncReport, ServiceError> {
    let lib_path = &paths.files_dir;
```

Replace the `.musicum` skip inside the walker:
```rust
// Replace:
if path.components().any(|c| c.as_os_str() == ".musicum") {
    continue;
}

// With:
if path.is_dir() {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') || name == "catalog" { continue; }
}
```

Remove the two lines at the end of `sync_library` that call `sync_collections` and `sync_presets`:
```rust
// Delete these two lines:
sync_collections(db, lib_path).await?;
sync_presets(db, lib_path, &mut report).await?;
```

**5. Delete `sync_collections` and `sync_presets` functions entirely** — remove both async functions.

---

## Task 7: Rewrite `preset_service.rs` as DB-only

**Files:**
- Modify: `libs/musicum-core/src/services/preset_service.rs`

**1. Update imports**:
```rust
// Replace:
use std::path::Path;
use crate::sidecar::{self, PresetSidecar};

// With:
use crate::sidecar;
```

**2. Replace `create_preset`** — check DB for duplicates, no sidecar:
```rust
pub async fn create_preset(
    db: &DatabaseConnection,
    slug: &str,
    title: &str,
    description: &str,
) -> Result<preset::Model, ServiceError> {
    if preset::Entity::find()
        .filter(preset::Column::Slug.eq(slug))
        .one(db)
        .await?
        .is_some()
    {
        return Err(ServiceError::InvalidInput(format!(
            "preset '{slug}' already exists"
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let model = preset::ActiveModel {
        id:          Set(Uuid::new_v4().to_string()),
        slug:        Set(slug.to_string()),
        title:       Set(title.to_string()),
        description: Set(description.to_string()),
        processors:  Set("[]".to_string()),
        created_at:  Set(now.clone()),
        updated_at:  Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}
```

**3. Replace `delete_preset`** — DB only, no sidecar:
```rust
pub async fn delete_preset(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    preset::Entity::delete_by_id(model.id).exec(db).await?;
    Ok(())
}
```

**4. Replace `set_processor_param`** — read processors from DB:
```rust
pub async fn set_processor_param(
    db: &DatabaseConnection,
    preset_slug: &str,
    instance_uuid: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, preset_slug).await?;
    let mut processors: Vec<sidecar::ProcessorEntry> =
        serde_json::from_str(&model.processors)
            .map_err(|e| ServiceError::InvalidInput(format!("invalid processors JSON: {e}")))?;

    let found = processors.iter_mut().find(|e| {
        let id = match e {
            sidecar::ProcessorEntry::Structural { id, .. } => id.as_str(),
            sidecar::ProcessorEntry::AudioPlugin { id, .. } => id.as_str(),
        };
        id == instance_uuid
    });

    let entry = found.ok_or_else(|| {
        ServiceError::NotFound(format!("processor '{instance_uuid}' in preset '{preset_slug}'"))
    })?;

    let params = match entry {
        sidecar::ProcessorEntry::Structural { processor, .. } => &mut processor.params,
        sidecar::ProcessorEntry::AudioPlugin { processor, .. } => &mut processor.params,
    };
    if let Some(map) = params.as_object_mut() {
        map.insert(key.to_string(), value);
    }

    update_preset_processors(db, preset_slug, processors).await
}
```

**5. Replace `update_preset_processors_full`** — delegate directly to `update_preset_processors`:
```rust
pub async fn update_preset_processors_full(
    db: &DatabaseConnection,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError> {
    update_preset_processors(db, slug, processors).await
}
```

**6. Remove `_library_dir` param from `update_preset_processors`**:
```rust
// Replace signature:
pub async fn update_preset_processors(
    db: &DatabaseConnection,
    _library_dir: &str,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError> {

// With:
pub async fn update_preset_processors(
    db: &DatabaseConnection,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
) -> Result<(), ServiceError> {
```

---

## Task 8: Drop `_library_dir` from `clip_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

Replace the `update_clip_processors` signature:
```rust
// Replace:
pub async fn update_clip_processors(
    db: &DatabaseConnection,
    _library_dir: &str,
    clip_slug: &str,
    processors: Vec<ProcessorEntry>,
) -> Result<(), ServiceError> {

// With:
pub async fn update_clip_processors(
    db: &DatabaseConnection,
    clip_slug: &str,
    processors: Vec<ProcessorEntry>,
) -> Result<(), ServiceError> {
```

Run:
```
cargo build -p musicum-core
```
Expected: clean build. Any remaining errors are in the test files.

---

## Task 9: Add `make_paths` helper to `tests/common/mod.rs`

**Files:**
- Modify: `libs/musicum-core/tests/common/mod.rs`

Append at the bottom of the file:
```rust
pub fn make_paths(base: &std::path::Path) -> musicum_core::config::LibraryPaths {
    let files_dir     = base.join("files");
    let catalog_dir   = base.join("catalog");
    let generated_dir = base.join(".generated");
    std::fs::create_dir_all(&files_dir).unwrap();
    std::fs::create_dir_all(&catalog_dir).unwrap();
    std::fs::create_dir_all(&generated_dir).unwrap();
    musicum_core::config::LibraryPaths {
        library_dir:   base.to_path_buf(),
        files_dir,
        catalog_dir,
        generated_dir,
    }
}
```

---

## Task 10: Rewrite `tests/sync_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/sync_service.rs`

Replace the entire file with:

```rust
mod common;

use musicum_core::{db, sidecar, services::sync_service};
use musicum_core::db::entities::{clip, file};
use sea_orm::{EntityTrait, PaginatorTrait};
use tempfile::tempdir;

async fn setup(paths: &musicum_core::config::LibraryPaths) -> sea_orm::DatabaseConnection {
    db::connect(&paths.catalog_dir).await.unwrap()
}

#[tokio::test]
async fn sync_discovers_wav_file() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("kick.wav");
    common::write_sine_wav(&wav, 0.5);

    let db = setup(&paths).await;
    let stats = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(stats.files_added.len(), 1, "should have found one new file");
    assert!(stats.files_removed.is_empty());

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
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("pad.wav");
    common::write_stereo_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let sidecar_path = paths.files_dir.join("pad.wav.musicum.json");
    assert!(sidecar_path.exists(), "sidecar should be created next to audio file");

    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.version, 1);
    assert!(sc.clips.is_empty());
}

#[tokio::test]
async fn sync_reads_existing_sidecar_with_clips() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("synth.wav");
    common::write_sine_wav(&wav, 2.0);

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
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

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
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("loop.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;

    let s1 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(s1.files_added.len(), 1);

    let s2 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert!(s2.files_added.is_empty(), "no new files on second sync");
    assert!(s2.files_updated.is_empty());
    assert!(s2.files_removed.is_empty());

    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn sync_detects_removed_files() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("temp.wav");
    common::write_sine_wav(&wav, 0.3);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    std::fs::remove_file(&wav).unwrap();

    let s2 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(s2.files_removed.len(), 1);
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn sync_walks_subdirectories() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    std::fs::create_dir(paths.files_dir.join("drums")).unwrap();
    common::write_sine_wav(&paths.files_dir.join("drums").join("kick.wav"), 0.1);
    common::write_sine_wav(&paths.files_dir.join("drums").join("snare.wav"), 0.1);
    common::write_sine_wav(&paths.files_dir.join("pad.wav"), 1.0);

    let db = setup(&paths).await;
    let stats = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(stats.files_added.len(), 3, "should find files in subdirectories too");
}

#[tokio::test]
async fn sync_picks_up_sidecar_metadata_when_audio_unchanged() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    sc.metadata.key = Some("Am".into());
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let files = musicum_core::db::entities::file::Entity::find()
        .all(&db).await.unwrap();
    let meta = musicum_core::db::entities::file_metadata::Entity::find_by_id(&files[0].id)
        .one(&db).await.unwrap().unwrap();
    assert_eq!(meta.bpm, Some(140.0));
    assert_eq!(meta.key.as_deref(), Some("Am"));
}

#[tokio::test]
async fn report_tracks_sidecar_metadata_update() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    let report = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(report.sidecars_updated, vec!["bass"]);
    assert!(report.files_added.is_empty());
    assert!(report.files_updated.is_empty());
}

#[tokio::test]
async fn report_sidecar_unchanged_is_silent() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("pad.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let report = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert!(report.sidecars_updated.is_empty());
    assert!(report.files_added.is_empty());
}
```

Note: `sync_preset_sidecar`, `sync_picks_up_updated_preset_sidecar`, and `report_tracks_preset_added_and_updated` are deleted — presets no longer participate in sync.

Run:
```
cargo test -p musicum-core sync_service
```
Expected: all tests pass.

---

## Task 11: Update `tests/clip_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/clip_service.rs`

Replace `setup_with_file`:
```rust
async fn setup_with_file(paths: &musicum_core::config::LibraryPaths, filename: &str) -> sea_orm::DatabaseConnection {
    let wav = paths.files_dir.join(filename);
    common::write_sine_wav(&wav, 0.5);
    let db = db::connect(&paths.catalog_dir).await.unwrap();
    sync_service::sync_library(&db, paths, || ()).await.unwrap();
    db
}
```

In `create_clip_adds_to_db_and_sidecar`:
```rust
let dir = tempdir().unwrap();
let paths = common::make_paths(dir.path());
let db = setup_with_file(&paths, "kick.wav").await;
let wav = paths.files_dir.join("kick.wav");
```

In `create_clip_file_not_found`:
```rust
let dir = tempdir().unwrap();
let paths = common::make_paths(dir.path());
let db = db::connect(&paths.catalog_dir).await.unwrap();
```

In `create_clip_slug_collision`:
```rust
let dir = tempdir().unwrap();
let paths = common::make_paths(dir.path());
let wav = paths.files_dir.join("pad.wav");
common::write_sine_wav(&wav, 0.5);
// (write sidecar to wav as before)
let db = db::connect(&paths.catalog_dir).await.unwrap();
sync_service::sync_library(&db, &paths, || ()).await.unwrap();
```

Run:
```
cargo test -p musicum-core clip_service
```
Expected: all tests pass.

---

## Task 12: Rewrite `tests/preset_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/preset_service.rs`

Replace the entire file with:

```rust
mod common;

use musicum_core::{db, sidecar::{ProcessorEntry, ProcessorRef}, services::preset_service};

async fn setup() -> sea_orm::DatabaseConnection {
    db::test_db().await
}

#[tokio::test]
async fn create_preset_writes_db() {
    let db = setup().await;

    let model = preset_service::create_preset(&db, "my-preset", "My Preset", "").await.unwrap();

    assert_eq!(model.slug, "my-preset");
    assert_eq!(model.title, "My Preset");
    assert_eq!(model.processors, "[]");
}

#[tokio::test]
async fn create_preset_errors_if_slug_exists() {
    let db = setup().await;

    preset_service::create_preset(&db, "dup", "Dup", "").await.unwrap();
    let err = preset_service::create_preset(&db, "dup", "Dup", "").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn delete_preset_removes_db_row() {
    let db = setup().await;

    preset_service::create_preset(&db, "gone", "Gone", "").await.unwrap();
    preset_service::delete_preset(&db, "gone").await.unwrap();

    let err = preset_service::get_preset_by_slug(&db, "gone").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn update_preset_processors_persists_to_db() {
    let db = setup().await;

    preset_service::create_preset(&db, "p1", "P1", "").await.unwrap();

    let processors = vec![ProcessorEntry::Structural {
        id: "uuid-abc".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    }];

    preset_service::update_preset_processors(&db, "p1", processors).await.unwrap();

    let model = preset_service::get_preset_by_slug(&db, "p1").await.unwrap();
    assert!(model.processors.contains("trim"));
}
```

Run:
```
cargo test -p musicum-core preset_service
```
Expected: all 4 tests pass.

---

## Task 13: Run full core test suite

```
cargo test -p musicum-core
```
Expected: all tests pass, zero warnings in changed code.

---

## Task 14: Delete `apps/cli/src/settings.rs`

Delete the file:
```
rm apps/cli/src/settings.rs
```

The compiler will flag usages in `main.rs` — fix in the next task.

---

## Task 15: Update `apps/cli/src/main.rs`

**Files:**
- Modify: `apps/cli/src/main.rs`

Replace the entire file with:

```rust
mod commands;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use musicum_core::config::{self, LibraryPaths};

#[derive(Parser)]
#[command(
    name = "musicum",
    about = "Musicum audio library CLI",
    version
)]
struct Cli {
    /// Override the library directory for this invocation
    #[arg(long, global = true)]
    library: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Walk the library directory and sync DB + sidecars
    Sync,
    /// File operations
    Files(commands::files::FilesArgs),
    /// Clip operations
    Clips(commands::clips::ClipsArgs),
    /// Collection operations
    Collections(commands::collections::CollectionsArgs),
    /// Preset operations
    Presets(commands::presets::PresetsArgs),
    /// List registered structural processors
    Processors(commands::processors::ProcessorsArgs),
    /// Play a file or clip (slug or file path)
    Play {
        /// Slug or file path to play
        target: String,
        /// Resolve target as a file slug (skips clip lookup)
        #[arg(long, conflicts_with = "clip")]
        file: bool,
        /// Resolve target as a clip slug (skips file lookup)
        #[arg(long, conflicts_with = "file")]
        clip: bool,
        /// Start playback with looping enabled
        #[arg(long = "loop")]
        loop_mode: bool,
    },
    /// Print config and resolved library paths
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let paths = if let Some(lib) = cli.library {
        LibraryPaths::from_override(&lib)
    } else {
        config::load()?.library_paths()
    };

    if let Commands::Config = cli.command {
        println!("Config file:   {}", config::config_path().display());
        println!("Library dir:   {}", paths.library_dir.display());
        println!("Files dir:     {}", paths.files_dir.display());
        println!("Catalog dir:   {}", paths.catalog_dir.display());
        println!("Generated dir: {}", paths.generated_dir.display());
        return Ok(());
    }

    let db = musicum_core::db::connect(&paths.catalog_dir).await?;

    match cli.command {
        Commands::Sync              => commands::sync::run(&db, &paths).await?,
        Commands::Files(args)       => commands::files::run(&db, args).await?,
        Commands::Clips(args)       => commands::clips::run(&db, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args)     => commands::presets::run(&db, &paths.catalog_dir, args).await?,
        Commands::Processors(args)  => commands::processors::run(args),
        Commands::Play { target, file, clip, loop_mode } => {
            commands::play::run(&db, target, file, clip, loop_mode).await?
        }
        Commands::Config => unreachable!(),
    }

    Ok(())
}
```

---

## Task 16: Update `apps/cli/src/commands/sync.rs`

**Files:**
- Modify: `apps/cli/src/commands/sync.rs`

Replace the entire file with:

```rust
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use musicum_core::config::LibraryPaths;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, paths: &LibraryPaths) -> Result<()> {
    println!("Syncing library: {}", paths.library_dir.display());

    let total = sync_service::count_audio_files(&paths.files_dir).unwrap_or(0);

    let pb = if total > 0 {
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "  {bar:40.cyan/blue} {pos}/{len}  {elapsed_precise}"
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        bar
    } else {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} scanning…  {elapsed_precise}")
                .unwrap(),
        );
        bar
    };

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

---

## Task 17: Update `apps/cli/src/commands/presets.rs`

**Files:**
- Modify: `apps/cli/src/commands/presets.rs`

Key changes:
- Signature: `library_dir: &str` → `catalog_dir: &std::path::Path`
- Remove `use std::path::Path;`
- Change `use musicum_core::sidecar::{self, ProcessorEntry, ProcessorRef};` → `use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};`
- `Create`: `create_preset(db, library_dir, ...)` → `create_preset(db, ...)`
- `Delete`: `delete_preset(db, library_dir, ...)` → `delete_preset(db, ...)`
- `AddProcessor`: replace sidecar read/write with DB read/write (see below)
- `RemoveProcessor`: replace sidecar read/write with DB read/write (see below)
- `Edit`: `run_editor(db, library_dir, &slug)` → `run_editor(db, &slug)`
- `SetParam`: `set_processor_param(db, library_dir, ...)` → `set_processor_param(db, ...)`
- Empty list message: `"No presets. Create one with 'presets create --title <name>'."`

Replace `AddProcessor` match arm body (after building `instance_id` and `new_entry`):
```rust
let preset = preset_service::get_preset_by_slug(db, &preset_slug).await?;
let mut processors: Vec<ProcessorEntry> =
    serde_json::from_str(&preset.processors).unwrap_or_default();
processors.push(new_entry);
preset_service::update_preset_processors(db, &preset_slug, processors).await?;
```

Replace `RemoveProcessor` match arm body:
```rust
let preset = preset_service::get_preset_by_slug(db, &preset_slug).await?;
let mut processors: Vec<ProcessorEntry> =
    serde_json::from_str(&preset.processors).unwrap_or_default();
let original_len = processors.len();
processors.retain(|e| {
    let id = match e {
        ProcessorEntry::Structural { id, .. } => id.as_str(),
        ProcessorEntry::AudioPlugin { id, .. } => id.as_str(),
    };
    id != instance_uuid
});
if processors.len() == original_len {
    bail!("processor '{instance_uuid}' not found in preset '{preset_slug}'");
}
preset_service::update_preset_processors(db, &preset_slug, processors).await?;
```

---

## Task 18: Update `apps/cli/src/commands/presets_editor.rs`

**Files:**
- Modify: `apps/cli/src/commands/presets_editor.rs`

Replace the entire file with:

```rust
use anyhow::Result;
use musicum_core::services::preset_service;
use sea_orm::DatabaseConnection;

use super::processor_list_editor::{run, SaveFn};

pub async fn run_editor(
    db: &DatabaseConnection,
    preset_slug: &str,
) -> Result<()> {
    let preset = preset_service::get_preset_by_slug(db, preset_slug).await?;
    let processors = serde_json::from_str(&preset.processors).unwrap_or_default();

    let save: SaveFn<'_> = Box::new(|procs| {
        Box::pin(async move {
            preset_service::update_preset_processors_full(db, preset_slug, procs)
                .await
                .map_err(anyhow::Error::from)
        })
    });

    run(&format!("Preset: {preset_slug}"), processors, save).await
}
```

---

## Task 19: Update `apps/cli/src/commands/clips.rs`

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

Remove `library_dir: &str` from `run` signature:
```rust
// Replace:
pub async fn run(db: &DatabaseConnection, library_dir: &str, args: ClipsArgs) -> Result<()> {

// With:
pub async fn run(db: &DatabaseConnection, args: ClipsArgs) -> Result<()> {
```

Update the three `update_clip_processors` calls — remove the `library_dir` argument:
```rust
// ApplyPreset:
clip_service::update_clip_processors(db, &clip_slug, new_processors).await?;

// ClearProcessors:
clip_service::update_clip_processors(db, &clip_slug, vec![]).await?;

// Edit (inside the SaveFn closure):
clip_service::update_clip_processors(db, &slug, procs).await?;
```

---

## Task 20: Update `apps/cli/src/commands/collections.rs`

**Files:**
- Modify: `apps/cli/src/commands/collections.rs`

Replace the empty-list message:
```rust
// Replace:
println!("No collections. Add a sidecar under .musicum/collections/ and run sync.");

// With:
println!("No collections.");
```

---

## Task 21: Final build, lint, and smoke test

```
cargo clippy --all 2>&1 | head -60
```
Expected: zero errors, zero warnings in changed files.

```
cargo test -p musicum-core
```
Expected: all tests pass.

```
cargo build
```
Expected: clean build.

```
cargo run -p musicum-cli -- config
```
Expected output (paths vary by HOME):
```
Config file:   /Users/<you>/.musicum/config.toml
Library dir:   /Users/<you>/Musik/musicum
Files dir:     /Users/<you>/Musik/musicum/files
Catalog dir:   /Users/<you>/Musik/musicum/catalog
Generated dir: /Users/<you>/Musik/musicum/.generated
```
