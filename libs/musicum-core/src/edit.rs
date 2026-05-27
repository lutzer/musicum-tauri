use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unified edit descriptor for both structural processors and audio plugins.
/// Stored in `ClipSidecar.processors` and passed to `PlaybackEngine`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessorEdit {
    pub uuid:    Uuid,
    pub enabled: bool,
    pub kind:    EditKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum EditKind {
    Structural {
        processor_id: String,
        #[serde(default)]
        params: HashMap<String, f64>,
    },
    Plugin {
        plugin_id: String,
        #[serde(default)]
        params: HashMap<String, f32>,
    },
}

/// Deserialize `Vec<ProcessorEdit>` from a JSON string, falling back to the
/// legacy `ProcessorEntry` format if the new format fails to parse.
///
/// Old format (written before this change):
/// `[{"type":"structural","id":"<uuid>","enabled":true,"processor":{"id":"trim","params":{...}}}]`
///
/// New format:
/// `[{"uuid":"<uuid>","enabled":true,"kind":{"type":"structural","processor_id":"trim","params":{...}}}]`
pub fn deserialize_processor_edits(json: &str) -> Vec<ProcessorEdit> {
    // Try new format
    if let Ok(edits) = serde_json::from_str::<Vec<ProcessorEdit>>(json) {
        return edits;
    }
    // Fall back to old ProcessorEntry format, then convert
    #[derive(Deserialize)]
    struct OldProcessorRef { id: String, params: serde_json::Value }
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "kebab-case")]
    enum OldEntry {
        Structural { id: String, enabled: bool, processor: OldProcessorRef },
        #[serde(rename = "audio-plugin")]
        AudioPlugin { id: String, enabled: bool, processor: OldProcessorRef },
    }

    fn json_to_f64_map(v: &serde_json::Value) -> HashMap<String, f64> {
        v.as_object()
            .map(|o| o.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f))).collect())
            .unwrap_or_default()
    }

    serde_json::from_str::<Vec<OldEntry>>(json)
        .unwrap_or_default()
        .into_iter()
        .map(|old| match old {
            OldEntry::Structural { id, enabled, processor } => ProcessorEdit {
                uuid: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                enabled,
                kind: EditKind::Structural {
                    processor_id: processor.id,
                    params: json_to_f64_map(&processor.params),
                },
            },
            OldEntry::AudioPlugin { id, enabled, processor } => ProcessorEdit {
                uuid: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::new_v4()),
                enabled,
                kind: EditKind::Plugin {
                    plugin_id: processor.id,
                    params: json_to_f64_map(&processor.params)
                        .into_iter()
                        .map(|(k, v)| (k, v as f32))
                        .collect(),
                },
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_structural() -> ProcessorEdit {
        let mut params = HashMap::new();
        params.insert("start".to_string(), 1.0_f64);
        ProcessorEdit {
            uuid: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            enabled: true,
            kind: EditKind::Structural { processor_id: "trim".to_string(), params },
        }
    }

    fn make_plugin() -> ProcessorEdit {
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 0.5_f32);
        ProcessorEdit {
            uuid: Uuid::parse_str("660e8400-e29b-41d4-a716-446655440001").unwrap(),
            enabled: false,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }
    }

    #[test]
    fn roundtrip_structural() {
        let edit = make_structural();
        let json = serde_json::to_string(&edit).unwrap();
        let back: ProcessorEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(edit, back);
    }

    #[test]
    fn roundtrip_plugin() {
        let edit = make_plugin();
        let json = serde_json::to_string(&edit).unwrap();
        let back: ProcessorEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(edit, back);
    }

    #[test]
    fn deserialize_new_format_vec() {
        let edits = vec![make_structural(), make_plugin()];
        let json = serde_json::to_string(&edits).unwrap();
        let result = deserialize_processor_edits(&json);
        assert_eq!(result, edits);
    }

    #[test]
    fn deserialize_old_structural_format() {
        // Old sidecar JSON (written by previous code)
        let old_json = r#"[
            {
                "type": "structural",
                "id": "550e8400-e29b-41d4-a716-446655440000",
                "enabled": true,
                "processor": {"id": "trim", "params": {"start": 1.0}}
            }
        ]"#;
        let result = deserialize_processor_edits(old_json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uuid.to_string(), "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(result[0].enabled, true);
        match &result[0].kind {
            EditKind::Structural { processor_id, params } => {
                assert_eq!(processor_id, "trim");
                assert_eq!(params["start"], 1.0);
            }
            _ => panic!("expected Structural"),
        }
    }

    #[test]
    fn deserialize_old_audio_plugin_format() {
        let old_json = r#"[
            {
                "type": "audio-plugin",
                "id": "660e8400-e29b-41d4-a716-446655440001",
                "enabled": false,
                "processor": {"id": "gain", "params": {"gain": 0.5}}
            }
        ]"#;
        let result = deserialize_processor_edits(old_json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].enabled, false);
        match &result[0].kind {
            EditKind::Plugin { plugin_id, params } => {
                assert_eq!(plugin_id, "gain");
                assert!((params["gain"] - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected Plugin"),
        }
    }

    #[test]
    fn deserialize_empty_json() {
        assert_eq!(deserialize_processor_edits("[]"), vec![]);
    }

    #[test]
    fn deserialize_garbage_returns_empty() {
        assert_eq!(deserialize_processor_edits("not json"), vec![]);
    }
}
