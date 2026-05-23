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
