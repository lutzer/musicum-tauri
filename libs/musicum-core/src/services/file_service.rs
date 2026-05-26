use std::path::Path;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::db::entities::{clip, file, file_metadata};
use crate::services::sync_service;
use crate::sidecar;
use crate::ServiceError;

pub async fn list_files(db: &DatabaseConnection) -> Result<Vec<file::Model>, ServiceError> {
    Ok(file::Entity::find()
        .order_by_asc(file::Column::Name)
        .all(db)
        .await?)
}

pub async fn get_file_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<file::Model, ServiceError> {
    file::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("file '{id}'")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
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
}
