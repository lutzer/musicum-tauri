# Annotations & Deletions Implementation Plan

**Goal:** Add `set-notes`/`set-tags` commands for files, `set-notes` for clips, `delete` commands for files and clips, and rename `presets remove` to `presets delete`.

**Architecture:** All business logic goes in `musicum-core` services. Sidecar is written first (it is the source of truth), DB updated after. `delete_file_cascade` in `sync_service` is made `pub(crate)` and reused by the new `file_service::delete_file`. A `test_db()` helper in `db/mod.rs` provides an in-memory SQLite for service tests.

**Tech Stack:** Rust, SeaORM 1 + SQLite, serde_json, tempfile (tests)

---

### Task 1: Add `test_db` helper to `db/mod.rs`

**Files:**
- Modify: `libs/musicum-core/src/db/mod.rs`

Add at the bottom of the file:

```rust
#[cfg(test)]
pub async fn test_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    create_all_tables(&db).await.unwrap();
    db
}
```

Run:
```
cargo test -p musicum-core
```
Expected: all existing tests still pass (the new function is test-only dead code until used).

---

### Task 2: Make `delete_file_cascade` pub(crate) in `sync_service`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:324`

Change:
```rust
async fn delete_file_cascade(db: &DatabaseConnection, file_id: &str) -> Result<(), ServiceError> {
```
To:
```rust
pub(crate) async fn delete_file_cascade(db: &DatabaseConnection, file_id: &str) -> Result<(), ServiceError> {
```

Run:
```
cargo build -p musicum-core
```
Expected: compiles with no errors.

---

### Task 3: Add `set_file_notes` and `set_file_tags` to `file_service`

**Files:**
- Modify: `libs/musicum-core/src/services/file_service.rs`

Add at the top of file_service.rs, extend the existing imports:
```rust
use std::path::Path;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use crate::db::entities::{file, file_metadata};
use crate::sidecar;
use crate::ServiceError;
```

Add these two functions after the existing `get_file_metadata`:

```rust
pub async fn set_file_notes(
    db: &DatabaseConnection,
    file_slug: &str,
    notes: &str,
) -> Result<(), ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    sc.metadata.notes = notes.to_string();
    sidecar::write_file_sidecar(audio_path, &sc)?;
    upsert_file_metadata_notes_tags(db, &file.id, Some(notes), None).await
}

pub async fn set_file_tags(
    db: &DatabaseConnection,
    file_slug: &str,
    tags: &str,
) -> Result<(), ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    sc.metadata.tags = tags.to_string();
    sidecar::write_file_sidecar(audio_path, &sc)?;
    upsert_file_metadata_notes_tags(db, &file.id, None, Some(tags)).await
}

// Sets notes and/or tags on file_metadata, creating the row if absent.
// Pass None for a field to leave it unchanged (or use empty string for initial insert).
async fn upsert_file_metadata_notes_tags(
    db: &DatabaseConnection,
    file_id: &str,
    notes: Option<&str>,
    tags: Option<&str>,
) -> Result<(), ServiceError> {
    let existing = file_metadata::Entity::find_by_id(file_id).one(db).await?;
    if let Some(ex) = existing {
        file_metadata::ActiveModel {
            file_id: Set(ex.file_id),
            bpm:     Set(ex.bpm),
            key:     Set(ex.key),
            rating:  Set(ex.rating),
            color:   Set(ex.color),
            notes:   Set(notes.map(str::to_string).unwrap_or(ex.notes)),
            tags:    Set(tags.map(str::to_string).unwrap_or(ex.tags)),
        }
        .update(db)
        .await?;
    } else {
        file_metadata::ActiveModel {
            file_id: Set(file_id.to_string()),
            bpm:     Set(None),
            key:     Set(None),
            rating:  Set(None),
            color:   Set(None),
            notes:   Set(notes.unwrap_or("").to_string()),
            tags:    Set(tags.unwrap_or("").to_string()),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}
```

