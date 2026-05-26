use std::path::Path;

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use slug::slugify;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::db::entities::{clip, collection, collection_clip, file, file_attachment, file_metadata, preset};
use crate::sidecar::{self, ClipSidecar, FileSidecar};
use crate::ServiceError;

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif"];

pub fn count_audio_files(library_dir: &str) -> Result<usize, ServiceError> {
    let lib_path = Path::new(library_dir);
    let count = WalkDir::new(lib_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            if p.components().any(|c| c.as_os_str() == ".musicum") { return false; }
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            AUDIO_EXTENSIONS.contains(&ext.as_str())
        })
        .count();
    Ok(count)
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub files_added:      Vec<String>,
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,
    pub presets_added:    Vec<String>,
    pub presets_updated:  Vec<String>,
}

pub async fn sync_library(
    db: &DatabaseConnection,
    library_dir: &str,
    on_progress: impl Fn(),
) -> Result<SyncReport, ServiceError> {
    let lib_path = Path::new(library_dir);
    let mut report = SyncReport::default();

    // 1. Collect all current file paths in the DB for removal detection
    let existing_files = file::Entity::find().all(db).await?;
    let mut existing_paths: std::collections::HashSet<String> =
        existing_files.iter().map(|f| f.path.clone()).collect();

    // 2. Walk the library directory for audio files
    for entry in WalkDir::new(lib_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        // Skip .musicum hidden directory
        if path.components().any(|c| c.as_os_str() == ".musicum") {
            continue;
        }

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();
        existing_paths.remove(&path_str);

        let hash = file_hash(path)?;
        let sc = sidecar::read_file_sidecar(path)?;
        let audio_info = probe_audio(path)?;

        upsert_file(db, path, &path_str, &hash, &audio_info, &sc, &mut report).await?;
        on_progress();
    }

    // 3. Mark removed files (paths no longer on disk)
    for removed_path in &existing_paths {
        if let Some(f) = file::Entity::find()
            .filter(file::Column::Path.eq(removed_path.as_str()))
            .one(db)
            .await?
        {
            let display = Path::new(removed_path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            delete_file_cascade(db, &f.id).await?;
            report.files_removed.push(display);
        }
    }

    // 4. Sync collections and presets from their sidecar directories
    sync_collections(db, lib_path).await?;
    sync_presets(db, lib_path, &mut report).await?;

    Ok(report)
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
    let channels = params
        .channels
        .map(|c| c.count() as u32)
        .unwrap_or(1);

    let duration = if let Some(frames) = params.n_frames {
        frames as f64 / sample_rate as f64
    } else if let Some(tb) = params.time_base {
        let ts = track.codec_params.start_ts;
        ts as f64 * tb.numer as f64 / tb.denom as f64
    } else {
        0.0
    };

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav")
        .to_lowercase();
    let mime_type = match ext.as_str() {
        "mp3"  => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg"  => "audio/ogg",
        "aiff" | "aif" => "audio/aiff",
        _      => "audio/wav",
    };

    Ok(AudioInfo {
        duration,
        sample_rate,
        channels,
        mime_type: mime_type.to_string(),
    })
}

fn file_hash(path: &Path) -> Result<String, ServiceError> {
    let bytes = std::fs::read(path)?;
    let hash = Sha256::digest(&bytes);
    Ok(hex::encode(hash))
}

// ── Upsert helpers ────────────────────────────────────────────────────────

async fn upsert_file(
    db: &DatabaseConnection,
    path: &Path,
    path_str: &str,
    hash: &str,
    audio: &AudioInfo,
    sc: &FileSidecar,
    report: &mut SyncReport,
) -> Result<(), ServiceError> {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let slug = slugify(&name);
    let now = chrono::Utc::now().to_rfc3339();

    // Find existing file by path
    let existing = file::Entity::find()
        .filter(file::Column::Path.eq(path_str))
        .one(db)
        .await?;

    let file_id = if let Some(existing_model) = existing {
        if existing_model.hash == hash {
            let meta_changed = upsert_file_metadata(db, &existing_model.id, &sc.metadata).await?;
            let clips_changed = upsert_clips(db, &existing_model.id, &sc.clips).await?;
            if meta_changed || clips_changed {
                report.sidecars_updated.push(name);
            }
            return Ok(());
        }
        // File changed (hash differs) — update
        file::ActiveModel {
            id: Set(existing_model.id.clone()),
            slug: Set(slug),
            name: Set(name.clone()),
            path: Set(path_str.to_string()),
            duration: Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels: Set(audio.channels as i32),
            mime_type: Set(audio.mime_type.clone()),
            hash: Set(hash.to_string()),
            created_at: Set(existing_model.created_at.clone()),
            updated_at: Set(now),
        }
        .update(db)
        .await?;

        upsert_file_metadata(db, &existing_model.id, &sc.metadata).await?;
        report.files_updated.push(name.clone());
        existing_model.id
    } else {
        // New file
        let id = Uuid::new_v4().to_string();

        file::ActiveModel {
            id: Set(id.clone()),
            slug: Set(slug),
            name: Set(name.clone()),
            path: Set(path_str.to_string()),
            duration: Set(audio.duration),
            sample_rate: Set(audio.sample_rate as i32),
            channels: Set(audio.channels as i32),
            mime_type: Set(audio.mime_type.clone()),
            hash: Set(hash.to_string()),
            created_at: Set(now.clone()),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;

        upsert_file_metadata(db, &id, &sc.metadata).await?;
        report.files_added.push(name.clone());

        // Write back default sidecar if it didn't exist
        let _ = sidecar::write_file_sidecar(path, sc);

        id
    };

    upsert_clips(db, &file_id, &sc.clips).await?;
    Ok(())
}

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
                    id: Set(ex.id.clone()),
                    slug: Set(cs.slug.clone()),
                    file_id: Set(file_id.to_string()),
                    title: Set(cs.title.clone()),
                    processors: Set(processors_json),
                    cached: Set(ex.cached.clone()),
                    cached_path: Set(ex.cached_path.clone()),
                    duration: Set(ex.duration),
                    notes: Set(cs.notes.clone()),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
                .update(db)
                .await?;
                any_changed = true;
            }
        } else {
            clip::ActiveModel {
                id: Set(Uuid::new_v4().to_string()),
                slug: Set(cs.slug.clone()),
                file_id: Set(file_id.to_string()),
                title: Set(cs.title.clone()),
                processors: Set(processors_json),
                cached: Set("no_cache".into()),
                cached_path: Set(None),
                duration: Set(None),
                notes: Set(cs.notes.clone()),
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

pub(crate) async fn delete_file_cascade(db: &DatabaseConnection, file_id: &str) -> Result<(), ServiceError> {
    // Collect clip IDs first so we can remove collection_clip references
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

async fn sync_collections(
    db: &DatabaseConnection,
    library_dir: &Path,
) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_collection_sidecars(library_dir)?;
    for sc in sidecars {
        let existing = collection::Entity::find()
            .filter(collection::Column::Slug.eq(&sc.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        let col_id = if let Some(ex) = existing {
            collection::ActiveModel {
                id: Set(ex.id.clone()),
                slug: Set(sc.slug.clone()),
                title: Set(sc.title.clone()),
                description: Set(sc.description.clone()),
                background_path: Set(None),
                created_at: Set(ex.created_at.clone()),
                updated_at: Set(now),
            }
            .update(db)
            .await?;
            ex.id
        } else {
            let id = Uuid::new_v4().to_string();
            collection::ActiveModel {
                id: Set(id.clone()),
                slug: Set(sc.slug.clone()),
                title: Set(sc.title.clone()),
                description: Set(sc.description.clone()),
                background_path: Set(None),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
            id
        };

        // Re-sync clip membership (position = index in sidecar clips array)
        collection_clip::Entity::delete_many()
            .filter(collection_clip::Column::CollectionId.eq(&col_id))
            .exec(db)
            .await?;

        for (pos, clip_slug) in sc.clips.iter().enumerate() {
            if let Some(c) = clip::Entity::find()
                .filter(clip::Column::Slug.eq(clip_slug.as_str()))
                .one(db)
                .await?
            {
                let _ = collection_clip::ActiveModel {
                    id: Set(Uuid::new_v4().to_string()),
                    collection_id: Set(col_id.clone()),
                    clip_id: Set(c.id.clone()),
                    position: Set(pos as i32),
                }
                .insert(db)
                .await;
            }
        }
    }
    Ok(())
}

async fn sync_presets(
    db: &DatabaseConnection,
    library_dir: &Path,
    report: &mut SyncReport,
) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_preset_sidecars(library_dir)?;
    for sc in sidecars {
        let processors_json = serde_json::to_string(&sc.processors)?;
        let existing = preset::Entity::find()
            .filter(preset::Column::Slug.eq(&sc.slug))
            .one(db)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();

        if let Some(ex) = existing {
            let differs = ex.title != sc.title
                || ex.description != sc.description
                || ex.processors != processors_json;
            if differs {
                preset::ActiveModel {
                    id: Set(ex.id.clone()),
                    slug: Set(sc.slug.clone()),
                    title: Set(sc.title.clone()),
                    description: Set(sc.description.clone()),
                    processors: Set(processors_json),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
                .update(db)
                .await?;
                report.presets_updated.push(sc.title.clone());
            }
        } else {
            preset::ActiveModel {
                id: Set(Uuid::new_v4().to_string()),
                slug: Set(sc.slug.clone()),
                title: Set(sc.title.clone()),
                description: Set(sc.description.clone()),
                processors: Set(processors_json),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
            report.presets_added.push(sc.title.clone());
        }
    }
    Ok(())
}

