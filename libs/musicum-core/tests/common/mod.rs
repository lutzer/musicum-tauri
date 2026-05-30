use hound::{SampleFormat, WavSpec, WavWriter};
use std::f32::consts::PI;
use std::path::{Path, PathBuf};

/// Write a mono 440 Hz sine wave WAV at `path` (16-bit PCM, 44100 Hz).
/// Duration in seconds. Returns the path for chaining.
pub fn write_sine_wav(path: &Path, duration_secs: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let num_samples = (spec.sample_rate as f32 * duration_secs) as u32;
    for i in 0..num_samples {
        let t = i as f32 / spec.sample_rate as f32;
        let sample = (2.0 * PI * 440.0 * t).sin();
        let pcm = (sample * i16::MAX as f32) as i16;
        writer.write_sample(pcm).unwrap();
    }
    writer.finalize().unwrap();
    path.to_path_buf()
}

pub fn make_paths(base: &std::path::Path) -> musicum_core::config::LibraryConfig {
    let files_dir     = base.join("files");
    let catalog_dir   = base.join("catalog");
    let generated_dir = base.join(".generated");
    std::fs::create_dir_all(&files_dir).unwrap();
    std::fs::create_dir_all(&catalog_dir).unwrap();
    std::fs::create_dir_all(&generated_dir).unwrap();
    musicum_core::config::LibraryConfig { files_dir, catalog_dir, generated_dir }
}

/// Write a stereo WAV with white noise at `path` (16-bit PCM, 48000 Hz).
pub fn write_stereo_wav(path: &Path, duration_secs: f32) -> PathBuf {
    let spec = WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let num_samples = (spec.sample_rate as f32 * duration_secs) as u32;
    let mut rng: u32 = 0xdeadbeef;
    for _ in 0..num_samples {
        for _ in 0..2 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let pcm = ((rng as i32) >> 16) as i16;
            writer.write_sample(pcm).unwrap();
        }
    }
    writer.finalize().unwrap();
    path.to_path_buf()
}
