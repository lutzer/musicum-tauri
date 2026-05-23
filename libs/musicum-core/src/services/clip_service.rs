use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use slug::slugify;
use uuid::Uuid;
use std::path::Path;

use crate::db::entities::clip;
use crate::sidecar::{self, ClipSidecar};
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
        cached: Set("no_cache".to_string()),
        cached_path: Set(None),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}
