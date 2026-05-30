use std::collections::{HashMap, HashSet};
use std::path::Path;

use sea_orm::{DatabaseConnection, EntityTrait};
use walkdir::WalkDir;

use crate::db::entities::file;
use crate::services::file_service::{self, AudioInfo};
use crate::sidecar;
use crate::ServiceError;

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif"];

#[derive(Debug, Default)]
pub struct SyncReport {
    pub files_added:      Vec<String>,
    pub files_updated:    Vec<String>,
    pub files_removed:    Vec<String>,
    pub sidecars_updated: Vec<String>,
}

pub async fn sync_library(
    db: &DatabaseConnection,
    paths: &crate::config::LibraryPaths,
    on_progress: impl Fn(),
) -> Result<SyncReport, ServiceError> {
    let lib_path = &paths.files_dir;
    let mut report = SyncReport::default();

    // Pre-load all DB records to avoid per-file queries.
    let all_files = file::Entity::find().all(db).await?;
    // path → model (fast-skip and hash checks)
    let path_map: HashMap<String, file::Model> = all_files.iter()
        .map(|f| (f.path.clone(), f.clone()))
        .collect();
    // uuid → model (rename detection)
    let uuid_map: HashMap<String, file::Model> = all_files.iter()
        .map(|f| (f.id.clone(), f.clone()))
        .collect();
    // paths still in the DB — remove as we encounter them; remainder = deleted
    let mut remaining: HashSet<String> = all_files.into_iter().map(|f| f.path).collect();

    for entry in WalkDir::new(lib_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "catalog" { continue; }
        }

        if !path.is_file() { continue; }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !AUDIO_EXTENSIONS.contains(&ext.as_str()) { continue; }

        let path_str = path.to_string_lossy().to_string();
        remaining.remove(&path_str);

        let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let (mtime, size_bytes) = file_service::file_mtime(path)?;
        let mut sc = sidecar::read_file_sidecar(path)?;

        // ── Rename detection ─────────────────────────────────────────────
        // Only possible if the sidecar already carries a UUID.
        if !sc.id.is_empty() {
            if let Some(existing) = uuid_map.get(&sc.id) {
                if existing.path != path_str {
                    // File was moved/renamed. Reuse existing audio info — no re-probe needed.
                    remaining.remove(&existing.path);
                    let audio = AudioInfo {
                        duration:    existing.duration,
                        sample_rate: existing.sample_rate as u32,
                        channels:    existing.channels as u32,
                        mime_type:   existing.mime_type.clone(),
                    };
                    file_service::upsert_file(
                        db, path, &mut sc,
                        &existing.hash, &mtime, size_bytes,
                        &audio, Some(existing),
                    ).await?;
                    report.files_updated.push(name);
                    on_progress();
                    continue;
                }
            }
        }

        // ── Fast-skip ────────────────────────────────────────────────────
        let existing = path_map.get(&path_str);
        if let Some(ex) = existing {
            if ex.mtime == mtime && ex.size_bytes == size_bytes {
                on_progress();
                continue;
            }
        }

        // ── Hash check ───────────────────────────────────────────────────
        let hash = file_service::file_hash(path)?;

        if let Some(ex) = existing {
            if ex.hash == hash {
                // Content unchanged (e.g. file was touched). Sync sidecar metadata only.
                let audio = AudioInfo {
                    duration:    ex.duration,
                    sample_rate: ex.sample_rate as u32,
                    channels:    ex.channels as u32,
                    mime_type:   ex.mime_type.clone(),
                };
                let changed = file_service::upsert_file(
                    db, path, &mut sc,
                    &hash, &mtime, size_bytes,
                    &audio, Some(ex),
                ).await?;
                if changed {
                    report.sidecars_updated.push(name);
                }
                on_progress();
                continue;
            }

            // Content changed — re-probe audio.
            let audio = file_service::probe_audio(path)?;
            file_service::upsert_file(
                db, path, &mut sc,
                &hash, &mtime, size_bytes,
                &audio, Some(ex),
            ).await?;
            report.files_updated.push(name);
        } else {
            // New file.
            let audio = file_service::probe_audio(path)?;
            file_service::upsert_file(
                db, path, &mut sc,
                &hash, &mtime, size_bytes,
                &audio, None,
            ).await?;
            report.files_added.push(name);
        }

        on_progress();
    }

    // ── Removal ──────────────────────────────────────────────────────────
    for removed_path in &remaining {
        if let Some(f) = path_map.get(removed_path) {
            let display = Path::new(removed_path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            file_service::delete_file_cascade(db, &f.id).await?;
            report.files_removed.push(display);
        }
    }

    Ok(report)
}
