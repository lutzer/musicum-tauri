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
