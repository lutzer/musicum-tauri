use std::path::Path;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder,
};
use uuid::Uuid;

use crate::db::entities::preset;
use crate::sidecar::{self, PresetSidecar};
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
    library_dir: &str,
    slug: &str,
    title: &str,
    description: &str,
) -> Result<preset::Model, ServiceError> {
    let lib = Path::new(library_dir);
    let sidecar_path = lib
        .join(".musicum")
        .join("presets")
        .join(format!("{slug}.musicum-preset.json"));
    if sidecar_path.exists() {
        return Err(ServiceError::InvalidInput(format!(
            "preset '{slug}' already exists"
        )));
    }

    let sc = PresetSidecar {
        version: 1,
        slug: slug.to_string(),
        title: title.to_string(),
        description: description.to_string(),
        processors: vec![],
    };
    sidecar::write_preset_sidecar(lib, &sc)?;

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
    library_dir: &str,
    slug: &str,
) -> Result<(), ServiceError> {
    let model = get_preset_by_slug(db, slug).await?;
    let lib = Path::new(library_dir);
    let sidecar_path = lib
        .join(".musicum")
        .join("presets")
        .join(format!("{slug}.musicum-preset.json"));
    if sidecar_path.exists() {
        std::fs::remove_file(&sidecar_path)?;
    }
    preset::Entity::delete_by_id(model.id).exec(db).await?;
    Ok(())
}

pub async fn update_preset_processors(
    db: &DatabaseConnection,
    _library_dir: &str,
    slug: &str,
    processors: Vec<sidecar::ProcessorEntry>,
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
