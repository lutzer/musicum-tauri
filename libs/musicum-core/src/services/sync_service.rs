use std::collections::{HashMap, HashSet};
use std::path::Path;

use sea_orm::{DatabaseConnection, EntityTrait};
use walkdir::WalkDir;

use crate::db::entities::file;
use crate::services::file_service::{self, AudioInfo};
use crate::sidecar;
use crate::ServiceError;

const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "flac", "ogg", "aiff", "aif"];

struct PendingNew {
    path:       std::path::PathBuf,
    hash:       String,
    mtime:      String,
    size_bytes: i64,
    audio:      AudioInfo,
    name:       String,
}

#[derive(Debug)]
pub struct OrphanedSidecarInfo {
    pub name: String,
    pub sidecar_path: std::path::PathBuf,
    pub db_id: String,
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub files_added:       Vec<String>,
    pub files_updated:     Vec<String>,
    pub files_removed:     Vec<String>,
    pub sidecars_updated:  Vec<String>,
    pub files_repaired:    Vec<String>,
    pub orphaned_sidecars: Vec<OrphanedSidecarInfo>,
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
    // New audio files with no sidecar — deferred for the repair pass.
    let mut pending_new: Vec<PendingNew> = Vec::new();

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
            // New file (not in DB by path).
            let audio = file_service::probe_audio(path)?;
            if sidecar::sidecar_path_for_audio(path).exists() {
                // Has its own sidecar identity — insert immediately.
                file_service::upsert_file(
                    db, path, &mut sc,
                    &hash, &mtime, size_bytes,
                    &audio, None,
                ).await?;
                report.files_added.push(name);
            } else {
                // No sidecar — may match an orphaned sidecar. Defer insertion.
                pending_new.push(PendingNew { path: path.to_path_buf(), hash, mtime, size_bytes, audio, name });
            }
        }

        on_progress();
    }

    // ── Post-walk: repair orphaned sidecars ──────────────────────────────────

    struct OrphanEntry {
        old_path:     std::path::PathBuf,
        sidecar_path: std::path::PathBuf,
        db_hash:      String,
        db_id:        String,
        name:         String,
    }

    let mut orphaned: Vec<OrphanEntry> = Vec::new();
    for removed_path_str in &remaining {
        let removed_path = Path::new(removed_path_str);
        let sc_path = sidecar::sidecar_path_for_audio(removed_path);
        if sc_path.exists() {
            if let Some(db_rec) = path_map.get(removed_path_str) {
                let name = removed_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                orphaned.push(OrphanEntry {
                    old_path:     removed_path.to_path_buf(),
                    sidecar_path: sc_path,
                    db_hash:      db_rec.hash.clone(),
                    db_id:        db_rec.id.clone(),
                    name,
                });
            }
        }
    }

    // Build hash → index map over pending_new (first occurrence wins).
    let mut hash_index: HashMap<String, usize> = HashMap::new();
    for (i, pn) in pending_new.iter().enumerate() {
        hash_index.entry(pn.hash.clone()).or_insert(i);
    }

    // Repair matched pairs.
    let mut consumed: HashSet<usize> = HashSet::new();
    for orphan in &orphaned {
        let old_path_str = orphan.old_path.to_string_lossy().to_string();
        if let Some(&idx) = hash_index.get(&orphan.db_hash) {
            if consumed.contains(&idx) {
                continue;
            }
            let pn = &pending_new[idx];

            let new_sc_path = sidecar::sidecar_path_for_audio(&pn.path);
            std::fs::rename(&orphan.sidecar_path, &new_sc_path)?;

            let mut sc = sidecar::read_file_sidecar(&pn.path)?;

            let db_rec = path_map.get(&old_path_str).unwrap();
            let audio = AudioInfo {
                duration:    db_rec.duration,
                sample_rate: db_rec.sample_rate as u32,
                channels:    db_rec.channels as u32,
                mime_type:   db_rec.mime_type.clone(),
            };

            file_service::upsert_file(
                db, &pn.path, &mut sc,
                &orphan.db_hash, &pn.mtime, pn.size_bytes,
                &audio, Some(db_rec),
            ).await?;

            remaining.remove(&old_path_str);
            consumed.insert(idx);
            report.files_repaired.push(format!("{} → {}", orphan.name, pn.name));
            on_progress();
        }
    }

    // Collect unresolvable orphans — remove from `remaining` so the deletion
    // step does not cascade-delete them; the CLI will handle cleanup.
    for orphan in &orphaned {
        let old_path_str = orphan.old_path.to_string_lossy().to_string();
        if remaining.contains(&old_path_str) {
            remaining.remove(&old_path_str);
            report.orphaned_sidecars.push(OrphanedSidecarInfo {
                name:         orphan.name.clone(),
                sidecar_path: orphan.sidecar_path.clone(),
                db_id:        orphan.db_id.clone(),
            });
        }
    }

    // Insert pending_new entries not consumed by repair.
    for (i, pn) in pending_new.iter().enumerate() {
        if consumed.contains(&i) {
            continue;
        }
        let mut sc = sidecar::FileSidecar::default_for_file();
        file_service::upsert_file(
            db, &pn.path, &mut sc,
            &pn.hash, &pn.mtime, pn.size_bytes,
            &pn.audio, None,
        ).await?;
        report.files_added.push(pn.name.clone());
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

pub async fn remove_orphaned_sidecar(
    db: &DatabaseConnection,
    sidecar_path: &Path,
    db_id: &str,
) -> Result<(), ServiceError> {
    if sidecar_path.exists() {
        std::fs::remove_file(sidecar_path)?;
    }
    file_service::delete_file_cascade(db, db_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LibraryPaths;
    use crate::db::test_db;
    use sea_orm::ActiveModelTrait;
    use sea_orm::ActiveValue::Set;
    use tempfile::tempdir;

    fn make_paths(dir: &std::path::Path) -> LibraryPaths {
        LibraryPaths {
            library_dir:   dir.to_path_buf(),
            files_dir:     dir.to_path_buf(),
            catalog_dir:   dir.join("catalog"),
            generated_dir: dir.join("catalog/generated"),
        }
    }

    fn write_wav(path: &std::path::Path) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        writer.write_sample(0i16).unwrap();
        writer.finalize().unwrap();
    }

    async fn insert_file_row(
        db: &DatabaseConnection,
        id: &str,
        path: &str,
        hash: &str,
    ) -> crate::db::entities::file::Model {
        use crate::db::entities::file;
        let now = chrono::Utc::now().to_rfc3339();
        file::ActiveModel {
            id:          Set(id.to_string()),
            slug:        Set(slug::slugify(
                std::path::Path::new(path)
                    .file_stem().unwrap().to_string_lossy().as_ref()
            )),
            name:        Set(std::path::Path::new(path)
                .file_stem().unwrap().to_string_lossy().to_string()),
            path:        Set(path.to_string()),
            duration:    Set(1.0),
            sample_rate: Set(44100),
            channels:    Set(2),
            mime_type:   Set("audio/wav".to_string()),
            hash:        Set(hash.to_string()),
            mtime:       Set(now.clone()),
            size_bytes:  Set(4),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
        .insert(db)
        .await
        .unwrap()
    }

    fn write_sidecar(audio_path: &std::path::Path, id: &str) {
        let sc = crate::sidecar::FileSidecar {
            id: id.to_string(),
            version: 2,
            metadata: crate::sidecar::FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        };
        crate::sidecar::write_file_sidecar(audio_path, &sc).unwrap();
    }

    // ── repair: sidecar orphaned, matching audio exists ──────────────────────

    #[tokio::test]
    async fn repair_renames_sidecar_and_updates_db() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("catalog")).unwrap();

        let old_audio  = dir.path().join("kick.wav");
        let new_audio  = dir.path().join("kick-renamed.wav");
        let uuid = "aaaaaaaa-0000-0000-0000-000000000001";

        // Write a valid WAV at the new location and compute its hash.
        write_wav(&new_audio);
        let hash = file_service::file_hash(&new_audio).unwrap();

        // Insert DB record (old path, same hash).
        insert_file_row(&db, uuid, old_audio.to_str().unwrap(), &hash).await;

        // Orphaned sidecar at old path (audio itself is absent).
        let old_sidecar = crate::sidecar::sidecar_path_for_audio(&old_audio);
        write_sidecar(&old_audio, uuid);
        assert!(!old_audio.exists());

        let new_sidecar = crate::sidecar::sidecar_path_for_audio(&new_audio);
        assert!(!new_sidecar.exists());

        let paths  = make_paths(dir.path());
        let report = sync_library(&db, &paths, || {}).await.unwrap();

        assert_eq!(report.files_repaired.len(), 1, "should report one repaired file");
        assert!(report.files_added.is_empty(),   "repaired file must not appear as added");
        assert!(report.files_removed.is_empty(), "old record must not be deleted");

        assert!(!old_sidecar.exists(), "orphaned sidecar should be gone");
        assert!(new_sidecar.exists(),  "sidecar should sit next to new audio");

        let row = crate::db::entities::file::Entity::find_by_id(uuid)
            .one(&db).await.unwrap().unwrap();
        assert_eq!(row.path, new_audio.to_str().unwrap());
    }

    // ── orphan: sidecar orphaned, no matching audio ──────────────────────────

    #[tokio::test]
    async fn orphan_with_no_match_reported_not_deleted() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("catalog")).unwrap();

        let old_audio = dir.path().join("lost.wav");
        let uuid = "bbbbbbbb-0000-0000-0000-000000000002";

        insert_file_row(&db, uuid, old_audio.to_str().unwrap(), "deadbeef").await;

        write_sidecar(&old_audio, uuid);
        assert!(!old_audio.exists());

        let paths  = make_paths(dir.path());
        let report = sync_library(&db, &paths, || {}).await.unwrap();

        assert_eq!(report.orphaned_sidecars.len(), 1);
        assert_eq!(report.orphaned_sidecars[0].name, "lost");
        assert_eq!(report.orphaned_sidecars[0].db_id, uuid);
        assert!(report.files_removed.is_empty(), "orphan must not be auto-deleted");

        let row = crate::db::entities::file::Entity::find_by_id(uuid)
            .one(&db).await.unwrap();
        assert!(row.is_some(), "DB record must survive");
        assert!(crate::sidecar::sidecar_path_for_audio(&old_audio).exists());
    }

    // ── remove_orphaned_sidecar ──────────────────────────────────────────────

    #[tokio::test]
    async fn remove_orphaned_sidecar_deletes_sidecar_and_db_record() {
        let db = test_db().await;
        let dir = tempdir().unwrap();

        let audio = dir.path().join("gone.wav");
        let uuid  = "cccccccc-0000-0000-0000-000000000003";
        insert_file_row(&db, uuid, audio.to_str().unwrap(), "hash999").await;

        let sc_path = crate::sidecar::sidecar_path_for_audio(&audio);
        write_sidecar(&audio, uuid);
        assert!(sc_path.exists());

        remove_orphaned_sidecar(&db, &sc_path, uuid).await.unwrap();

        assert!(!sc_path.exists(), "sidecar file should be removed");
        let row = crate::db::entities::file::Entity::find_by_id(uuid)
            .one(&db).await.unwrap();
        assert!(row.is_none(), "DB record should be deleted");
    }

    // ── pending_new without orphan → inserted as new ─────────────────────────

    #[tokio::test]
    async fn sidecar_less_audio_with_no_orphan_inserted_as_new() {
        let db = test_db().await;
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("catalog")).unwrap();

        let audio = dir.path().join("brand-new.wav");
        write_wav(&audio);

        let paths  = make_paths(dir.path());
        let report = sync_library(&db, &paths, || {}).await.unwrap();

        assert_eq!(report.files_added.len(), 1);
        assert!(report.files_repaired.is_empty());

        let rows = crate::db::entities::file::Entity::find()
            .all(&db).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "brand-new");
    }
}
