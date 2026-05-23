use structural_processor_sdk::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

static TRIM_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "start", name: "Start", default: 0.0 },
    ParameterDescriptor::Time { id: "end", name: "End", default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "trim",
    name: "Trim",
    parameters: &TRIM_PARAMS,
};

pub struct TrimProcessor;

impl StructuralProcessor for TrimProcessor {
    fn descriptor() -> &'static ProcessorDescriptor {
        &DESCRIPTOR
    }

    fn validate(params: &Params) -> bool {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end = params.get("end").copied().unwrap_or(0.0);
        start >= 0.0 && end >= 0.0
    }

    fn apply(samples: &[f32], sample_rate: u32, channels: u16, params: &Params) -> Vec<f32> {
        let ch = channels as usize;
        let rate = sample_rate as f64;
        let total_frames = samples.len() / ch;

        let start = params.get("start").copied().unwrap_or(0.0);
        let end = params.get("end").copied().unwrap_or(0.0);

        let start_frame = ((start * rate) as usize).min(total_frames);
        let end_frame = total_frames.saturating_sub((end * rate) as usize);

        if start_frame >= end_frame {
            return vec![0.0_f32; ch]; // always return at least 1 frame
        }
        samples[(start_frame * ch)..(end_frame * ch)].to_vec()
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end = params.get("end").copied().unwrap_or(0.0);
        (duration - start.min(duration) - end.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        t + start
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end = params.get("end").copied().unwrap_or(0.0);
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

    fn sine_samples(frames: usize, ch: usize) -> Vec<f32> {
        (0..frames * ch).map(|i| (i as f32) * 0.001).collect()
    }

    #[test]
    fn validate_accepts_valid_params() {
        assert!(TrimProcessor::validate(&params(0.5, 1.5)));
        assert!(TrimProcessor::validate(&params(0.0, 0.0)));
    }

    #[test]
    fn validate_rejects_negative_start() {
        let p = params(-0.1, 1.0);
        assert!(!TrimProcessor::validate(&p));
    }

    #[test]
    fn apply_clips_to_range() {
        // 2-second mono audio at 100 Hz → 200 frames
        let samples = sine_samples(200, 1);
        let p = params(0.5, 0.5); // cut 0.5s from start, 0.5s from end → keep frames 50..150
        let result = TrimProcessor::apply(&samples, 100, 1, &p);
        assert_eq!(result.len(), 100); // 100 frames × 1 ch
    }

    #[test]
    fn apply_stereo_keeps_interleaving() {
        // 100 frames stereo at 100 Hz = 1s
        let samples = sine_samples(100, 2);
        let p = params(0.0, 0.5); // cut 0.5s from end → keep first 50 frames → 100 samples
        let result = TrimProcessor::apply(&samples, 100, 2, &p);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn apply_no_trim_returns_full_audio() {
        let samples = sine_samples(100, 1);
        let p = params(0.0, 0.0); // no trimming
        let result = TrimProcessor::apply(&samples, 100, 1, &p);
        assert_eq!(result.len(), 100);
    }

    #[test]
    fn apply_returns_one_frame_when_range_empty() {
        let samples = sine_samples(100, 1);
        let p = params(0.5, 0.5); // cut 0.5s from each side of a 1s clip → nothing left
        let result = TrimProcessor::apply(&samples, 100, 1, &p);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn map_time_back_adds_start() {
        let p = params(1.0, 2.0);
        assert!((TrimProcessor::map_time_back(0.5, 10.0, &p) - 1.5).abs() < 1e-9);
        assert!((TrimProcessor::map_time_back(0.0, 10.0, &p) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_and_shifts() {
        // 10s track, cut 1s from start, 3s from end → effective range [1.0, 7.0]
        let p = params(1.0, 3.0);
        // source time 1.5 → in range → 0.5 in processed
        assert!((TrimProcessor::map_time_forward(1.5, 10.0, &p) - 0.5).abs() < 1e-9);
        // source time 0.0 → below start → clamps to 1.0 → 0.0 in processed
        assert!((TrimProcessor::map_time_forward(0.0, 10.0, &p) - 0.0).abs() < 1e-9);
        // source time 8.0 → beyond effective end (7.0) → clamps to 7.0 → 6.0 in processed
        assert!((TrimProcessor::map_time_forward(8.0, 10.0, &p) - 6.0).abs() < 1e-9);
    }
}
