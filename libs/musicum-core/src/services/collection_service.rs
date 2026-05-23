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
