use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static CUT_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to",   name: "To",   default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "cut",
    name: "Cut",
    parameters: &CUT_PARAMS,
};

pub struct CutInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for CutInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let from = self.params.get("from").copied().unwrap_or(0.0);
        let to   = self.params.get("to").copied().unwrap_or(0.0);

        if out_end <= from || out_start >= from {
            // No cut boundary crossed: simple mapped read
            let src_start = CutProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
            let n = secs_to_samples(out_end - out_start, source.sample_rate(), source.channels());
            return source.read_at(src_start, n);
        }

        // Range spans the cut: read before-cut portion then after-cut portion
        let part1_n = secs_to_samples(from - out_start, source.sample_rate(), source.channels());
        let mut result = source.read_at(out_start, part1_n);

        let part2_n = secs_to_samples(out_end - from, source.sample_rate(), source.channels());
        result.extend(source.read_at(to, part2_n));
        result
    }
    fn reset(&mut self) {}
}

pub struct CutProcessor;

impl StructuralProcessor for CutProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        from >= 0.0 && to > from
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(CutInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        (duration - (to - from).clamp(0.0, duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        if t >= from { t + (to - from) } else { t }
    }

    fn map_time_forward(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        if t < from      { t }
        else if t < to   { from }
        else             { t - (to - from) }
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
    fn validate_accepts_valid_params() { assert!(CutProcessor::validate(&params(0.5, 1.5))); }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CutProcessor::validate(&params(1.0, 0.5)));
        assert!(!CutProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn map_time_back_adds_gap_for_times_at_or_after_from() {
        let p = params(1.0, 2.0);
        assert!((CutProcessor::map_time_back(1.0, 10.0, &p) - 2.0).abs() < 1e-9);
        assert!((CutProcessor::map_time_back(0.5, 10.0, &p) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_snaps_cut_region_to_boundary() {
        let p = params(1.0, 2.0);
        assert!((CutProcessor::map_time_forward(0.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((CutProcessor::map_time_forward(1.5, 10.0, &p) - 1.0).abs() < 1e-9);
        assert!((CutProcessor::map_time_forward(2.5, 10.0, &p) - 1.5).abs() < 1e-9);
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
    fn fill_entirely_before_cut() {
        let mut inst = CutInstance { params: params(0.5, 1.0) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 0.3, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn fill_entirely_after_cut() {
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.3, 0.7, &mut src);
        assert_eq!(out.len(), 40);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_spanning_cut_concatenates() {
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0]  - 20.0).abs() < 1e-6);
        assert!((out[10] - 50.0).abs() < 1e-6);
    }
}
