use anyhow::Result;
use musicum_core::{deserialize_processor_edits, services::preset_service};
use sea_orm::DatabaseConnection;

use super::processor_list_editor::{run, SaveFn};

pub async fn run_editor(
    db: &DatabaseConnection,
    preset_slug: &str,
) -> Result<()> {
    let preset = preset_service::get_preset_by_slug(db, preset_slug).await?;
    let processors = deserialize_processor_edits(&preset.processors);

    let save: SaveFn<'_> = Box::new(|procs| {
        Box::pin(async move {
            preset_service::update_preset_processors_full(db, preset_slug, procs)
                .await
                .map_err(anyhow::Error::from)
        })
    });

    run(&format!("Preset: {preset_slug}"), processors, save).await
}
