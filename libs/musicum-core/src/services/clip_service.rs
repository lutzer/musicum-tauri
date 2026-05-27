use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use slug::slugify;
use uuid::Uuid;
use std::path::Path;

use crate::db::entities::{clip, collection_clip};
use crate::sidecar::{self, ClipSidecar, ProcessorEntry};
use crate::services::file_service;
use crate::ServiceError;

pub async fn list_all_clips(db: &DatabaseConnection) -> Result<Vec<clip::Model>, ServiceError> {
    Ok(clip::Entity::find()
        .order_by_asc(clip::Column::Title)
        .all(db)
        .await?)
}

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

pub async fn create_clip(
    db: &DatabaseConnection,
    file_slug: &str,
    title: &str,
) -> Result<clip::Model, ServiceError> {
    let file = file_service::get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;

    let clip_slug = slugify(title);

    if sc.clips.iter().any(|c| c.slug == clip_slug) {
        return Err(ServiceError::InvalidInput(format!(
            "clip with slug '{clip_slug}' already exists for this file"
        )));
    }

    sc.clips.push(ClipSidecar {
        slug: clip_slug.clone(),
        title: title.to_string(),
        notes: String::new(),
        processors: vec![],
    });

    sidecar::write_file_sidecar(audio_path, &sc)?;

    let now = chrono::Utc::now().to_rfc3339();
    let model = clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set(clip_slug),
        file_id: Set(file.id),
        title: Set(title.to_string()),
        processors: Set("[]".to_string()),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub async fn update_clip_processors(
    db: &DatabaseConnection,
    clip_slug: &str,
    processors: Vec<ProcessorEntry>,
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
    entry.processors = processors.clone();
    sidecar::write_file_sidecar(audio_path, &sc)?;

    let processors_json = serde_json::to_string(&processors)?;
    let now = chrono::Utc::now().to_rfc3339();
    clip::ActiveModel {
        id:          Set(clip.id),
        slug:        Set(clip.slug),
        file_id:     Set(clip.file_id),
        title:       Set(clip.title),
        processors: Set(processors_json),
        duration:   Set(clip.duration),
        notes:      Set(clip.notes),
        created_at: Set(clip.created_at),
        updated_at: Set(now),
    }
    .update(db)
    .await?;

    Ok(())
}

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
        processors: Set(clip.processors),
        duration:   Set(clip.duration),
        notes:      Set(notes.to_string()),
        created_at: Set(clip.created_at),
        updated_at: Set(now),
    }
    .update(db)
    .await?;

    Ok(())
}

pub async fn delete_clip(
    db: &DatabaseConnection,
    clip_slug: &str,
) -> Result<(), ServiceError> {
    let clip = get_clip_by_slug(db, clip_slug).await?;
    let file = file_service::get_file_by_id(db, &clip.file_id).await?;
    let audio_path = Path::new(&file.path);

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
            id:         Set(uuid::Uuid::new_v4().to_string()),
            slug:       Set("my-clip".to_string()),
            file_id:    Set(file_id),
            title:      Set("My Clip".to_string()),
            processors: Set("[]".to_string()),
            duration:   Set(None),
            notes:      Set(String::new()),
            created_at: Set(now.clone()),
            updated_at: Set(now),
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
        setup(&db, &audio).await;

        set_clip_notes(&db, "my-clip", "great drop").await.unwrap();

        let updated = get_clip_by_slug(&db, "my-clip").await.unwrap();
        assert_eq!(updated.notes, "great drop");

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.clips[0].notes, "great drop");
    }

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
}
