use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "file")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub name: String,
    pub path: String,
    pub duration: f64,
    pub sample_rate: i32,
    pub channels: i32,
    pub mime_type: String,
    pub hash: String,
    pub mtime: String,
    pub size_bytes: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::file_metadata::Entity")]
    FileMetadata,
    #[sea_orm(has_many = "super::file_attachment::Entity")]
    FileAttachment,
    #[sea_orm(has_many = "super::clip::Entity")]
    Clip,
}

impl ActiveModelBehavior for ActiveModel {}
