use musicum_core::sidecar::{ProcessorEntry, ProcessorRef};

#[test]
fn structural_entry_round_trips() {
    let entry = ProcessorEntry::Structural {
        id: "uuid-1".into(),
        enabled: true,
        processor: ProcessorRef {
            id: "trim".into(),
            params: serde_json::json!({ "start": 0.0, "end": 0.0 }),
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"structural\""));
    let back: ProcessorEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}

#[test]
fn audio_plugin_entry_round_trips() {
    let entry = ProcessorEntry::AudioPlugin {
        id: "uuid-2".into(),
        enabled: false,
        processor: ProcessorRef {
            id: "gain".into(),
            params: serde_json::json!({ "gain": 0.8 }),
        },
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(json.contains("\"type\":\"audio-plugin\""));
    let back: ProcessorEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, entry);
}
