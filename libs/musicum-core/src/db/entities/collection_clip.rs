use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_clip")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub collection_id: String,
    pub clip_id: String,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::collection::Entity",
        from = "Column::CollectionId",
        to = "super::collection::Column::Id"
    )]
    Collection,
    #[sea_orm(
        belongs_to = "super::clip::Entity",
        from = "Column::ClipId",
        to = "super::clip::Column::Id"
    )]
    Clip,
}

impl Related<super::collection::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Collection.def()
    }
}

impl Related<super::clip::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Clip.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
