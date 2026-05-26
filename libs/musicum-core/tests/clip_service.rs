mod common;

use musicum_core::{db, sidecar, services::{clip_service, sync_service}};
use musicum_core::db::entities::clip;
use musicum_core::ServiceError;
use sea_orm::EntityTrait;
use tempfile::tempdir;

async fn setup_with_file(paths: &musicum_core::config::LibraryPaths, filename: &str) -> sea_orm::DatabaseConnection {
    let wav = paths.files_dir.join(filename);
    common::write_sine_wav(&wav, 0.5);
    let db = db::connect(&paths.catalog_dir).await.unwrap();
    sync_service::sync_library(&db, paths, || ()).await.unwrap();
    db
}

#[tokio::test]
async fn create_clip_adds_to_db_and_sidecar() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let db = setup_with_file(&paths, "kick.wav").await;
    let wav = paths.files_dir.join("kick.wav");

    let clip = clip_service::create_clip(&db, "kick", "My Clip").await.unwrap();

    assert_eq!(clip.slug, "my-clip");
    assert_eq!(clip.title, "My Clip");
    assert_eq!(clip.cached, "no_cache");

    // Clip is in DB
    let all = clip::Entity::find().all(&db).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].slug, "my-clip");

    // Clip is in sidecar
    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.clips.len(), 1);
    assert_eq!(sc.clips[0].slug, "my-clip");
    assert_eq!(sc.clips[0].title, "My Clip");
    assert!(sc.clips[0].processors.is_empty());
}

#[tokio::test]
async fn create_clip_file_not_found() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let db = db::connect(&paths.catalog_dir).await.unwrap();

    let err = clip_service::create_clip(&db, "nonexistent", "Foo").await.unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

#[tokio::test]
async fn create_clip_slug_collision() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("pad.wav");
    common::write_sine_wav(&wav, 0.5);

    // Pre-write sidecar with a clip whose slug would collide
    let sc = sidecar::FileSidecar {
        version: 1,
        metadata: Default::default(),
        attachments: vec![],
        clips: vec![sidecar::ClipSidecar {
            slug: "my-clip".into(),
            title: "My Clip".into(),
            notes: String::new(),
            processors: vec![],
        }],
    };
    sidecar::write_file_sidecar(&wav, &sc).unwrap();

    let db = db::connect(&paths.catalog_dir).await.unwrap();
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let err = clip_service::create_clip(&db, "pad", "My Clip").await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}
