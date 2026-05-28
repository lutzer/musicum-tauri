use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use sea_orm::sea_query::Expr;
use slug::slugify;
use uuid::Uuid;

use crate::db::entities::clip;
use crate::db::entities::collection;
use crate::db::entities::collection_clip;
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

pub async fn create_collection(
    db: &DatabaseConnection,
    title: &str,
    description: &str,
) -> Result<collection::Model, ServiceError> {
    let slug = slugify(title);

    let existing = collection::Entity::find()
        .filter(collection::Column::Slug.eq(&slug))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(ServiceError::InvalidInput(format!(
            "collection with slug '{slug}' already exists"
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let model = collection::ActiveModel {
        id:              Set(Uuid::new_v4().to_string()),
        slug:            Set(slug),
        title:           Set(title.to_string()),
        description:     Set(description.to_string()),
        background_path: Set(None),
        created_at:      Set(now.clone()),
        updated_at:      Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub async fn set_collection_description(
    db: &DatabaseConnection,
    slug: &str,
    description: &str,
) -> Result<(), ServiceError> {
    let col = get_collection_by_slug(db, slug).await?;
    let now = chrono::Utc::now().to_rfc3339();
    collection::ActiveModel {
        id:              Set(col.id),
        slug:            Set(col.slug),
        title:           Set(col.title),
        description:     Set(description.to_string()),
        background_path: Set(col.background_path),
        created_at:      Set(col.created_at),
        updated_at:      Set(now),
    }
    .update(db)
    .await?;
    Ok(())
}

pub async fn delete_collection(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<(), ServiceError> {
    let col = get_collection_by_slug(db, slug).await?;

    collection_clip::Entity::delete_many()
        .filter(collection_clip::Column::CollectionId.eq(&col.id))
        .exec(db)
        .await?;

    collection::Entity::delete_by_id(&col.id).exec(db).await?;
    Ok(())
}

pub async fn add_clip_to_collection(
    db: &DatabaseConnection,
    collection_slug: &str,
    clip_slug: &str,
    position: Option<i32>,
) -> Result<collection_clip::Model, ServiceError> {
    let col = get_collection_by_slug(db, collection_slug).await?;

    let clip_row = clip::Entity::find()
        .filter(clip::Column::Slug.eq(clip_slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{clip_slug}'")))?;

    let existing = collection_clip::Entity::find()
        .filter(collection_clip::Column::CollectionId.eq(&col.id))
        .filter(collection_clip::Column::ClipId.eq(&clip_row.id))
        .one(db)
        .await?;
    if existing.is_some() {
        return Err(ServiceError::InvalidInput(format!(
            "clip '{clip_slug}' is already in collection '{collection_slug}'"
        )));
    }

    let pos = match position {
        None => {
            let count = collection_clip::Entity::find()
                .filter(collection_clip::Column::CollectionId.eq(&col.id))
                .count(db)
                .await? as i32;
            count + 1
        }
        Some(n) => {
            collection_clip::Entity::update_many()
                .col_expr(
                    collection_clip::Column::Position,
                    Expr::col(collection_clip::Column::Position).add(1),
                )
                .filter(collection_clip::Column::CollectionId.eq(&col.id))
                .filter(collection_clip::Column::Position.gte(n))
                .exec(db)
                .await?;
            n
        }
    };

    let model = collection_clip::ActiveModel {
        id:            Set(Uuid::new_v4().to_string()),
        collection_id: Set(col.id),
        clip_id:       Set(clip_row.id),
        position:      Set(pos),
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub async fn remove_clip_from_collection(
    db: &DatabaseConnection,
    collection_slug: &str,
    clip_slug: &str,
) -> Result<(), ServiceError> {
    let col = get_collection_by_slug(db, collection_slug).await?;

    let clip_row = clip::Entity::find()
        .filter(clip::Column::Slug.eq(clip_slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{clip_slug}'")))?;

    let join_row = collection_clip::Entity::find()
        .filter(collection_clip::Column::CollectionId.eq(&col.id))
        .filter(collection_clip::Column::ClipId.eq(&clip_row.id))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!(
            "clip '{clip_slug}' is not a member of collection '{collection_slug}'"
        )))?;

    collection_clip::Entity::delete_by_id(&join_row.id)
        .exec(db)
        .await?;

    let remaining = collection_clip::Entity::find()
        .filter(collection_clip::Column::CollectionId.eq(&col.id))
        .order_by_asc(collection_clip::Column::Position)
        .all(db)
        .await?;

    for (i, row) in remaining.iter().enumerate() {
        collection_clip::ActiveModel {
            id:            Set(row.id.clone()),
            collection_id: Set(row.collection_id.clone()),
            clip_id:       Set(row.clip_id.clone()),
            position:      Set((i + 1) as i32),
        }
        .update(db)
        .await?;
    }

    Ok(())
}

pub async fn get_collection_with_clips(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<(collection::Model, Vec<clip::Model>), ServiceError> {
    let col = get_collection_by_slug(db, slug).await?;

    let pairs = collection_clip::Entity::find()
        .find_also_related(clip::Entity)
        .filter(collection_clip::Column::CollectionId.eq(&col.id))
        .order_by_asc(collection_clip::Column::Position)
        .all(db)
        .await?;

    let clips: Vec<clip::Model> = pairs
        .into_iter()
        .filter_map(|(_, c)| c)
        .collect();

    Ok((col, clips))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;

    #[tokio::test]
    async fn create_collection_stores_row() {
        let db = test_db().await;
        let col = create_collection(&db, "My Mix", "").await.unwrap();
        assert_eq!(col.slug, "my-mix");
        assert_eq!(col.title, "My Mix");
        assert_eq!(col.description, "");
    }

    #[tokio::test]
    async fn create_collection_rejects_duplicate_slug() {
        let db = test_db().await;
        create_collection(&db, "My Mix", "").await.unwrap();
        let err = create_collection(&db, "My Mix", "second").await.unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn set_collection_description_updates_field() {
        let db = test_db().await;
        create_collection(&db, "My Mix", "old").await.unwrap();
        set_collection_description(&db, "my-mix", "new desc").await.unwrap();
        let col = get_collection_by_slug(&db, "my-mix").await.unwrap();
        assert_eq!(col.description, "new desc");
    }

    #[tokio::test]
    async fn delete_collection_removes_row() {
        let db = test_db().await;
        create_collection(&db, "My Mix", "").await.unwrap();
        delete_collection(&db, "my-mix").await.unwrap();
        let err = get_collection_by_slug(&db, "my-mix").await.unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    use crate::db::entities::{clip, file};

    async fn insert_file_and_clip(db: &DatabaseConnection, clip_slug: &str) -> clip::Model {
        let now = chrono::Utc::now().to_rfc3339();
        let file_id = Uuid::new_v4().to_string();
        file::ActiveModel {
            id:          Set(file_id.clone()),
            slug:        Set(format!("file-{clip_slug}")),
            name:        Set(format!("file-{clip_slug}")),
            path:        Set(format!("/tmp/{clip_slug}.wav")),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set(Uuid::new_v4().to_string()),
            created_at:  Set(now.clone()),
            updated_at:  Set(now.clone()),
        }
        .insert(db)
        .await
        .unwrap();

        clip::ActiveModel {
            id:         Set(Uuid::new_v4().to_string()),
            slug:       Set(clip_slug.to_string()),
            file_id:    Set(file_id),
            title:      Set(clip_slug.to_string()),
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
    async fn add_clip_appends_to_end() {
        let db = test_db().await;
        create_collection(&db, "Mix", "").await.unwrap();
        insert_file_and_clip(&db, "clip-a").await;
        insert_file_and_clip(&db, "clip-b").await;

        let r1 = add_clip_to_collection(&db, "mix", "clip-a", None).await.unwrap();
        let r2 = add_clip_to_collection(&db, "mix", "clip-b", None).await.unwrap();

        assert_eq!(r1.position, 1);
        assert_eq!(r2.position, 2);
    }

    #[tokio::test]
    async fn add_clip_at_position_shifts_others() {
        let db = test_db().await;
        create_collection(&db, "Mix", "").await.unwrap();
        insert_file_and_clip(&db, "clip-a").await;
        insert_file_and_clip(&db, "clip-b").await;
        insert_file_and_clip(&db, "clip-c").await;

        add_clip_to_collection(&db, "mix", "clip-a", None).await.unwrap(); // pos 1
        add_clip_to_collection(&db, "mix", "clip-b", None).await.unwrap(); // pos 2
        add_clip_to_collection(&db, "mix", "clip-c", Some(1)).await.unwrap(); // insert at 1

        let (_, clips) = get_collection_with_clips(&db, "mix").await.unwrap();
        assert_eq!(clips[0].slug, "clip-c");
        assert_eq!(clips[1].slug, "clip-a");
        assert_eq!(clips[2].slug, "clip-b");
    }

    #[tokio::test]
    async fn add_clip_rejects_duplicate() {
        let db = test_db().await;
        create_collection(&db, "Mix", "").await.unwrap();
        insert_file_and_clip(&db, "clip-a").await;

        add_clip_to_collection(&db, "mix", "clip-a", None).await.unwrap();
        let err = add_clip_to_collection(&db, "mix", "clip-a", None).await.unwrap_err();
        assert!(matches!(err, ServiceError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn remove_clip_renumbers_remaining() {
        let db = test_db().await;
        create_collection(&db, "Mix", "").await.unwrap();
        insert_file_and_clip(&db, "clip-a").await;
        insert_file_and_clip(&db, "clip-b").await;
        insert_file_and_clip(&db, "clip-c").await;

        add_clip_to_collection(&db, "mix", "clip-a", None).await.unwrap(); // pos 1
        add_clip_to_collection(&db, "mix", "clip-b", None).await.unwrap(); // pos 2
        add_clip_to_collection(&db, "mix", "clip-c", None).await.unwrap(); // pos 3

        remove_clip_from_collection(&db, "mix", "clip-b").await.unwrap();

        let (_, clips) = get_collection_with_clips(&db, "mix").await.unwrap();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].slug, "clip-a");
        assert_eq!(clips[1].slug, "clip-c");
    }

    #[tokio::test]
    async fn remove_clip_not_member_returns_not_found() {
        let db = test_db().await;
        create_collection(&db, "Mix", "").await.unwrap();
        insert_file_and_clip(&db, "clip-a").await;

        let err = remove_clip_from_collection(&db, "mix", "clip-a").await.unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_collection_with_clips_returns_ordered() {
        let db = test_db().await;
        create_collection(&db, "Mix", "cool mix").await.unwrap();
        insert_file_and_clip(&db, "clip-x").await;
        insert_file_and_clip(&db, "clip-y").await;

        add_clip_to_collection(&db, "mix", "clip-x", None).await.unwrap();
        add_clip_to_collection(&db, "mix", "clip-y", None).await.unwrap();

        let (col, clips) = get_collection_with_clips(&db, "mix").await.unwrap();
        assert_eq!(col.description, "cool mix");
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].slug, "clip-x");
        assert_eq!(clips[1].slug, "clip-y");
    }
}
