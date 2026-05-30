use std::path::Path;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection,
              EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use sha2::{Digest, Sha256};
use slug::slugify;
use uuid::Uuid;

use crate::db::entities::{clip, collection_clip, file, file_attachment, file_metadata};
use crate::sidecar::{self, ClipSidecar};
use crate::ServiceError;

pub async fn list_files(db: &DatabaseConnection) -> Result<Vec<file::Model>, ServiceError> {
    Ok(file::Entity::find()
        .order_by_asc(file::Column::Name)
        .all(db)
        .await?)
}

pub async fn get_file_by_id(
    db: &DatabaseConnection,
    id: &str,
) -> Result<file::Model, ServiceError> {
    file::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("file '{id}'")))
}

pub async fn get_file_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<file::Model, ServiceError> {
    file::Entity::find()
        .filter(file::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("file '{slug}'")))
}

pub async fn get_file_metadata(
    db: &DatabaseConnection,
    file_id: &str,
) -> Result<Option<file_metadata::Model>, ServiceError> {
    Ok(file_metadata::Entity::find_by_id(file_id).one(db).await?)
}

pub async fn set_file_notes(
    db: &DatabaseConnection,
    file_slug: &str,
    notes: &str,
) -> Result<(), ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    sc.metadata.notes = notes.to_string();
    sidecar::write_file_sidecar(audio_path, &sc)?;
    upsert_file_metadata_notes_tags(db, &file.id, Some(notes), None).await
}

pub async fn set_file_tags(
    db: &DatabaseConnection,
    file_slug: &str,
    tags: &str,
) -> Result<(), ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;
    sc.metadata.tags = tags.to_string();
    sidecar::write_file_sidecar(audio_path, &sc)?;
    upsert_file_metadata_notes_tags(db, &file.id, None, Some(tags)).await
}

// Sets notes and/or tags on file_metadata, creating the row if absent.
// Pass None for a field to leave it unchanged (or use empty string for initial insert).
async fn upsert_file_metadata_notes_tags(
    db: &DatabaseConnection,
    file_id: &str,
    notes: Option<&str>,
    tags: Option<&str>,
) -> Result<(), ServiceError> {
    let existing = file_metadata::Entity::find_by_id(file_id).one(db).await?;
    if let Some(ex) = existing {
        file_metadata::ActiveModel {
            file_id: Set(ex.file_id),
            bpm:     Set(ex.bpm),
            key:     Set(ex.key),
            rating:  Set(ex.rating),
            color:   Set(ex.color),
            notes:   Set(notes.map(str::to_string).unwrap_or(ex.notes)),
            tags:    Set(tags.map(str::to_string).unwrap_or(ex.tags)),
        }
        .update(db)
        .await?;
    } else {
        file_metadata::ActiveModel {
            file_id: Set(file_id.to_string()),
            bpm:     Set(None),
            key:     Set(None),
            rating:  Set(None),
            color:   Set(None),
            notes:   Set(notes.unwrap_or("").to_string()),
            tags:    Set(tags.unwrap_or("").to_string()),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

pub async fn delete_file(
    db: &DatabaseConnection,
    file_slug: &str,
    delete_audio: bool,
) -> Result<usize, ServiceError> {
    let file = get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);

    let clip_count = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file.id))
        .count(db)
        .await? as usize;

    let sidecar_path = sidecar::sidecar_path_for_audio(audio_path);
    if sidecar_path.exists() {
        std::fs::remove_file(&sidecar_path)?;
    }

    delete_file_cascade(db, &file.id).await?;

    if delete_audio && audio_path.exists() {
        std::fs::remove_file(audio_path)?;
    }

    Ok(clip_count)
}

// ── Audio probing ─────────────────────────────────────────────────────────

pub struct AudioInfo {
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub mime_type: String,
}

pub fn probe_audio(path: &Path) -> Result<AudioInfo, ServiceError> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let src = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| ServiceError::InvalidInput(format!("audio probe failed: {e}")))?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| ServiceError::InvalidInput("no audio track found".into()))?;

    let params = &track.codec_params;
    let sample_rate = params.sample_rate.unwrap_or(44100);
    let channels = params.channels.map(|c| c.count() as u32).unwrap_or(1);
    let duration = if let Some(frames) = params.n_frames {
        frames as f64 / sample_rate as f64
    } else if let Some(tb) = params.time_base {
        let ts = track.codec_params.start_ts;
        ts as f64 * tb.numer as f64 / tb.denom as f64
    } else {
        0.0
    };

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("wav").to_lowercase();
    let mime_type = match ext.as_str() {
        "mp3"  => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg"  => "audio/ogg",
        "aiff" | "aif" => "audio/aiff",
        _      => "audio/wav",
    };

    Ok(AudioInfo { duration, sample_rate, channels, mime_type: mime_type.to_string() })
}

