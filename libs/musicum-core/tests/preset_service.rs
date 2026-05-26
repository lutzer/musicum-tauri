mod common;

use musicum_core::{db, sidecar::{ProcessorEntry, ProcessorRef}, services::preset_service};
use tempfile::tempdir;

async fn setup() -> sea_orm::DatabaseConnection {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    // Keep dir alive by leaking — tests are short-lived
    std::mem::forget(dir);
    db::connect(&paths.catalog_dir).await.unwrap()
}

#[tokio::test]
async fn create_preset_writes_db() {
    let db = setup().await;

    let model = preset_service::create_preset(&db, "my-preset", "My Preset", "").await.unwrap();

    assert_eq!(model.slug, "my-preset");
    assert_eq!(model.title, "My Preset");
    assert_eq!(model.processors, "[]");
}

#[tokio::test]
async fn create_preset_errors_if_slug_exists() {
    let db = setup().await;

    preset_service::create_preset(&db, "dup", "Dup", "").await.unwrap();
    let err = preset_service::create_preset(&db, "dup", "Dup", "").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn delete_preset_removes_db_row() {
    let db = setup().await;

    preset_service::create_preset(&db, "gone", "Gone", "").await.unwrap();
    preset_service::delete_preset(&db, "gone").await.unwrap();

    let err = preset_service::get_preset_by_slug(&db, "gone").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn update_preset_processors_persists_to_db() {
    let db = setup().await;

    preset_service::create_preset(&db, "p1", "P1", "").await.unwrap();

    let processors = vec![ProcessorEntry::Structural {
        id: "uuid-abc".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    }];

    preset_service::update_preset_processors(&db, "p1", processors).await.unwrap();

    let model = preset_service::get_preset_by_slug(&db, "p1").await.unwrap();
    assert!(model.processors.contains("trim"));
}
