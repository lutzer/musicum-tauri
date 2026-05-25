use std::collections::HashMap;

use serde::Serialize;

use crate::source::AudioSource;

pub type Params = HashMap<String, f64>;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParameterDescriptor {
    Time { id: &'static str, name: &'static str, default: f64 },
    Int { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

#[derive(Serialize)]
pub struct ProcessorDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static [ParameterDescriptor],
}

impl ProcessorDescriptor {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("descriptor serialisation failed")
    }
}

/// Stateful, streaming processor instance. Created per playback session with
/// params baked in. `fill` is called repeatedly as the ring buffer needs data.
pub trait StreamingProcessorInstance {
    /// Produce interleaved f32 samples for output time `[out_start, out_end)`.
    /// May call `source.read_at` one or more times.
    fn fill(
        &mut self,
        out_start: f64,
        out_end: f64,
        source: &mut dyn AudioSource,
    ) -> Vec<f32>;

    /// Reset any internal state (e.g. overlap buffers). Chain rebuild on seek
    /// is preferred over calling this, but `reset` is provided for completeness.
    fn reset(&mut self);
}

/// Implemented by every structural processor type. Holds static/pure methods
/// used to populate a `ProcessorEntry` via `ProcessorEntry::of::<P>()`.
pub trait StructuralProcessor {
    fn descriptor() -> &'static ProcessorDescriptor;
    fn validate(params: &Params) -> bool;
    /// Construct a streaming instance with `params` baked in.
    fn create(params: Params) -> Box<dyn StreamingProcessorInstance>;
    fn output_duration(duration: f64, params: &Params) -> f64;
    fn map_time_back(t: f64, duration: f64, params: &Params) -> f64;
    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64;
}

#[cfg(test)]
mod trait_shape_tests {
    use super::*;
    use crate::source::VecAudioSource;

    struct Passthrough;
    impl StreamingProcessorInstance for Passthrough {
        fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
            let n = crate::source::secs_to_samples(out_end - out_start, source.sample_rate(), source.channels());
            source.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn passthrough_fill_returns_correct_count() {
        let mut src = VecAudioSource::new((0..100).map(|i| i as f32).collect(), 100, 1);
        let mut proc = Passthrough;
        let out = proc.fill(0.0, 0.5, &mut src); // 0..0.5s @100Hz = 50 samples
        assert_eq!(out.len(), 50);
    }
}
