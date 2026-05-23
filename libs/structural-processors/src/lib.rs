pub mod processors;

structural_processor_sdk::implement_sp_chain!(
    processors::trim::TrimProcessor,
    processors::cut::CutProcessor,
    processors::slice::SliceProcessor,
    processors::crop::CropProcessor,
);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use structural_processor_sdk::chain::{
        apply_chain, descriptors_json, map_time_back, map_time_forward, validate_edit, Edit,
    };

    fn registry() -> Vec<structural_processor_sdk::ProcessorEntry> {
        use crate::processors::{
            crop::CropProcessor, cut::CutProcessor, slice::SliceProcessor, trim::TrimProcessor,
        };
        use structural_processor_sdk::ProcessorEntry;
        vec![
            ProcessorEntry::of::<TrimProcessor>(),
            ProcessorEntry::of::<CutProcessor>(),
            ProcessorEntry::of::<SliceProcessor>(),
            ProcessorEntry::of::<CropProcessor>(),
        ]
    }

    fn edits(json: &str) -> Vec<Edit> {
        serde_json::from_str(json).unwrap()
    }

    fn sine(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32 / frames as f32).collect()
    }

    #[test]
    fn chain_trim_then_cut() {
        // 100-frame mono @100Hz; trim start=0.2, end=0.2 → keep [0.2, 0.8] = 60 frames; cut [0.1, 0.3] → 40 frames
        let edit_json = r#"[
            {"type":"trim","enabled":true,"parameters":{"start":0.2,"end":0.2}},
            {"type":"cut","enabled":true,"parameters":{"from":0.1,"to":0.3}}
        ]"#;
        let result = apply_chain(&registry(), &sine(100), 100, 1, &edits(edit_json));
        assert_eq!(result.len(), 40);
    }

    #[test]
    fn chain_skips_disabled_edits() {
        let edit_json = r#"[
            {"type":"trim","enabled":false,"parameters":{"start":0.5,"end":0.9}}
        ]"#;
        let result = apply_chain(&registry(), &sine(100), 100, 1, &edits(edit_json));
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn chain_unknown_type_is_passthrough() {
        let edit_json = r#"[{"type":"wormhole","enabled":true,"parameters":{}}]"#;
        let result = apply_chain(&registry(), &sine(50), 100, 1, &edits(edit_json));
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn descriptors_json_contains_all_four() {
        let json = descriptors_json(&registry());
        assert!(json.contains("\"id\":\"trim\""));
        assert!(json.contains("\"id\":\"cut\""));
        assert!(json.contains("\"id\":\"slice\""));
        assert!(json.contains("\"id\":\"crop\""));
    }

    #[test]
    fn validate_edit_trim_valid() {
        let mut p = HashMap::new();
        p.insert("start".into(), 0.5_f64);
        p.insert("end".into(), 1.5_f64);
        assert!(validate_edit(&registry(), "trim", &p));
    }

    #[test]
    fn validate_edit_unknown_type_is_false() {
        assert!(!validate_edit(&registry(), "wormhole", &HashMap::new()));
    }

    #[test]
    fn map_time_forward_trim_identity_at_zero() {
        let edit_json =
            r#"[{"type":"trim","enabled":true,"parameters":{"start":0.0,"end":0.0}}]"#;
        // raw duration = 1.0s; no trimming → t=0.0 maps to 0.0
        let result = map_time_forward(&registry(), &edits(edit_json), 0.0, 1.0);
        assert!(result.abs() < 1e-9);
    }

    #[test]
    fn map_time_back_trim_adds_start() {
        let edit_json =
            r#"[{"type":"trim","enabled":true,"parameters":{"start":1.0,"end":2.0}}]"#;
        // raw duration = 2.0s
        let result = map_time_back(&registry(), &edits(edit_json), 0.5, 2.0);
        assert!((result - 1.5).abs() < 1e-9);
    }
}
