use structural_processor_sdk::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

static CUT_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to", name: "To", default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "cut",
    name: "Cut",
    parameters: &CUT_PARAMS,
};

pub struct CutProcessor;

impl StructuralProcessor for CutProcessor {
    fn descriptor() -> &'static ProcessorDescriptor {
        &DESCRIPTOR
    }

    fn validate(params: &Params) -> bool {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(0.0);
        from >= 0.0 && to > from
    }

    fn apply(samples: &[f32], sample_rate: u32, channels: u16, params: &Params) -> Vec<f32> {
        let ch = channels as usize;
        let rate = sample_rate as f64;
        let total_frames = samples.len() / ch;

        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(0.0);

        let from_frame = ((from * rate) as usize).min(total_frames);
        let to_frame = ((to * rate) as usize).min(total_frames);

        let mut result = samples[..(from_frame * ch)].to_vec();
        result.extend_from_slice(&samples[(to_frame * ch)..]);
        if result.is_empty() {
            result = vec![0.0_f32; ch];
        }
        result
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(0.0);
        (duration - (to - from).clamp(0.0, duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(0.0);
        if t >= from { t + (to - from) } else { t }
    }

    fn map_time_forward(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(0.0);
        if t < from {
            t
        } else if t < to {
            from
        } else {
            t - (to - from)
        }
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

    fn numbered_samples(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32).collect()
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(CutProcessor::validate(&params(0.5, 1.5)));
    }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CutProcessor::validate(&params(1.0, 0.5)));
        assert!(!CutProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn apply_removes_range_and_concatenates() {
        // 200 frames mono @100Hz; cut [0.5s, 1.0s] → remove frames 50..100
        let samples = numbered_samples(200);
        let result = CutProcessor::apply(&samples, 100, 1, &params(0.5, 1.0));
        assert_eq!(result.len(), 150);
        // First 50 frames unchanged
        for i in 0..50 {
            assert!((result[i] - samples[i]).abs() < 1e-6, "frame {i}");
        }
        // Frames after cut start at sample 100
        for i in 50..150 {
            assert!((result[i] - samples[100 + (i - 50)]).abs() < 1e-6, "frame {i}");
        }
    }

    #[test]
    fn apply_returns_one_frame_when_result_empty() {
        let samples = numbered_samples(10);
        // Cut the entire signal
        let result = CutProcessor::apply(&samples, 1, 1, &params(0.0, 10.0));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn map_time_back_adds_gap_for_times_at_or_after_from() {
        let p = params(1.0, 2.0); // gap = 1.0s
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
