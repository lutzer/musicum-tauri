mod common;

use musicum_core::{db, sidecar, services::sync_service};
use musicum_core::db::entities::{clip, file};
use sea_orm::{EntityTrait, PaginatorTrait};
use tempfile::tempdir;

async fn setup(lib_path: &std::path::Path) -> sea_orm::DatabaseConnection {
    db::connect(lib_path.to_str().unwrap()).await.unwrap()
}

#[tokio::test]
async fn sync_discovers_wav_file() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("kick.wav");
    common::write_sine_wav(&wav, 0.5);

    let db = setup(dir.path()).await;
    let stats = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(stats.added, 1, "should have found one new file");
    assert_eq!(stats.removed, 0);

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
    let wav = dir.path().join("pad.wav");
    common::write_stereo_wav(&wav, 1.0);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let sidecar_path = dir.path().join("pad.wav.musicum.json");
    assert!(sidecar_path.exists(), "sidecar should be created next to audio file");

    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.version, 1);
    assert!(sc.clips.is_empty());
}

#[tokio::test]
async fn sync_reads_existing_sidecar_with_clips() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("synth.wav");
    common::write_sine_wav(&wav, 2.0);

    // Pre-write a sidecar with one clip
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let sc = sidecar::FileSidecar {
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
    std::fs::write(
        &sidecar_path,
        serde_json::to_string_pretty(&sc).unwrap(),
    )
    .unwrap();

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

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
    let wav = dir.path().join("loop.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(dir.path()).await;

    // First sync
    let s1 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s1.added, 1);

    // Second sync — file unchanged
    let s2 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s2.added, 0, "no new files on second sync");
    assert_eq!(s2.updated, 0);
    assert_eq!(s2.removed, 0);

    // DB should still have exactly one file
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);
}

#[tokio::test]
async fn sync_detects_removed_files() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("temp.wav");
    common::write_sine_wav(&wav, 0.3);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    // Delete the file from disk
    std::fs::remove_file(&wav).unwrap();

    let s2 = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(s2.removed, 1);
    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn sync_walks_subdirectories() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("drums")).unwrap();
    common::write_sine_wav(&dir.path().join("drums").join("kick.wav"), 0.1);
    common::write_sine_wav(&dir.path().join("drums").join("snare.wav"), 0.1);
    common::write_sine_wav(&dir.path().join("pad.wav"), 1.0);

    let db = setup(dir.path()).await;
    let stats = sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    assert_eq!(stats.added, 3, "should find files in subdirectories too");
}

#[tokio::test]
async fn sync_preset_sidecar() {
    let dir = tempdir().unwrap();

    // Write a preset sidecar
    let preset_sc = sidecar::PresetSidecar {
        version: 1,
        slug: "lo-fi".into(),
        title: "Lo-Fi".into(),
        description: "lo-fi preset".into(),
        processors: vec![sidecar::ProcessorEntry::AudioPlugin {
            id: "gain".into(),
            enabled: true,
            processor: sidecar::ProcessorRef {
                id: "gain".into(),
                params: serde_json::json!({ "gain": 0.5 }),
            },
        }],
    };
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let presets = musicum_core::db::entities::preset::Entity::find()
        .all(&db)
        .await
        .unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].slug, "lo-fi");
}

#[tokio::test]
async fn sync_picks_up_updated_preset_sidecar() {
    let dir = tempdir().unwrap();

    let mut preset_sc = sidecar::PresetSidecar {
        version: 1,
        slug: "reverb-hall".into(),
        title: "Hall Reverb".into(),
        description: "".into(),
        processors: vec![],
    };
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    // Update the sidecar on disk — add a processor
    preset_sc.processors.push(sidecar::ProcessorEntry::AudioPlugin {
        id: "rev-1".into(),
        enabled: true,
        processor: sidecar::ProcessorRef {
            id: "reverb".into(),
            params: serde_json::json!({ "mix": 0.4 }),
        },
    });
    sidecar::write_preset_sidecar(dir.path(), &preset_sc).unwrap();

    // Second sync should pick up the change
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let presets = musicum_core::db::entities::preset::Entity::find()
        .all(&db)
        .await
        .unwrap();
    assert_eq!(presets.len(), 1);
    let stored: Vec<sidecar::ProcessorEntry> =
        serde_json::from_str(&presets[0].processors).unwrap();
    assert_eq!(stored.len(), 1, "processor added in sidecar should appear in DB after sync");
}

#[tokio::test]
async fn sync_picks_up_sidecar_metadata_when_audio_unchanged() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("bass.wav");
    common::write_sine_wav(&wav, 1.0);

    let db = setup(dir.path()).await;
    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    // Update the sidecar with new metadata (audio file untouched)
    let sidecar_path = sidecar::sidecar_path_for_audio(&wav);
    let mut sc = sidecar::read_file_sidecar(&wav).unwrap();
    sc.metadata.bpm = Some(140.0);
    sc.metadata.key = Some("Am".into());
    std::fs::write(&sidecar_path, serde_json::to_string_pretty(&sc).unwrap()).unwrap();

    sync_service::sync_library(&db, dir.path().to_str().unwrap())
        .await
        .unwrap();

    let files = musicum_core::db::entities::file::Entity::find()
        .all(&db)
        .await
        .unwrap();
    let meta = musicum_core::db::entities::file_metadata::Entity::find_by_id(&files[0].id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.bpm, Some(140.0), "BPM should be updated from sidecar even when audio is unchanged");
    assert_eq!(meta.key.as_deref(), Some("Am"));
}
