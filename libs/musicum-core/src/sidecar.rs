use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::ServiceError;

// ── Processor entry (shared by clips and presets) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorRef {
    pub id:     String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ProcessorEntry {
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
            version: 1,
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
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub processors: Vec<ProcessorEntry>,
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