Write the failing tests first (add at the bottom of file_service.rs):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
    use crate::db::entities::file;
    use sea_orm::ActiveModelTrait;
    use tempfile::tempdir;

    async fn insert_test_file(db: &DatabaseConnection, path: &str) -> file::Model {
        let now = chrono::Utc::now().to_rfc3339();
        file::ActiveModel {
            id:          Set(uuid::Uuid::new_v4().to_string()),
            slug:        Set("test-file".to_string()),
            name:        Set("test-file".to_string()),
            path:        Set(path.to_string()),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set("abc".to_string()),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn set_file_notes_creates_row_if_missing() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        let file = insert_test_file(&db, audio.to_str().unwrap()).await;

        set_file_notes(&db, "test-file", "my notes").await.unwrap();

        let meta = file_metadata::Entity::find_by_id(&file.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(meta.notes, "my notes");

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.metadata.notes, "my notes");
    }

    #[tokio::test]
    async fn set_file_notes_preserves_other_fields() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        let file = insert_test_file(&db, audio.to_str().unwrap()).await;

        file_metadata::ActiveModel {
            file_id: Set(file.id.clone()),
            bpm:     Set(Some(120.0)),
            key:     Set(None),
            rating:  Set(None),
            color:   Set(None),
            notes:   Set("old".to_string()),
            tags:    Set("kick".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();

        set_file_notes(&db, "test-file", "new notes").await.unwrap();

        let meta = file_metadata::Entity::find_by_id(&file.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(meta.notes, "new notes");
        assert_eq!(meta.bpm, Some(120.0));   // untouched
        assert_eq!(meta.tags, "kick");        // untouched
    }

    #[tokio::test]
    async fn set_file_tags_creates_row_if_missing() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        set_file_tags(&db, "test-file", "kick, loop").await.unwrap();

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.metadata.tags, "kick, loop");
    }
}
```

Run failing tests:
```
cargo test -p musicum-core services::file_service
```
Expected: tests fail (functions not yet implemented).

Add the implementation code. Run tests again:
```
cargo test -p musicum-core services::file_service
```
Expected: all three tests pass.

---

### Task 4: Add `delete_file` to `file_service`

**Files:**
- Modify: `libs/musicum-core/src/services/file_service.rs`

Add the import at the top (extend existing imports):
```rust
use crate::db::entities::{clip, file_metadata};
use crate::services::sync_service;
```

Add function after `set_file_tags`:

```rust
pub async fn delete_file(
    db: &DatabaseConnection,
    file_slug: &str,
    delete_audio: bool,
) -> Result<usize, ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);

    // Collect clip cached paths before cascade deletes them
    let clips = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file.id))
        .all(db)
        .await?;
    let clip_count = clips.len();

    // Delete cached clip audio files from disk
    for c in &clips {
        if let Some(ref cp) = c.cached_path {
            let _ = std::fs::remove_file(cp); // best-effort
        }
    }

    // Delete sidecar
    let sidecar_path = sidecar::sidecar_path_for_audio(audio_path);
    if sidecar_path.exists() {
        std::fs::remove_file(&sidecar_path)?;
    }

    // Cascade-delete all DB rows (collection_clip, clip, file_attachment, file_metadata, file)
    sync_service::delete_file_cascade(db, &file.id).await?;

    if delete_audio && audio_path.exists() {
        std::fs::remove_file(audio_path)?;
    }

    Ok(clip_count)
}
```

Write the failing test (add inside the existing `#[cfg(test)] mod tests` block):

```rust
    #[tokio::test]
    async fn delete_file_removes_db_and_sidecar() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        // Write a sidecar so we can verify it gets deleted
        let sc = sidecar::FileSidecar::default_for_file();
        sidecar::write_file_sidecar(&audio, &sc).unwrap();

        delete_file(&db, "test-file", false).await.unwrap();

        // DB entry gone
        assert!(get_file_by_slug(&db, "test-file").await.is_err());
        // Sidecar gone
        assert!(!sidecar::sidecar_path_for_audio(&audio).exists());
        // Audio still present
        assert!(audio.exists());
    }

    #[tokio::test]
    async fn delete_file_with_audio_flag_removes_audio() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        delete_file(&db, "test-file", true).await.unwrap();

        assert!(!audio.exists());
    }
```

Run failing tests:
```
cargo test -p musicum-core delete_file
```
Expected: fail.

Implement, then run:
```
cargo test -p musicum-core delete_file
```
Expected: both pass.

---

### Task 5: Add `set_clip_notes` to `clip_service`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

Add after `update_clip_processors`:

