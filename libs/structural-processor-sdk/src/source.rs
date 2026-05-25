/// Number of interleaved f32 samples that span `secs` of audio.
pub fn secs_to_samples(secs: f64, sample_rate: u32, channels: u16) -> usize {
    (secs * sample_rate as f64 * channels as f64).round() as usize
}

/// Seekable, streaming audio source.
pub trait AudioSource {
    /// Read `num_samples` interleaved f32 samples starting at `start_secs`.
    /// Implementations seek internally when `start_secs` differs from current position.
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32>;
    fn duration_secs(&self) -> f64;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
}

/// Simple in-memory `AudioSource` backed by a `Vec<f32>`.
/// Used in tests and the CLI harness.
pub struct VecAudioSource {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

impl VecAudioSource {
    pub fn new(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Self {
        Self { samples, sample_rate, channels }
    }
}

impl AudioSource for VecAudioSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        let start = secs_to_samples(start_secs, self.sample_rate, self.channels)
            .min(self.samples.len());
        let end = (start + num_samples).min(self.samples.len());
        self.samples[start..end].to_vec()
    }
    fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / (self.sample_rate as f64 * self.channels as f64)
    }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn channels(&self) -> u16 { self.channels }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_source(frames: usize) -> VecAudioSource {
        VecAudioSource::new(
            (0..frames).map(|i| i as f32).collect(),
            100,
            1,
        )
    }

    #[test]
    fn read_at_start_returns_first_samples() {
        let mut src = mono_source(100);
        let got = src.read_at(0.0, 10);
        assert_eq!(got.len(), 10);
        assert!((got[0] - 0.0).abs() < 1e-6);
        assert!((got[9] - 9.0).abs() < 1e-6);
    }

    #[test]
    fn read_at_mid_seeks_correctly() {
        let mut src = mono_source(100);
        // start_secs=0.5s, @100Hz mono → frame 50
        let got = src.read_at(0.5, 5);
        assert_eq!(got.len(), 5);
        assert!((got[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn read_at_past_end_returns_fewer_samples() {
        let mut src = mono_source(10);
        let got = src.read_at(0.09, 100); // only 1 sample left
        assert!(got.len() <= 1);
    }

    #[test]
    fn duration_secs_correct() {
        let src = mono_source(100); // 100 frames @100Hz = 1.0s
        assert!((src.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn secs_to_samples_stereo() {
        // 1.0s stereo @44100 = 88200 samples
        assert_eq!(secs_to_samples(1.0, 44_100, 2), 88_200);
    }
}
