use musicum_core::db;
use musicum_core::db::entities::{clip, collection, collection_clip, file, file_metadata, preset};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter,
};
use tempfile::tempdir;
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

async fn open_db() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = db::connect(dir.path()).await.unwrap();
    (db, dir)
}

#[tokio::test]
async fn connect_creates_db_file() {
    let (_, dir) = open_db().await;
    let db_path = dir.path().join("musicum.db");
    assert!(db_path.exists(), "musicum.db was not created");
}

#[tokio::test]
async fn schema_version_is_stored() {
    let (db, _dir) = open_db().await;
    let row = db
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT value FROM _musicum_meta WHERE key = 'schema_version'".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    let version: String = row.try_get("", "value").unwrap();
    assert_eq!(version, musicum_core::db::schema::SCHEMA_VERSION.to_string());
}

#[tokio::test]
async fn schema_reset_on_version_bump() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    let db = db::connect(path).await.unwrap();

    file::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set("test-file".into()),
        name: Set("test".into()),
        path: Set("/tmp/test.wav".into()),
        duration: Set(1.0),
        sample_rate: Set(44100),
        channels: Set(2),
        mime_type: Set("audio/wav".into()),
        hash: Set("abc123".into()),
        mtime: Set(String::new()),
        size_bytes: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    db.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "UPDATE _musicum_meta SET value = '999' WHERE key = 'schema_version'".to_owned(),
    ))
    .await
    .unwrap();
    drop(db);

    let db2 = db::connect(&path).await.unwrap();
    assert_eq!(
        file::Entity::find().count(&db2).await.unwrap(),
        0,
        "tables should be empty after schema reset"
    );
}

#[tokio::test]
async fn insert_and_query_file_with_metadata() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("drums-kick".into()),
        name: Set("Kick Drum".into()),
        path: Set("/music/kick.wav".into()),
        duration: Set(0.5),
        sample_rate: Set(44100),
        channels: Set(1),
        mime_type: Set("audio/wav".into()),
        hash: Set("deadbeef".into()),
        mtime: Set(String::new()),
        size_bytes: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    file_metadata::ActiveModel {
        file_id: Set(file_id.clone()),
        bpm: Set(Some(128.0)),
        key: Set(Some("Am".into())),
        rating: Set(Some(4)),
        color: Set(None),
        notes: Set("punchy kick".into()),
        tags: Set("drums,percussion".into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let f = file::Entity::find_by_id(&file_id)
        .one(&db)
        .await
        .unwrap()
        .expect("file should exist");
    assert_eq!(f.slug, "drums-kick");
    assert_eq!(f.sample_rate, 44100);

    let m = file_metadata::Entity::find_by_id(&file_id)
        .one(&db)
        .await
        .unwrap()
        .expect("metadata should exist");
    assert_eq!(m.bpm, Some(128.0));
    assert_eq!(m.tags, "drums,percussion");
}

#[tokio::test]
async fn insert_clip_and_query_by_file() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();
    let clip_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("synth-pad".into()),
        name: Set("Pad".into()),
        path: Set("/music/pad.wav".into()),
        duration: Set(4.0),
        sample_rate: Set(48000),
        channels: Set(2),
        mime_type: Set("audio/wav".into()),
        hash: Set("cafebabe".into()),
        mtime: Set(String::new()),
        size_bytes: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    clip::ActiveModel {
        id: Set(clip_id.clone()),
        slug: Set("pad-reverb".into()),
        file_id: Set(file_id.clone()),
        title: Set("With Reverb".into()),
        processors: Set(r#"[{"type":"plugin","id":"reverb","enabled":true,"params":{}}]"#.into()),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    let clips = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].slug, "pad-reverb");
}

#[tokio::test]
async fn collection_clip_unique_constraint() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();
    let clip_id = Uuid::new_v4().to_string();
    let col_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("my-file".into()),
        name: Set("File".into()),
        path: Set("/tmp/f.wav".into()),
        duration: Set(1.0),
        sample_rate: Set(44100),
        channels: Set(1),
        mime_type: Set("audio/wav".into()),
        hash: Set("11".into()),
        mtime: Set(String::new()),
        size_bytes: Set(0),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    clip::ActiveModel {
        id: Set(clip_id.clone()),
        slug: Set("my-clip".into()),
        file_id: Set(file_id.clone()),
        title: Set("Clip".into()),
        processors: Set("[]".into()),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    collection::ActiveModel {
        id: Set(col_id.clone()),
        slug: Set("my-col".into()),
        title: Set("Col".into()),
        description: Set(String::new()),
        background_path: Set(None),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    collection_clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        collection_id: Set(col_id.clone()),
        clip_id: Set(clip_id.clone()),
        position: Set(0),
    }
    .insert(&db)
    .await
    .unwrap();

    let result = collection_clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        collection_id: Set(col_id.clone()),
        clip_id: Set(clip_id.clone()),
        position: Set(1),
    }
    .insert(&db)
    .await;

    assert!(result.is_err(), "duplicate collection_clip should be rejected");
}

#[tokio::test]
async fn insert_and_query_preset() {
    let (db, _dir) = open_db().await;
    let preset_id = Uuid::new_v4().to_string();

    preset::ActiveModel {
        id: Set(preset_id.clone()),
        slug: Set("warm-reverb".into()),
        title: Set("Warm Reverb".into()),
        description: Set("A warm reverb preset".into()),
        processors: Set(r#"[{"type":"plugin","id":"reverb","enabled":true,"params":{}}]"#.into()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    let p = preset::Entity::find_by_id(&preset_id)
        .one(&db)
        .await
        .unwrap()
        .expect("preset should exist");
    assert_eq!(p.slug, "warm-reverb");
}
