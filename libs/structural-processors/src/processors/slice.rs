use structural_processor_sdk::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

static SLICE_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Int { id: "slices", name: "Slices", default: 2, min: 1, max: 64 },
    ParameterDescriptor::Int {
        id: "select_slice",
        name: "Select Slice",
        default: 0,
        min: 0,
        max: 63,
    },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "slice",
    name: "Slice",
    parameters: &SLICE_PARAMS,
};

pub struct SliceProcessor;

impl StructuralProcessor for SliceProcessor {
    fn descriptor() -> &'static ProcessorDescriptor {
        &DESCRIPTOR
    }

    fn validate(params: &Params) -> bool {
        let slices = params.get("slices").copied().unwrap_or(0.0) as i64;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as i64;
        slices >= 1 && select >= 0 && select < slices
    }

    fn apply(samples: &[f32], _sample_rate: u32, channels: u16, params: &Params) -> Vec<f32> {
        let ch = channels as usize;
        let total_frames = samples.len() / ch;

        let slices = (params.get("slices").copied().unwrap_or(1.0) as usize).max(1);
        let select = params.get("select_slice").copied().unwrap_or(0.0) as usize;

        let slice_frames = total_frames / slices;
        let start_frame = select * slice_frames;
        let end_frame = if select + 1 == slices {
            total_frames
        } else {
            start_frame + slice_frames
        };

        if start_frame >= end_frame || end_frame > total_frames {
            return vec![0.0_f32; ch];
        }
        samples[(start_frame * ch)..(end_frame * ch)].to_vec()
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        duration / slices as f64
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as usize;
        let slice_dur = duration / slices as f64;
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

    fn numbered_samples(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32).collect()
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(SliceProcessor::validate(&params(4, 0)));
        assert!(SliceProcessor::validate(&params(4, 3)));
        assert!(SliceProcessor::validate(&params(1, 0)));
    }

    #[test]
    fn validate_rejects_out_of_bounds_select() {
        assert!(!SliceProcessor::validate(&params(4, 4)));
        assert!(!SliceProcessor::validate(&params(4, -1)));
    }

    #[test]
    fn validate_rejects_zero_slices() {
        assert!(!SliceProcessor::validate(&params(0, 0)));
    }

    #[test]
    fn apply_returns_first_slice() {
        // 100 frames @1Hz, 2 slices → slice 0 = frames 0..50
        let samples = numbered_samples(100);
        let result = SliceProcessor::apply(&samples, 1, 1, &params(2, 0));
        assert_eq!(result.len(), 50);
        assert!((result[0] - 0.0).abs() < 1e-6);
        assert!((result[49] - 49.0).abs() < 1e-6);
    }

    #[test]
    fn apply_returns_last_slice_including_remainder() {
        // 101 frames, 2 slices → slice 1 gets frames 50..101 (51 frames)
        let samples = numbered_samples(101);
        let result = SliceProcessor::apply(&samples, 1, 1, &params(2, 1));
        assert_eq!(result.len(), 51);
        assert!((result[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn apply_stereo_slice() {
        // 100 stereo frames (200 samples), 4 slices → slice 2 = frames 50..75 → 25 frames × 2 ch = 50 samples
        let samples: Vec<f32> = (0..200_usize).map(|i| i as f32).collect();
        let result = SliceProcessor::apply(&samples, 1, 2, &params(4, 2));
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn map_time_forward_clamps_into_selected_slice() {
        // 4 slices, select slice 2; duration=1.0s → each slice=0.25s, slice 2 = [0.5, 0.75)
        let p = params(4, 2);
        // t=0.6 is inside slice 2 → processed = 0.6 - 0.5 = 0.1
        assert!((SliceProcessor::map_time_forward(0.6, 1.0, &p) - 0.1).abs() < 1e-9);
        // t=0.0 is before slice 2 → clamps to slice_start → processed = 0.0
        assert!((SliceProcessor::map_time_forward(0.0, 1.0, &p) - 0.0).abs() < 1e-9);
        // t=0.9 is past slice 2 → clamps to slice_end → processed = 0.25
        assert!((SliceProcessor::map_time_forward(0.9, 1.0, &p) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_adds_slice_offset() {
        // slice 2 of 4, duration=1.0s → offset = 0.5s
        let p = params(4, 2);
        assert!((SliceProcessor::map_time_back(0.1, 1.0, &p) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn output_duration_is_slice_fraction() {
        let p = params(4, 0);
        assert!((SliceProcessor::output_duration(1.0, &p) - 0.25).abs() < 1e-9);
    }
}
