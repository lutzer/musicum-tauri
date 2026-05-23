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
