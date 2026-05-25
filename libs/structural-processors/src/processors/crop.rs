use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static CROP_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to",   name: "To",   default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "crop",
    name: "Crop",
    parameters: &CROP_PARAMS,
};

pub struct CropInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for CropInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = CropProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = CropProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct CropProcessor;

impl StructuralProcessor for CropProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        from >= 0.0 && to > from
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(CropInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(duration);
        (to.min(duration) - from.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        t + from
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(duration);
        t.max(from).min(to) - from
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn params(from: f64, to: f64) -> Params {
        let mut m = HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    #[test]
    fn validate_accepts_valid_params() { assert!(CropProcessor::validate(&params(0.5, 1.5))); }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CropProcessor::validate(&params(1.0, 0.5)));
        assert!(!CropProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn map_time_back_adds_from() {
        let p = params(2.0, 5.0);
        assert!((CropProcessor::map_time_back(1.0, 10.0, &p) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_and_shifts() {
        let p = params(2.0, 5.0);
        assert!((CropProcessor::map_time_forward(2.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((CropProcessor::map_time_forward(1.0, 10.0, &p) - 0.0).abs() < 1e-9);
        assert!((CropProcessor::map_time_forward(6.0, 10.0, &p) - 3.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::VecAudioSource;

    fn params(from: f64, to: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_returns_correct_range() {
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_partial_read_in_range() {
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 70.0).abs() < 1e-6);
    }
}
