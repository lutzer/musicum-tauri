pub mod processors;

use structural_processor_sdk::{ProcessorEntry, Registry};
use processors::{
    crop::CropProcessor, cut::CutProcessor,
    slice::SliceProcessor, trim::TrimProcessor,
};

pub fn registry() -> Registry {
    let mut m = Registry::new();
    m.insert("trim".to_string(),  ProcessorEntry::of::<TrimProcessor>());
    m.insert("cut".to_string(),   ProcessorEntry::of::<CutProcessor>());
    m.insert("slice".to_string(), ProcessorEntry::of::<SliceProcessor>());
    m.insert("crop".to_string(),  ProcessorEntry::of::<CropProcessor>());
    m
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_registry_has_four_entries() {
        let r = super::registry();
        assert_eq!(r.len(), 4);
        assert!(r.contains_key("trim"));
        assert!(r.contains_key("cut"));
        assert!(r.contains_key("slice"));
        assert!(r.contains_key("crop"));
    }
}

#[cfg(test)]
mod integration_tests {
    use structural_processor_sdk::{
        build_chain, chain_output_duration, VecAudioSource, AudioSource,
        map_time_forward, map_time_back, validate_edit,
        chain::StructuralEdit,
    };
    use std::collections::HashMap;

    fn edits(json: &str) -> Vec<StructuralEdit> {
        serde_json::from_str(json).unwrap()
    }

    fn mono_src(frames: usize) -> Box<dyn AudioSource> {
        Box::new(VecAudioSource::new(
            (0..frames).map(|i| i as f32).collect(),
            100,
            1,
        ))
    }

    #[test]
    fn chain_trim_then_read_correct_length() {
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":0.2,"end":0.2}}]"#);
        let mut chain = build_chain(mono_src(100), &es, &super::registry());
        let out = chain.read_at(0.0, 60);
        assert_eq!(out.len(), 60);
        assert!((out[0] - 20.0).abs() < 1e-6);
    }

    #[test]
    fn chain_trim_then_cut() {
        let es = edits(r#"[
            {"type":"trim","enabled":true,"parameters":{"start":0.2,"end":0.2}},
            {"type":"cut","enabled":true,"parameters":{"from":0.1,"to":0.3}}
        ]"#);
        let output_dur = chain_output_duration(1.0, &es, &super::registry());
        assert!((output_dur - 0.4).abs() < 1e-9);
    }

    #[test]
    fn chain_skips_disabled_edits() {
        let es = edits(r#"[{"type":"trim","enabled":false,"parameters":{"start":0.5,"end":0.9}}]"#);
        let chain = build_chain(mono_src(100), &es, &super::registry());
        assert!((chain.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn chain_unknown_type_is_passthrough() {
        let es = edits(r#"[{"type":"wormhole","enabled":true,"parameters":{}}]"#);
        let chain = build_chain(mono_src(50), &es, &super::registry());
        assert!((chain.duration_secs() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn validate_edit_trim_valid() {
        let mut p = HashMap::new();
        p.insert("start".into(), 0.5_f64);
        p.insert("end".into(), 1.5_f64);
        assert!(validate_edit(&super::registry(), "trim", &p));
    }

    #[test]
    fn validate_edit_unknown_type_is_false() {
        assert!(!validate_edit(&super::registry(), "wormhole", &HashMap::new()));
    }

    #[test]
    fn map_time_forward_trim_identity_at_zero() {
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":0.0,"end":0.0}}]"#);
        let result = map_time_forward(&super::registry(), &es, 0.0, 1.0);
        assert!(result.abs() < 1e-9);
    }

    #[test]
    fn map_time_back_trim_adds_start() {
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":1.0,"end":2.0}}]"#);
        let result = map_time_back(&super::registry(), &es, 0.5, 2.0);
        assert!((result - 1.5).abs() < 1e-9);
    }
}