```rust
pub async fn set_clip_notes(
    db: &DatabaseConnection,
    clip_slug: &str,
    notes: &str,
) -> Result<(), ServiceError> {
    let clip = get_clip_by_slug(db, clip_slug).await?;
    let file = file_service::get_file_by_id(db, &clip.file_id).await?;
    let audio_path = Path::new(&file.path);

    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    let entry = sc
        .clips
        .iter_mut()
        .find(|c| c.slug == clip_slug)
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{clip_slug}' in sidecar")))?;
    entry.notes = notes.to_string();
    sidecar::write_file_sidecar(audio_path, &sc)?;

    let now = chrono::Utc::now().to_rfc3339();
    clip::ActiveModel {
        id:          Set(clip.id),
        slug:        Set(clip.slug),
        file_id:     Set(clip.file_id),
        title:       Set(clip.title),
        processors:  Set(clip.processors),
        cached:      Set(clip.cached),
        cached_path: Set(clip.cached_path),
        duration:    Set(clip.duration),
        notes:       Set(notes.to_string()),
        created_at:  Set(clip.created_at),
        updated_at:  Set(now),
    }
    .update(db)
    .await?;

    Ok(())
}
```

Write failing test (add at the bottom of clip_service.rs):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
    use crate::db::entities::file;
    use crate::sidecar::{ClipSidecar, FileSidecar, FileMetadataSidecar};
    use tempfile::tempdir;

    async fn setup(db: &DatabaseConnection, audio_path: &std::path::Path) -> clip::Model {
        let now = chrono::Utc::now().to_rfc3339();
        let file_id = uuid::Uuid::new_v4().to_string();
        file::ActiveModel {
            id:          Set(file_id.clone()),
            slug:        Set("test-file".to_string()),
            name:        Set("test-file".to_string()),
            path:        Set(audio_path.to_str().unwrap().to_string()),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set("abc".to_string()),
            created_at:  Set(now.clone()),
            updated_at:  Set(now.clone()),
        }
        .insert(db)
        .await
        .unwrap();

        let sc = FileSidecar {
            version: 1,
            metadata: FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![ClipSidecar {
                slug: "my-clip".to_string(),
                title: "My Clip".to_string(),
                notes: String::new(),
                processors: vec![],
            }],
        };
        sidecar::write_file_sidecar(audio_path, &sc).unwrap();

        clip::ActiveModel {
            id:          Set(uuid::Uuid::new_v4().to_string()),
            slug:        Set("my-clip".to_string()),
            file_id:     Set(file_id),
            title:       Set("My Clip".to_string()),
            processors:  Set("[]".to_string()),
            cached:      Set("no_cache".to_string()),
            cached_path: Set(None),
            duration:    Set(None),
            notes:       Set(String::new()),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn set_clip_notes_updates_db_and_sidecar() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        let clip = setup(&db, &audio).await;

        set_clip_notes(&db, "my-clip", "great drop").await.unwrap();

        let updated = get_clip_by_slug(&db, "my-clip").await.unwrap();
        assert_eq!(updated.notes, "great drop");

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.clips[0].notes, "great drop");
    }
}
```

Run failing test:
```
cargo test -p musicum-core set_clip_notes
```
Expected: fail.

Implement, then:
```
cargo test -p musicum-core set_clip_notes
```
Expected: passes.

---

### Task 6: Add `delete_clip` to `clip_service`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

Add import at the top (extend existing):
```rust
use crate::db::entities::{clip, collection_clip};
```

Add after `set_clip_notes`:

```rust
pub async fn delete_clip(
    db: &DatabaseConnection,
    clip_slug: &str,
) -> Result<(), ServiceError> {
    let clip = get_clip_by_slug(db, clip_slug).await?;
    let file = file_service::get_file_by_id(db, &clip.file_id).await?;
    let audio_path = Path::new(&file.path);

    // Delete cached audio file if present
    if let Some(ref cp) = clip.cached_path {
        let _ = std::fs::remove_file(cp); // best-effort
    }

    // Remove clip entry from sidecar
    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    sc.clips.retain(|c| c.slug != clip_slug);
    sidecar::write_file_sidecar(audio_path, &sc)?;

    // Remove from any collections
    collection_clip::Entity::delete_many()
        .filter(collection_clip::Column::ClipId.eq(&clip.id))
        .exec(db)
        .await?;

    // Delete clip DB row
    clip::Entity::delete_by_id(&clip.id).exec(db).await?;

    Ok(())
}
```

Write failing test (add inside the existing `#[cfg(test)] mod tests` block in clip_service.rs):

