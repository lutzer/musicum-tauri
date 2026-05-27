use serde::Deserialize;

use crate::{AudioSource, Params, ProcessorDescriptor, Registry};

#[derive(Deserialize, Default, Clone)]
pub struct StructuralEdit {
    #[serde(rename = "type")]
    pub processor_id: String,
    pub enabled: bool,
    #[serde(rename = "parameters")]
    pub params: Params,
}

// ── ProcessorSource ───────────────────────────────────────────────────────────

struct ProcessorSource {
    processor: Box<dyn crate::StreamingProcessorInstance>,
    inner: Box<dyn AudioSource>,
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
}

impl AudioSource for ProcessorSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        let end_secs = start_secs
            + num_samples as f64 / (self.sample_rate as f64 * self.channels as f64);
        self.processor.fill(start_secs, end_secs, &mut *self.inner)
    }
    fn duration_secs(&self) -> f64 { self.duration_secs }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn channels(&self) -> u16 { self.channels }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fold `edits` over `source`, nesting each enabled processor as a `ProcessorSource`.
/// The returned `AudioSource` is the head of the chain; call `read_at` to pull samples.
pub fn build_chain(
    source: Box<dyn AudioSource>,
    edits: &[StructuralEdit],
    registry: &Registry,
) -> Box<dyn AudioSource> {
    edits.iter()
        .filter(|e| e.enabled)
        .fold(source, |inner, edit| {
            let Some(entry) = registry.get(&edit.processor_id) else {
                return inner;
            };
            let output_duration = (entry.output_duration)(inner.duration_secs(), &edit.params);
            let sample_rate = inner.sample_rate();
            let channels = inner.channels();
            let processor = (entry.create)(edit.params.clone());
            Box::new(ProcessorSource {
                processor,
                inner,
                sample_rate,
                channels,
                duration_secs: output_duration,
            })
        })
}

/// Compute the output duration (seconds) of a chain without constructing instances.
pub fn chain_output_duration(raw_duration: f64, edits: &[StructuralEdit], registry: &Registry) -> f64 {
    edits.iter()
        .filter(|e| e.enabled)
        .fold(raw_duration, |dur, edit| {
            registry.get(&edit.processor_id)
                .map(|entry| (entry.output_duration)(dur, &edit.params))
                .unwrap_or(dur)
        })
}

pub fn descriptors_json(registry: &Registry) -> String {
    let mut descriptors: Vec<&ProcessorDescriptor> =
        registry.values().map(|e| (e.descriptor)()).collect();
    descriptors.sort_by_key(|d| d.id);
    serde_json::to_string(&descriptors).expect("descriptor serialisation failed")
}

pub fn validate_edit(registry: &Registry, processor_id: &str, params: &Params) -> bool {
    registry.get(processor_id).is_some_and(|e| (e.validate)(params))
}

/// Map `t` forward through the edit chain. `duration` is the raw audio length in seconds.
pub fn map_time_forward(
    registry: &Registry,
    edits: &[StructuralEdit],
    t: f64,
    duration: f64,
) -> f64 {
    let mut current_t = t;
    let mut current_dur = duration;
    for edit in edits {
        if !edit.enabled { continue; }
        if let Some(entry) = registry.get(&edit.processor_id) {
            current_t = (entry.map_time_forward)(current_t, current_dur, &edit.params);
            current_dur = (entry.output_duration)(current_dur, &edit.params);
        }
    }
    current_t
}

/// Map `t` backward through the edit chain. `duration` is the raw audio length in seconds.
pub fn map_time_back(
    registry: &Registry,
    edits: &[StructuralEdit],
    t: f64,
    duration: f64,
) -> f64 {
    let mut durations = Vec::with_capacity(edits.len() + 1);
    durations.push(duration);
    for edit in edits.iter() {
        let last = *durations.last().unwrap();
        let next = if !edit.enabled {
            last
        } else if let Some(entry) = registry.get(&edit.processor_id) {
            (entry.output_duration)(last, &edit.params)
        } else {
            last
        };
        durations.push(next);
    }

    let mut current_t = t;
    for (i, edit) in edits.iter().enumerate().rev() {
        if !edit.enabled { continue; }
        if let Some(entry) = registry.get(&edit.processor_id) {
            current_t = (entry.map_time_back)(current_t, durations[i], &edit.params);
        }
    }
    current_t
}

