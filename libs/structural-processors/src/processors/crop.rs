use structural_processor_sdk::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

static CROP_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to", name: "To", default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "crop",
    name: "Crop",
    parameters: &CROP_PARAMS,
};

pub struct CropProcessor;

impl StructuralProcessor for CropProcessor {
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
        let to = params
            .get("to")
            .copied()
            .unwrap_or(total_frames as f64 / rate);

        let from_frame = ((from * rate) as usize).min(total_frames);
        let to_frame = ((to * rate) as usize).min(total_frames);

        if from_frame >= to_frame {
            return vec![0.0_f32; ch];
        }
        samples[(from_frame * ch)..(to_frame * ch)].to_vec()
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(duration);
        (to.min(duration) - from.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        t + from
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to = params.get("to").copied().unwrap_or(duration);
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

    fn numbered_samples(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32).collect()
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(CropProcessor::validate(&params(0.5, 1.5)));
    }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CropProcessor::validate(&params(1.0, 0.5)));
        assert!(!CropProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn apply_returns_correct_range() {
        // 200 frames @100Hz; crop [0.5s, 1.5s] → frames 50..150 = 100 frames
        let samples = numbered_samples(200);
        let result = CropProcessor::apply(&samples, 100, 1, &params(0.5, 1.5));
        assert_eq!(result.len(), 100);
        assert!((result[0] - 50.0).abs() < 1e-6);
        assert!((result[99] - 149.0).abs() < 1e-6);
    }

    #[test]
    fn apply_clamps_to_boundary() {
        let samples = numbered_samples(100);
        let result = CropProcessor::apply(&samples, 1, 1, &params(0.0, 999.0));
        assert_eq!(result.len(), 100);
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
        assert!((CropProcessor::map_time_forward(1.0, 10.0, &p) - 0.0).abs() < 1e-9); // below from
        assert!((CropProcessor::map_time_forward(6.0, 10.0, &p) - 3.0).abs() < 1e-9); // above to
    }
}
