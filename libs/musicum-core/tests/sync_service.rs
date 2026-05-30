mod common;

use musicum_core::{db, sidecar, services::sync_service};
use musicum_core::db::entities::{clip, file};
use sea_orm::{EntityTrait, PaginatorTrait};
use tempfile::tempdir;

async fn setup(paths: &musicum_core::config::LibraryPaths) -> sea_orm::DatabaseConnection {
    db::connect(&paths.catalog_dir).await.unwrap()
}

#[tokio::test]
async fn sync_discovers_wav_file() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("kick.wav");
    common::write_sine_wav(&wav, 0.5);

    let db = setup(&paths).await;
    let stats = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(stats.files_added.len(), 1, "should have found one new file");
    assert!(stats.files_removed.is_empty());

    let files = file::Entity::find().all(&db).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "kick");
    assert_eq!(files[0].channels, 1);
    assert_eq!(files[0].sample_rate, 44100);
    assert!(files[0].duration > 0.4 && files[0].duration < 0.6);
}

#[tokio::test]
async fn sync_creates_sidecar_next_to_audio() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("pad.wav");
    common::write_stereo_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let sidecar_path = paths.files_dir.join("pad.wav.musicum.json");
    assert!(sidecar_path.exists(), "sidecar should be created next to audio file");

    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.version, 2);
    assert!(sc.clips.is_empty());
}

#[tokio::test]
async fn sync_reads_existing_sidecar_with_clips() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("synth.wav");
    common::write_sine_wav(&wav, 2.0);

    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let sc = sidecar::FileSidecar {
        id: String::new(),
        version: 1,
        metadata: sidecar::FileMetadataSidecar {
            bpm: Some(120.0),
            key: Some("C".into()),
            rating: Some(5),
            color: None,
            notes: "test note".into(),
            tags: "synth,pad".into(),
        },
        attachments: vec![],
        clips: vec![sidecar::ClipSidecar {
            slug: "synth-clean".into(),
            title: "Clean".into(),
            notes: String::new(),
            processors: vec![],
        }],
    };
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let files = file::Entity::find().all(&db).await.unwrap();
    assert_eq!(files.len(), 1);

    let clips = clip::Entity::find().all(&db).await.unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].slug, "synth-clean");

    let meta = musicum_core::db::entities::file_metadata::Entity::find_by_id(&files[0].id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.bpm, Some(120.0));
    assert_eq!(meta.tags, "synth,pad");
}

#[tokio::test]
async fn sync_idempotent_on_unchanged_file() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("loop.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;

    let s1 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(s1.files_added.len(), 1);

    let s2 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert!(s2.files_added.is_empty(), "no new files on second sync");
    assert!(s2.files_updated.is_empty());
    assert!(s2.files_removed.is_empty());

    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn sync_detects_removed_files() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("temp.wav");
    common::write_sine_wav(&wav, 0.3);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    std::fs::remove_file(&wav).unwrap();

    let s2 = sync_service::sync_library(&db, &paths, || ()).await.unwrap();
    assert_eq!(s2.files_removed.len(), 1);
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn sync_walks_subdirectories() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    std::fs::create_dir(paths.files_dir.join("drums")).unwrap();
    common::write_sine_wav(&paths.files_dir.join("drums").join("kick.wav"), 0.1);
    common::write_sine_wav(&paths.files_dir.join("drums").join("snare.wav"), 0.1);
    common::write_sine_wav(&paths.files_dir.join("pad.wav"), 1.0);

    let db = setup(&paths).await;
    let stats = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(stats.files_added.len(), 3, "should find files in subdirectories too");
}

// Sidecar-only changes are not picked up without touching the audio file (known limitation).
// Touching the audio file (rewriting its bytes) causes the fast-skip gate to fire on
// mtime change, which triggers a re-read of the sidecar.
#[tokio::test]
async fn sync_picks_up_sidecar_metadata_after_audio_touch() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    sc.metadata.key = Some("Am".into());
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    // Touch the audio file so the mtime-based fast-skip gate fires and re-reads the sidecar.
    let wav_bytes = std::fs::read(&wav).unwrap();
    std::fs::write(&wav, &wav_bytes).unwrap();

    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let files = musicum_core::db::entities::file::Entity::find()
        .all(&db).await.unwrap();
    let meta = musicum_core::db::entities::file_metadata::Entity::find_by_id(&files[0].id)
        .one(&db).await.unwrap().unwrap();
    assert_eq!(meta.bpm, Some(140.0));
    assert_eq!(meta.key.as_deref(), Some("Am"));
}

#[tokio::test]
async fn report_tracks_sidecar_metadata_update() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    // Touch the audio file so the mtime-based fast-skip gate fires and re-reads the sidecar.
    let wav_bytes = std::fs::read(&wav).unwrap();
    std::fs::write(&wav, &wav_bytes).unwrap();

    let report = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert_eq!(report.sidecars_updated, vec!["bass"]);
    assert!(report.files_added.is_empty());
    assert!(report.files_updated.is_empty());
}

#[tokio::test]
async fn report_sidecar_unchanged_is_silent() {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let wav = paths.files_dir.join("pad.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(&paths).await;
    sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    let report = sync_service::sync_library(&db, &paths, || ()).await.unwrap();

    assert!(report.sidecars_updated.is_empty());
    assert!(report.files_added.is_empty());
}