#[cfg(test)]
mod new_api_tests {
    use super::*;
    use crate::{ProcessorEntry, StructuralProcessor, StreamingProcessorInstance,
                AudioSource, VecAudioSource, secs_to_samples,
                ParameterDescriptor, ProcessorDescriptor, Params};
    use std::collections::HashMap;

    // ── Passthrough processor ────────────────────────────────────────────────
    static PASS_PARAMS: [ParameterDescriptor; 0] = [];
    static PASS_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "pass", name: "Pass", parameters: &PASS_PARAMS };

    struct PassInstance;
    impl StreamingProcessorInstance for PassInstance {
        fn fill(&mut self, out_start: f64, out_end: f64, src: &mut dyn AudioSource) -> Vec<f32> {
            let n = secs_to_samples(out_end - out_start, src.sample_rate(), src.channels());
            src.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }
    struct PassProcessor;
    impl StructuralProcessor for PassProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &PASS_DESC }
        fn validate(_: &Params) -> bool { true }
        fn create(_: Params) -> Box<dyn StreamingProcessorInstance> { Box::new(PassInstance) }
        fn output_duration(d: f64, _: &Params) -> f64 { d }
        fn map_time_forward(t: f64, _: f64, _: &Params) -> f64 { t }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    // ── Half processor: keeps first half ─────────────────────────────────────
    static HALF_PARAMS: [ParameterDescriptor; 0] = [];
    static HALF_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "half", name: "Half", parameters: &HALF_PARAMS };

    struct HalfInstance;
    impl StreamingProcessorInstance for HalfInstance {
        fn fill(&mut self, out_start: f64, out_end: f64, src: &mut dyn AudioSource) -> Vec<f32> {
            let clamped_end = out_end.min(src.duration_secs() / 2.0);
            if clamped_end <= out_start { return vec![]; }
            let n = secs_to_samples(clamped_end - out_start, src.sample_rate(), src.channels());
            src.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }
    struct HalfProcessor;
    impl StructuralProcessor for HalfProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &HALF_DESC }
        fn validate(_: &Params) -> bool { true }
        fn create(_: Params) -> Box<dyn StreamingProcessorInstance> { Box::new(HalfInstance) }
        fn output_duration(d: f64, _: &Params) -> f64 { d / 2.0 }
        fn map_time_forward(t: f64, dur: f64, _: &Params) -> f64 { t.min(dur / 2.0) }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    fn reg() -> Registry {
        let mut m = HashMap::new();
        m.insert("pass".to_string(), ProcessorEntry::of::<PassProcessor>());
        m.insert("half".to_string(), ProcessorEntry::of::<HalfProcessor>());
        m
    }

    fn vec_src(frames: usize) -> Box<dyn AudioSource> {
        Box::new(VecAudioSource::new(
            (0..frames).map(|i| i as f32).collect(),
            100,
            1,
        ))
    }

    fn edits(json: &str) -> Vec<StructuralEdit> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn build_chain_passthrough_returns_same_samples() {
        let es = edits(r#"[{"type":"pass","enabled":true,"parameters":{}}]"#);
        let mut chain = build_chain(vec_src(100), &es, &reg());
        let out = chain.read_at(0.0, 100);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[50] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn build_chain_empty_edits_is_passthrough() {
        let mut chain = build_chain(vec_src(50), &[], &reg());
        let out = chain.read_at(0.0, 50);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn build_chain_disabled_edit_is_skipped() {
        let es = edits(r#"[{"type":"half","enabled":false,"parameters":{}}]"#);
        let chain = build_chain(vec_src(100), &es, &reg());
        assert!((chain.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn build_chain_half_reduces_duration() {
        let es = edits(r#"[{"type":"half","enabled":true,"parameters":{}}]"#);
        let chain = build_chain(vec_src(100), &es, &reg());
        assert!((chain.duration_secs() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn chain_output_duration_two_halves() {
        let es = edits(r#"[
            {"type":"half","enabled":true,"parameters":{}},
            {"type":"half","enabled":true,"parameters":{}}
        ]"#);
        let dur = chain_output_duration(1.0, &es, &reg());
        assert!((dur - 0.25).abs() < 1e-9);
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
    fn map_time_forward_empty_is_identity() {
        assert!((map_time_forward(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_empty_is_identity() {
        assert!((map_time_back(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }
}
