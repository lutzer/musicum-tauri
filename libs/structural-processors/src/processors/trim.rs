use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static TRIM_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "start", name: "Start", default: 0.0 },
    ParameterDescriptor::Time { id: "end", name: "End", default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "trim",
    name: "Trim",
    parameters: &TRIM_PARAMS,
};

pub struct TrimInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for TrimInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = TrimProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = TrimProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct TrimProcessor;

impl StructuralProcessor for TrimProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
        start >= 0.0 && end >= 0.0
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(TrimInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
        (duration - start.min(duration) - end.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        t + start
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
        let effective_end = duration - end;
        t.max(start).min(effective_end) - start
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn params(start: f64, end: f64) -> Params {
        let mut m = HashMap::new();
        m.insert("start".into(), start);
        m.insert("end".into(), end);
        m
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(TrimProcessor::validate(&params(0.5, 1.5)));
        assert!(TrimProcessor::validate(&params(0.0, 0.0)));
    }

    #[test]
    fn validate_rejects_negative_start() {
        assert!(!TrimProcessor::validate(&params(-0.1, 1.0)));
    }

    #[test]
    fn output_duration_basic() {
        assert!((TrimProcessor::output_duration(1.0, &params(0.2, 0.2)) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_adds_start() {
        let p = params(1.0, 2.0);
        assert!((TrimProcessor::map_time_back(0.5, 10.0, &p) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_and_shifts() {
        let p = params(1.0, 3.0);
        assert!((TrimProcessor::map_time_forward(1.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((TrimProcessor::map_time_forward(0.0, 10.0, &p) - 0.0).abs() < 1e-9);
        assert!((TrimProcessor::map_time_forward(8.0, 10.0, &p) - 6.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::VecAudioSource;

    fn params(start: f64, end: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("start".into(), start);
        m.insert("end".into(), end);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_no_trim_is_passthrough() {
        let mut inst = TrimInstance { params: params(0.0, 0.0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn fill_with_start_trim_shifts_source() {
        let mut inst = TrimInstance { params: params(0.5, 0.0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_reads_correct_sub_range() {
        let mut inst = TrimInstance { params: params(0.2, 0.2) };
        let mut src = mono_src(100);
        let out = inst.fill(0.1, 0.4, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 30.0).abs() < 1e-6);
    }
}
