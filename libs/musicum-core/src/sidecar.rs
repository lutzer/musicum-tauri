use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::edit::{deserialize_processor_edits, ProcessorEdit};
use crate::ServiceError;

// ── Legacy processor entry (kept for potential future migration paths) ───────
// These types are no longer part of the public API; use `ProcessorEdit` instead.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ProcessorRef {
    pub(crate) id:     String,
    pub(crate) params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum ProcessorEntry {
    Structural {
        id:        String,
        enabled:   bool,
        processor: ProcessorRef,
    },
    #[serde(rename = "audio-plugin")]
    AudioPlugin {
        id:        String,
        enabled:   bool,
        processor: ProcessorRef,
    },
}

// ── Audio-file sidecar ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSidecar {
    #[serde(default)]
    pub id: String,
    pub version: u32,
    pub metadata: FileMetadataSidecar,
    #[serde(default)]
    pub attachments: Vec<AttachmentSidecar>,
    #[serde(default)]
    pub clips: Vec<ClipSidecar>,
}

impl FileSidecar {
    pub fn default_for_file() -> Self {
        FileSidecar {
            id: String::new(),
            version: 2,
            metadata: FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMetadataSidecar {
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub rating: Option<i32>,
    pub color: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentSidecar {
    pub uuid: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSidecar {
    pub slug:  String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// Processor and plugin edits for this clip.
    /// Deserialized with migration support for old `ProcessorEntry` format.
    #[serde(default, deserialize_with = "deserialize_clip_processors")]
    pub processors: Vec<ProcessorEdit>,
}

fn deserialize_clip_processors<'de, D>(d: D) -> Result<Vec<ProcessorEdit>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Capture raw JSON array value, then run migration-aware helper
    let raw = serde_json::Value::deserialize(d)?;
    let json_str = raw.to_string();
    Ok(deserialize_processor_edits(&json_str))
}

// ── Read/write helpers ────────────────────────────────────────────────────

pub fn read_file_sidecar(audio_path: &Path) -> Result<FileSidecar, ServiceError> {
    let sidecar_path = sidecar_path_for_audio(audio_path);
    if !sidecar_path.exists() {
        return Ok(FileSidecar::default_for_file());
    }
    let text = std::fs::read_to_string(&sidecar_path)?;
    Ok(serde_json::from_str(&text)?)
}

pub fn write_file_sidecar(audio_path: &Path, sidecar: &FileSidecar) -> Result<(), ServiceError> {
    let sidecar_path = sidecar_path_for_audio(audio_path);
    let json = serde_json::to_string_pretty(sidecar)?;
    std::fs::write(&sidecar_path, json)?;
    Ok(())
}

pub fn sidecar_path_for_audio(audio_path: &Path) -> std::path::PathBuf {
    let stem = audio_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    audio_path
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem}.musicum.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::edit::{EditKind, ProcessorEdit};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn write_read_sidecar_with_processor_edits() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        std::fs::write(&audio, b"").unwrap();

        let mut params = HashMap::new();
        params.insert("start".to_string(), 1.5_f64);
        let edit = ProcessorEdit {
            uuid: Uuid::new_v4(),
            enabled: true,
            kind: EditKind::Structural { processor_id: "trim".to_string(), params },
        };

        let sc = FileSidecar {
            id: "test-file-id".to_string(),
            version: 1,
            metadata: FileMetadataSidecar::default(),
            attachments: vec![],
            clips: vec![ClipSidecar {
                slug: "c".to_string(),
                title: "C".to_string(),
                notes: String::new(),
                processors: vec![edit.clone()],
            }],
        };

        write_file_sidecar(&audio, &sc).unwrap();
        let loaded = read_file_sidecar(&audio).unwrap();
        assert_eq!(loaded.clips[0].processors[0], edit);
    }

    #[test]
    fn read_sidecar_with_old_processor_entry_format() {
        // Simulate a sidecar file written before this change
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.wav");
        let old_json = r#"{
            "version": 1,
            "metadata": {},
            "clips": [{
                "slug": "c", "title": "C", "notes": "",
                "processors": [{
                    "type": "structural",
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "enabled": true,
                    "processor": {"id": "trim", "params": {"start": 2.0}}
                }]
            }]
        }"#;
        let sidecar_path = audio.with_file_name("test.wav.musicum.json");
        std::fs::write(&sidecar_path, old_json).unwrap();

        let sc = read_file_sidecar(&audio).unwrap();
        assert_eq!(sc.clips.len(), 1);
        assert_eq!(sc.clips[0].processors.len(), 1);
        let edit = &sc.clips[0].processors[0];
        assert_eq!(edit.uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
        match &edit.kind {
            EditKind::Structural { processor_id, params } => {
                assert_eq!(processor_id, "trim");
                assert_eq!(params["start"], 2.0);
            }
            _ => panic!("expected Structural"),
        }
    }
}

