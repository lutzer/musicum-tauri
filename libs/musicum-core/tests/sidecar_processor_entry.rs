// These tests verify that the migration path from the old ProcessorEntry format
// to the new ProcessorEdit format works correctly.
use musicum_core::edit::{deserialize_processor_edits, EditKind};

#[test]
fn structural_entry_round_trips_via_migration() {
    // Old format JSON
    let old_json = r#"[{"type":"structural","id":"uuid-1","enabled":true,"processor":{"id":"trim","params":{"start":0.0,"end":0.0}}}]"#;
    let edits = deserialize_processor_edits(old_json);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].enabled, true);
    match &edits[0].kind {
        EditKind::Structural { processor_id, params } => {
            assert_eq!(processor_id, "trim");
            assert_eq!(params["start"], 0.0);
        }
        _ => panic!("expected Structural"),
    }
}

#[test]
fn audio_plugin_entry_round_trips_via_migration() {
    // Old format JSON
    let old_json = r#"[{"type":"audio-plugin","id":"uuid-2","enabled":false,"processor":{"id":"gain","params":{"gain":0.8}}}]"#;
    let edits = deserialize_processor_edits(old_json);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].enabled, false);
    match &edits[0].kind {
        EditKind::Plugin { plugin_id, params } => {
            assert_eq!(plugin_id, "gain");
            assert!((params["gain"] - 0.8).abs() < 1e-6);
        }
        _ => panic!("expected Plugin"),
    }
}
