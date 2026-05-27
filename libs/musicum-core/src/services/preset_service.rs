use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder,
};
use uuid::Uuid;

use crate::db::entities::preset;
use crate::edit::{EditKind, ProcessorEdit};
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

pub async fn create_preset(
    db: &DatabaseConnection,
    slug: &str,
    title: &str,
    description: &str,
) -> Result<preset::Model, ServiceError> {
    if preset::Entity::find()
        .filter(preset::Column::Slug.eq(slug))
        .one(db)
        .await?
        .is_some()
    {
        return Err(ServiceError::InvalidInput(format!(
            "preset '{slug}' already exists"
        )));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let model = preset::ActiveModel {
        id:          Set(Uuid::new_v4().to_string()),
        slug:        Set(slug.to_string()),
        title:       Set(title.to_string()),
        description: Set(description.to_string()),
        processors:  Set("[]".to_string()),
        created_at:  Set(now.clone()),
        updated_at:  Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}

pub async fn delete_preset(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    preset::Entity::delete_by_id(model.id).exec(db).await?;
    Ok(())
}

pub async fn set_processor_param(
    db: &DatabaseConnection,
    preset_slug: &str,
    instance_uuid: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, preset_slug).await?;
    let mut processors: Vec<ProcessorEdit> =
        crate::edit::deserialize_processor_edits(&model.processors);

    let found = processors.iter_mut().find(|e| {
        e.uuid.to_string() == instance_uuid
    });

    let entry = found.ok_or_else(|| {
        ServiceError::NotFound(format!("processor '{instance_uuid}' in preset '{preset_slug}'"))
    })?;

    // Update params based on kind
    match &mut entry.kind {
        EditKind::Structural { params, .. } => {
            if let Some(v) = value.as_f64() {
                params.insert(key.to_string(), v);
            }
        }
        EditKind::Plugin { params, .. } => {
            if let Some(v) = value.as_f64() {
                params.insert(key.to_string(), v as f32);
            }
        }
    }

    update_preset_processors(db, preset_slug, processors).await
}

pub async fn update_preset_processors_full(
    db: &DatabaseConnection,
    slug: &str,
    processors: Vec<ProcessorEdit>,
) -> Result<(), ServiceError> {
    update_preset_processors(db, slug, processors).await
}

pub async fn update_preset_processors(
    db: &DatabaseConnection,
    slug: &str,
    processors: Vec<ProcessorEdit>,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    let processors_json = serde_json::to_string(&processors)?;
    let now = chrono::Utc::now().to_rfc3339();
    preset::ActiveModel {
        id:          Set(model.id),
        slug:        Set(model.slug),
        title:       Set(model.title),
        description: Set(model.description),
        processors:  Set(processors_json),
        created_at:  Set(model.created_at),
        updated_at:  Set(now),
    }
    .update(db)
    .await?;
    Ok(())
}
