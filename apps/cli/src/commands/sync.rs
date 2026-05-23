use anyhow::Result;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

use crate::settings::AppSettings;

pub async fn run(db: &DatabaseConnection, settings: &AppSettings) -> Result<()> {
    println!("Syncing library: {}", settings.library_dir);
    let stats = sync_service::sync_library(db, &settings.library_dir).await?;
    println!(
        "Done — added: {}, updated: {}, removed: {}",
        stats.added, stats.updated, stats.removed
    );
    Ok(())
}