pub(crate) fn file_hash(path: &Path) -> Result<String, ServiceError> {
    use std::io::{BufReader, Read};
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn file_mtime(path: &Path) -> Result<(String, i64), ServiceError> {
    let meta = std::fs::metadata(path)?;
    let modified = meta.modified()?;
    let size = meta.len() as i64;
    let dt: chrono::DateTime<chrono::Utc> = modified.into();
    Ok((dt.to_rfc3339(), size))
}

// ── Upsert helpers ────────────────────────────────────────────────────────

async fn upsert_file_metadata(
    db: &DatabaseConnection,
    file_id: &str,
    meta: &crate::sidecar::FileMetadataSidecar,
) -> Result<bool, ServiceError> {
    let existing = file_metadata::Entity::find_by_id(file_id).one(db).await?;
    let model = file_metadata::ActiveModel {
        file_id: Set(file_id.to_string()),
        bpm: Set(meta.bpm),
        key: Set(meta.key.clone()),
        rating: Set(meta.rating),
        color: Set(meta.color.clone()),
        notes: Set(meta.notes.clone()),
        tags: Set(meta.tags.clone()),
    };
    let changed = if let Some(ex) = existing {
        let differs = ex.bpm != meta.bpm
            || ex.key != meta.key
            || ex.rating != meta.rating
            || ex.color != meta.color
            || ex.notes != meta.notes
            || ex.tags != meta.tags;
        if differs {
            model.update(db).await?;
        }
        differs
    } else {
        model.insert(db).await?;
        true
    };
    Ok(changed)
}

async fn upsert_clips(
    db: &DatabaseConnection,
    file_id: &str,
    clip_sidecars: &[ClipSidecar],
) -> Result<bool, ServiceError> {
    let mut any_changed = false;
    for cs in clip_sidecars {
        let processors_json = serde_json::to_string(&cs.processors)?;
        let existing = clip::Entity::find()
            .filter(clip::Column::Slug.eq(&cs.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(ex) = existing {
            let differs = ex.title != cs.title
                || ex.processors != processors_json
                || ex.notes != cs.notes;
            if differs {
                clip::ActiveModel {
                    id:         Set(ex.id.clone()),
                    slug:       Set(cs.slug.clone()),
                    file_id:    Set(file_id.to_string()),
                    title:      Set(cs.title.clone()),
                    processors: Set(processors_json),
                    duration:   Set(ex.duration),
                    notes:      Set(cs.notes.clone()),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
                .update(db)
                .await?;
                any_changed = true;
            }
        } else {
            clip::ActiveModel {
                id:         Set(Uuid::new_v4().to_string()),
                slug:       Set(cs.slug.clone()),
                file_id:    Set(file_id.to_string()),
                title:      Set(cs.title.clone()),
                processors: Set(processors_json),
                duration:   Set(None),
                notes:      Set(cs.notes.clone()),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
            any_changed = true;
        }
    }
    Ok(any_changed)
}

pub(crate) async fn delete_file_cascade(
    db: &DatabaseConnection,
    file_id: &str,
) -> Result<(), ServiceError> {
    let clips = clip::Entity::find()
        .filter(clip::Column::FileId.eq(file_id))
        .all(db)
        .await?;

    for c in &clips {
        collection_clip::Entity::delete_many()
            .filter(collection_clip::Column::ClipId.eq(&c.id))
            .exec(db)
            .await?;
    }

    clip::Entity::delete_many()
        .filter(clip::Column::FileId.eq(file_id))
        .exec(db)
        .await?;

    file_attachment::Entity::delete_many()
        .filter(file_attachment::Column::FileId.eq(file_id))
        .exec(db)
        .await?;

    file_metadata::Entity::delete_many()
        .filter(file_metadata::Column::FileId.eq(file_id))
        .exec(db)
        .await?;

    file::Entity::delete_many()
        .filter(file::Column::Id.eq(file_id))
        .exec(db)
        .await?;

    Ok(())
}

/// Ensures the sidecar has a UUID (assigns + writes if empty), then persists
/// the caller-supplied data to the DB. Returns true if metadata or clips changed.
#[allow(clippy::too_many_arguments)]
pub async fn upsert_file(
    db: &DatabaseConnection,
    path: &Path,
    sc: &mut sidecar::FileSidecar,
    hash: &str,
    mtime: &str,
    size_bytes: i64,
    audio: &AudioInfo,
    existing: Option<&file::Model>,
) -> Result<bool, ServiceError> {
    if sc.id.is_empty() {
        sc.id = Uuid::new_v4().to_string();
        sidecar::write_file_sidecar(path, sc)?;
    }

    let path_str = path.to_string_lossy().to_string();
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    if let Some(ex) = existing {
        file::ActiveModel {
            id:          Set(ex.id.clone()),
            slug:        Set(slugify(&name)),
            name:        Set(name),
            path:        Set(path_str),
            duration:    Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels:    Set(audio.channels as i32),
            mime_type:   Set(audio.mime_type.clone()),
            hash:        Set(hash.to_string()),
            mtime:       Set(mtime.to_string()),
            size_bytes:  Set(size_bytes),
            created_at:  Set(ex.created_at.clone()),
            updated_at:  Set(now),
        }
        .update(db)
        .await?;
        let meta_changed = upsert_file_metadata(db, &ex.id, &sc.metadata).await?;
        let clips_changed = upsert_clips(db, &ex.id, &sc.clips).await?;
        Ok(meta_changed || clips_changed)
    } else {
        file::ActiveModel {
            id:          Set(sc.id.clone()),
            slug:        Set(slugify(&name)),
            name:        Set(name),
            path:        Set(path_str),
            duration:    Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels:    Set(audio.channels as i32),
            mime_type:   Set(audio.mime_type.clone()),
            hash:        Set(hash.to_string()),
            mtime:       Set(mtime.to_string()),
            size_bytes:  Set(size_bytes),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await?;
        upsert_file_metadata(db, &sc.id, &sc.metadata).await?;
        upsert_clips(db, &sc.id, &sc.clips).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_db;
    use sea_orm::ActiveModelTrait;
    use tempfile::tempdir;

    async fn insert_test_file(db: &DatabaseConnection, path: &str) -> file::Model {
        let now = chrono::Utc::now().to_rfc3339();
        file::ActiveModel {
            id:          Set(uuid::Uuid::new_v4().to_string()),
            slug:        Set("test-file".to_string()),
            name:        Set("test-file".to_string()),
            path:        Set(path.to_string()),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set("abc".to_string()),
            mtime:       Set(String::new()),
            size_bytes:  Set(0),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn set_file_notes_creates_row_if_missing() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        let file = insert_test_file(&db, audio.to_str().unwrap()).await;

        set_file_notes(&db, "test-file", "my notes").await.unwrap();

        let meta = file_metadata::Entity::find_by_id(&file.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(meta.notes, "my notes");

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.metadata.notes, "my notes");
    }

    #[tokio::test]
    async fn set_file_notes_preserves_other_fields() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        let file = insert_test_file(&db, audio.to_str().unwrap()).await;

        file_metadata::ActiveModel {
            file_id: Set(file.id.clone()),
            bpm:     Set(Some(120.0)),
            key:     Set(None),
            rating:  Set(None),
            color:   Set(None),
            notes:   Set("old".to_string()),
            tags:    Set("kick".to_string()),
        }
        .insert(&db)
        .await
        .unwrap();

        set_file_notes(&db, "test-file", "new notes").await.unwrap();

        let meta = file_metadata::Entity::find_by_id(&file.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(meta.notes, "new notes");
        assert_eq!(meta.bpm, Some(120.0));   // untouched
        assert_eq!(meta.tags, "kick");        // untouched
    }

    #[tokio::test]
    async fn set_file_tags_creates_row_if_missing() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        set_file_tags(&db, "test-file", "kick, loop").await.unwrap();

        let sc = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.metadata.tags, "kick, loop");
    }

    #[tokio::test]
    async fn delete_file_removes_db_and_sidecar() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        let sc = sidecar::FileSidecar::default_for_file();
        sidecar::write_file_sidecar(&audio, &sc).unwrap();

        delete_file(&db, "test-file", false).await.unwrap();

        assert!(get_file_by_slug(&db, "test-file").await.is_err());
        assert!(!sidecar::sidecar_path_for_audio(&audio).exists());
        assert!(audio.exists());
    }

    #[tokio::test]
    async fn delete_file_with_audio_flag_removes_audio() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();
        insert_test_file(&db, audio.to_str().unwrap()).await;

        delete_file(&db, "test-file", true).await.unwrap();

        assert!(!audio.exists());
    }

    // ── upsert_file tests ────────────────────────────────────────────────────

    async fn insert_file_row(
        db: &DatabaseConnection,
        id: &str,
        path: &str,
    ) -> file::Model {
        let now = chrono::Utc::now().to_rfc3339();
        file::ActiveModel {
            id:          Set(id.to_string()),
            slug:        Set(id.to_string()),
            name:        Set(id.to_string()),
            path:        Set(path.to_string()),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set("oldhash".to_string()),
            mtime:       Set("1970-01-01T00:00:00+00:00".to_string()),
            size_bytes:  Set(0),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn dummy_audio() -> AudioInfo {
        AudioInfo { duration: 2.5, sample_rate: 44100, channels: 2, mime_type: "audio/wav".to_string() }
    }

    #[tokio::test]
    async fn upsert_file_writes_uuid_to_sidecar_when_empty() {
        let db = crate::db::test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"data").unwrap();

        let mut sc = sidecar::FileSidecar {
            id: String::new(),
            version: 2,
            metadata: sidecar::FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        };

        upsert_file(&db, &audio, &mut sc, "abc123", "2024-01-01T00:00:00+00:00", 4, &dummy_audio(), None).await.unwrap();

        assert!(!sc.id.is_empty());
        assert!(uuid::Uuid::parse_str(&sc.id).is_ok());

        let saved = sidecar::read_file_sidecar(&audio).unwrap();
        assert_eq!(saved.id, sc.id);
    }

    #[tokio::test]
    async fn upsert_file_inserts_new_row_when_no_existing() {
        let db = crate::db::test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("new.wav");
        std::fs::write(&audio, b"data").unwrap();

        let uuid = "bbbbbbbb-0000-0000-0000-000000000001";
        let mut sc = sidecar::FileSidecar {
            id: uuid.to_string(),
            version: 2,
            metadata: sidecar::FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        };

        upsert_file(&db, &audio, &mut sc, "hash1", "2024-01-01T00:00:00+00:00", 4, &dummy_audio(), None).await.unwrap();

        let row = file::Entity::find_by_id(uuid).one(&db).await.unwrap().unwrap();
        assert_eq!(row.path, audio.to_str().unwrap());
        assert_eq!(row.hash, "hash1");
    }

    #[tokio::test]
    async fn upsert_file_updates_row_when_existing_provided() {
        let db = crate::db::test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("existing.wav");
        std::fs::write(&audio, b"data").unwrap();

        let uuid = "bbbbbbbb-0000-0000-0000-000000000002";
        let existing = insert_file_row(&db, uuid, audio.to_str().unwrap()).await;

        let mut sc = sidecar::FileSidecar {
            id: uuid.to_string(),
            version: 2,
            metadata: sidecar::FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        };

        let new_audio = AudioInfo { duration: 10.0, sample_rate: 48000, channels: 1, mime_type: "audio/wav".to_string() };
        upsert_file(&db, &audio, &mut sc, "newhash", "2025-01-01T00:00:00+00:00", 99, &new_audio, Some(&existing)).await.unwrap();

        let row = file::Entity::find_by_id(uuid).one(&db).await.unwrap().unwrap();
        assert_eq!(row.hash, "newhash");
        assert_eq!(row.duration, 10.0);
        assert_eq!(row.sample_rate, 48000);
        assert_eq!(row.size_bytes, 99);
    }

    #[tokio::test]
    async fn upsert_file_returns_true_when_metadata_changes() {
        let db = crate::db::test_db().await;
        let dir = tempdir().unwrap();
        let audio = dir.path().join("meta.wav");
        std::fs::write(&audio, b"data").unwrap();

        let uuid = "bbbbbbbb-0000-0000-0000-000000000003";
        let existing = insert_file_row(&db, uuid, audio.to_str().unwrap()).await;

        file_metadata::ActiveModel {
            file_id: Set(uuid.to_string()),
            bpm:     Set(None),
            key:     Set(None),
            rating:  Set(None),
            color:   Set(None),
            notes:   Set(String::new()),
            tags:    Set(String::new()),
        }
        .insert(&db)
        .await
        .unwrap();

        let mut sc = sidecar::FileSidecar {
            id: uuid.to_string(),
            version: 2,
            metadata: sidecar::FileMetadataSidecar { bpm: Some(128.0), ..Default::default() },
            attachments: vec![],
            clips: vec![],
        };

        let changed = upsert_file(&db, &audio, &mut sc, "oldhash", "2025-01-01T00:00:00+00:00", 4, &dummy_audio(), Some(&existing)).await.unwrap();
        assert!(changed, "metadata changed → should return true");

        let meta = file_metadata::Entity::find_by_id(uuid).one(&db).await.unwrap().unwrap();
        assert_eq!(meta.bpm, Some(128.0));
    }
}
