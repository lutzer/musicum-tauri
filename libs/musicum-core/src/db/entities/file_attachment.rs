use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_attachment")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub file_id: String,
    #[sea_orm(column_name = "type")]
    pub attachment_type: String,
    pub text: Option<String>,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileId",
        to = "super::file::Column::Id"
    )]
    File,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
