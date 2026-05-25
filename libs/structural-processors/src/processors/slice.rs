use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static SLICE_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Int { id: "slices",       name: "Slices",       default: 2,  min: 1, max: 64 },
    ParameterDescriptor::Int { id: "select_slice", name: "Select Slice", default: 0,  min: 0, max: 63 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "slice",
    name: "Slice",
    parameters: &SLICE_PARAMS,
};

pub struct SliceInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for SliceInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = SliceProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = SliceProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct SliceProcessor;

impl StructuralProcessor for SliceProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let slices = params.get("slices").copied().unwrap_or(0.0) as i64;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as i64;
        slices >= 1 && select >= 0 && select < slices
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(SliceInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        duration / slices as f64
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as usize;
        let slice_dur   = duration / slices as f64;
        let slice_start = select as f64 * slice_dur;
        t.clamp(slice_start, slice_start + slice_dur) - slice_start
    }

    fn map_time_back(t: f64, duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as usize;
        let slice_dur = duration / slices as f64;
        t + select as f64 * slice_dur
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn params(slices: i64, select: i64) -> Params {
        let mut m = HashMap::new();
        m.insert("slices".into(), slices as f64);
        m.insert("select_slice".into(), select as f64);
        m
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(SliceProcessor::validate(&params(4, 0)));
        assert!(SliceProcessor::validate(&params(4, 3)));
    }

    #[test]
    fn validate_rejects_out_of_bounds_select() {
        assert!(!SliceProcessor::validate(&params(4, 4)));
    }

    #[test]
    fn validate_rejects_zero_slices() {
        assert!(!SliceProcessor::validate(&params(0, 0)));
    }

    #[test]
    fn output_duration_is_slice_fraction() {
        assert!((SliceProcessor::output_duration(1.0, &params(4, 0)) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_into_selected_slice() {
        let p = params(4, 2);
        assert!((SliceProcessor::map_time_forward(0.6, 1.0, &p) - 0.1).abs() < 1e-9);
        assert!((SliceProcessor::map_time_forward(0.0, 1.0, &p) - 0.0).abs() < 1e-9);
        assert!((SliceProcessor::map_time_forward(0.9, 1.0, &p) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_adds_slice_offset() {
        let p = params(4, 2);
        assert!((SliceProcessor::map_time_back(0.1, 1.0, &p) - 0.6).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::VecAudioSource;

    fn params(slices: i64, select: i64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("slices".into(), slices as f64);
        m.insert("select_slice".into(), select as f64);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_selects_correct_slice() {
        let mut inst = SliceInstance { params: params(4, 2) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.25, &mut src);
        assert_eq!(out.len(), 25);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_first_slice_reads_from_zero() {
        let mut inst = SliceInstance { params: params(2, 0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }
}