```rust
    #[tokio::test]
    async fn delete_clip_removes_db_and_sidecar_entry() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        setup(&db, &audio).await;

        delete_clip(&db, "my-clip").await.unwrap();

        assert!(get_clip_by_slug(&db, "my-clip").await.is_err());

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert!(sc.clips.is_empty());
    }
```

Run failing test:
```
cargo test -p musicum-core delete_clip
```
Expected: fail.

Implement, then:
```
cargo test -p musicum-core delete_clip
```
Expected: passes.

Run all core tests:
```
cargo test -p musicum-core
```
Expected: all pass.

---

### Task 7: Wire `files set-notes`, `files set-tags`, `files delete` in CLI

**Files:**
- Modify: `apps/cli/src/commands/files.rs`

Replace the `FilesCommand` enum and `run` function with the extended version:

Add to imports at the top:
```rust
use musicum_core::services::file_service;
```
(already imported — extend the existing use)

Add three new variants to `FilesCommand`:
```rust
    /// Set notes for a file (full replace)
    SetNotes {
        slug: String,
        notes: String,
    },
    /// Set tags for a file (full replace, comma-separated string)
    SetTags {
        slug: String,
        tags: String,
    },
    /// Delete a file from DB and remove its sidecar
    Delete {
        slug: String,
        /// Also delete the audio file from disk
        #[arg(long)]
        delete_audio: bool,
    },
```

Add three new match arms to `run`:
```rust
        FilesCommand::SetNotes { slug, notes } => {
            file_service::set_file_notes(db, &slug, &notes).await?;
            print_result("Set notes", &[Field("file", slug.clone())]);
        }
        FilesCommand::SetTags { slug, tags } => {
            file_service::set_file_tags(db, &slug, &tags).await?;
            print_result("Set tags", &[Field("file", slug.clone())]);
        }
        FilesCommand::Delete { slug, delete_audio } => {
            let clip_count = file_service::delete_file(db, &slug, delete_audio).await?;
            print_result("Deleted file", &[
                Field("slug", slug.clone()),
                Field("clips", clip_count.to_string()),
                Field("audio", if delete_audio { "deleted" } else { "kept" }.into()),
            ]);
        }
```

Verify:
```
cargo build -p musicum-cli
cargo run -p musicum-cli -- files --help
```
Expected: help text shows `set-notes`, `set-tags`, `delete` subcommands.

---

### Task 8: Wire `clips set-notes` and `clips delete` in CLI

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

Add two new variants to `ClipsCommand`:
```rust
    /// Set notes for a clip (full replace)
    SetNotes {
        slug: String,
        notes: String,
    },
    /// Delete a clip from DB and remove it from its sidecar
    Delete {
        slug: String,
    },
```

Add two new match arms to `run`:
```rust
        ClipsCommand::SetNotes { slug, notes } => {
            clip_service::set_clip_notes(db, &slug, &notes).await?;
            print_result("Set notes", &[Field("clip", slug.clone())]);
        }
        ClipsCommand::Delete { slug } => {
            clip_service::delete_clip(db, &slug).await?;
            print_result("Deleted clip", &[Field("slug", slug.clone())]);
        }
```

Verify:
```
cargo build -p musicum-cli
cargo run -p musicum-cli -- clips --help
```
Expected: help shows `set-notes` and `delete` subcommands.

---

### Task 9: Rename `presets remove` → `presets delete`

**Files:**
- Modify: `apps/cli/src/commands/presets.rs`

In the `PresetsCommand` enum, rename the variant:
```rust
    // Before:
    Remove { slug: String },

    // After:
    Delete { slug: String },
```

In the `run` match arm:
```rust
    // Before:
    PresetsCommand::Remove { slug } => { ... }

    // After:
    PresetsCommand::Delete { slug } => { ... }
```

Verify:
```
cargo build -p musicum-cli
cargo run -p musicum-cli -- presets --help
```
Expected: help shows `delete`, not `remove`.

---

### Task 10: Final checks

```
cargo clippy --all
```
Expected: no warnings.

```
cargo test -p musicum-core
```
Expected: all tests pass.

```
cargo build --release
```
Expected: clean build.
