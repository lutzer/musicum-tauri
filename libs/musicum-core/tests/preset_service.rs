mod common;

use musicum_core::{db, sidecar::{self, ProcessorEntry, ProcessorRef}, services::preset_service};
use tempfile::tempdir;

async fn setup() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = db::connect(dir.path().to_str().unwrap()).await.unwrap();
    (db, dir)
}

#[tokio::test]
async fn create_preset_writes_sidecar_and_db() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    let model = preset_service::create_preset(&db, lib, "my-preset", "My Preset", "").await.unwrap();

    assert_eq!(model.slug, "my-preset");
    assert_eq!(model.title, "My Preset");

    // Sidecar exists
    let sc = sidecar::read_preset_sidecar(dir.path(), "my-preset").unwrap();
    assert_eq!(sc.slug, "my-preset");
    assert!(sc.processors.is_empty());
}

#[tokio::test]
async fn create_preset_errors_if_sidecar_exists() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "dup", "Dup", "").await.unwrap();
    let err = preset_service::create_preset(&db, lib, "dup", "Dup", "").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn delete_preset_removes_sidecar_and_db_row() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "gone", "Gone", "").await.unwrap();
    preset_service::delete_preset(&db, lib, "gone").await.unwrap();

    // Sidecar gone
    assert!(sidecar::read_preset_sidecar(dir.path(), "gone").is_err());
    // DB row gone
    let err = preset_service::get_preset_by_slug(&db, "gone").await.unwrap_err();
    assert!(matches!(err, musicum_core::ServiceError::NotFound(_)));
}

#[tokio::test]
async fn update_preset_processors_persists_to_db() {
    let (db, dir) = setup().await;
    let lib = dir.path().to_str().unwrap();

    preset_service::create_preset(&db, lib, "p1", "P1", "").await.unwrap();

    let processors = vec![ProcessorEntry::Structural {
        id: "uuid-abc".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    }];

    preset_service::update_preset_processors(&db, lib, "p1", processors).await.unwrap();

    let model = preset_service::get_preset_by_slug(&db, "p1").await.unwrap();
    assert!(model.processors.contains("trim"));
}
