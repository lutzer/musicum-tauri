use std::collections::HashMap;

use serde::Deserialize;

use crate::{Params, ProcessorDescriptor, ProcessorEntry};

#[derive(Deserialize, Default)]
pub struct Edit {
    #[serde(rename = "type")]
    pub edit_type: String,
    pub enabled: bool,
    pub parameters: HashMap<String, f64>,
}

pub fn apply_chain(
    registry: &[ProcessorEntry],
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    edits: &[Edit],
) -> Vec<f32> {
    let mut current = samples.to_vec();
    for edit in edits {
        if !edit.enabled {
            continue;
        }
        if let Some(entry) = find(registry, &edit.edit_type) {
            current = (entry.apply)(&current, sample_rate, channels, &edit.parameters);
        }
    }
    current
}

pub fn descriptors_json(registry: &[ProcessorEntry]) -> String {
    let descriptors: Vec<&ProcessorDescriptor> =
        registry.iter().map(|e| (e.descriptor)()).collect();
    serde_json::to_string(&descriptors).expect("descriptor serialisation failed")
}

pub fn validate_edit(registry: &[ProcessorEntry], edit_type: &str, params: &Params) -> bool {
    find(registry, edit_type).is_some_and(|e| (e.validate)(params))
}

/// Map `t` forward through the edit chain.
/// `duration` is the raw audio length in seconds.
pub fn map_time_forward(
    registry: &[ProcessorEntry],
    edits: &[Edit],
    t: f64,
    duration: f64,
) -> f64 {
    let mut current_t = t;
    let mut current_dur = duration;
    for edit in edits {
        if !edit.enabled {
            continue;
        }
        if let Some(entry) = find(registry, &edit.edit_type) {
            current_t = (entry.map_time_forward)(current_t, current_dur, &edit.parameters);
            current_dur = (entry.output_duration)(current_dur, &edit.parameters);
        }
    }
    current_t
}

/// Map `t` backward through the edit chain.
/// `duration` is the raw audio length in seconds.
pub fn map_time_back(
    registry: &[ProcessorEntry],
    edits: &[Edit],
    t: f64,
    duration: f64,
) -> f64 {
    // Pre-compute input duration before each edit (needed for reverse traversal)
    let mut durations = Vec::with_capacity(edits.len() + 1);
    durations.push(duration);
    for edit in edits.iter() {
        let last = *durations.last().unwrap();
        let next = if !edit.enabled {
            last
        } else if let Some(entry) = find(registry, &edit.edit_type) {
            (entry.output_duration)(last, &edit.parameters)
        } else {
            last
        };
        durations.push(next);
    }

    let mut current_t = t;
    for (i, edit) in edits.iter().enumerate().rev() {
        if !edit.enabled {
            continue;
        }
        if let Some(entry) = find(registry, &edit.edit_type) {
            current_t = (entry.map_time_back)(current_t, durations[i], &edit.parameters);
        }
    }
    current_t
}

fn find<'a>(registry: &'a [ProcessorEntry], edit_type: &str) -> Option<&'a ProcessorEntry> {
    registry.iter().find(|e| (e.descriptor)().id == edit_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParameterDescriptor, ProcessorDescriptor, ProcessorEntry, StructuralProcessor};
    use std::collections::HashMap;

    // ── Minimal test processors (no real audio logic needed here) ─────────────

    static PASS_PARAMS: [ParameterDescriptor; 0] = [];
    static PASS_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "pass", name: "Pass", parameters: &PASS_PARAMS };

    struct PassProcessor;
    impl StructuralProcessor for PassProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &PASS_DESC }
        fn validate(_: &Params) -> bool { true }
        fn apply(s: &[f32], _: u32, _: u16, _: &Params) -> Vec<f32> { s.to_vec() }
        fn output_duration(d: f64, _: &Params) -> f64 { d }
        fn map_time_forward(t: f64, _: f64, _: &Params) -> f64 { t }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    // HalfProcessor keeps the first half of audio; duration → duration/2
    static HALF_PARAMS: [ParameterDescriptor; 0] = [];
    static HALF_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "half", name: "Half", parameters: &HALF_PARAMS };

    struct HalfProcessor;
    impl StructuralProcessor for HalfProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &HALF_DESC }
        fn validate(_: &Params) -> bool { true }
        fn apply(s: &[f32], _: u32, ch: u16, _: &Params) -> Vec<f32> {
            let ch = ch as usize;
            s[..s.len() / (2 * ch) * ch].to_vec()
        }
        fn output_duration(d: f64, _: &Params) -> f64 { d / 2.0 }
        fn map_time_forward(t: f64, duration: f64, _: &Params) -> f64 { t.min(duration / 2.0) }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    fn reg() -> Vec<ProcessorEntry> {
        vec![
            ProcessorEntry::of::<PassProcessor>(),
            ProcessorEntry::of::<HalfProcessor>(),
        ]
    }

    fn edits(json: &str) -> Vec<Edit> {
        serde_json::from_str(json).unwrap()
    }

    fn sine(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32 / frames as f32).collect()
    }

    #[test]
    fn apply_chain_passthrough() {
        let es = edits(r#"[{"type":"pass","enabled":true,"parameters":{}}]"#);
        assert_eq!(apply_chain(&reg(), &sine(100), 100, 1, &es).len(), 100);
    }

    #[test]
    fn apply_chain_skips_disabled_edits() {
        let es = edits(r#"[{"type":"half","enabled":false,"parameters":{}}]"#);
        assert_eq!(apply_chain(&reg(), &sine(100), 100, 1, &es).len(), 100);
    }

    #[test]
    fn apply_chain_unknown_type_is_passthrough() {
        let es = edits(r#"[{"type":"wormhole","enabled":true,"parameters":{}}]"#);
        assert_eq!(apply_chain(&reg(), &sine(50), 100, 1, &es).len(), 50);
    }

    #[test]
    fn descriptors_json_contains_registered_ids() {
        let json = descriptors_json(&reg());
        assert!(json.contains("\"id\":\"pass\""));
        assert!(json.contains("\"id\":\"half\""));
    }

    #[test]
    fn validate_edit_known_type_passes() {
        assert!(validate_edit(&reg(), "pass", &HashMap::new()));
    }

    #[test]
    fn validate_edit_unknown_type_is_false() {
        assert!(!validate_edit(&reg(), "wormhole", &HashMap::new()));
    }

    #[test]
    fn map_time_forward_accumulates_duration_across_edits() {
        // Two half edits: 1.0s → 0.5s → 0.25s; t=0.2 stays 0.2 throughout
        let es = edits(r#"[
            {"type":"half","enabled":true,"parameters":{}},
            {"type":"half","enabled":true,"parameters":{}}
        ]"#);
        let result = map_time_forward(&reg(), &es, 0.2, 1.0);
        assert!((result - 0.2).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_identity_for_passthrough() {
        let es = edits(r#"[{"type":"pass","enabled":true,"parameters":{}}]"#);
        let result = map_time_back(&reg(), &es, 0.5, 1.0);
        assert!((result - 0.5).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_empty_edits_is_identity() {
        assert!((map_time_forward(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_empty_edits_is_identity() {
        assert!((map_time_back(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }
}
