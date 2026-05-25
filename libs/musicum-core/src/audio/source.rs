use std::path::Path;

use anyhow::{Context, Result};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};
use structural_processor_sdk::AudioSource;

const SEEK_THRESHOLD: f64 = 1.0 / 44_100.0;

pub struct FileAudioSource {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
    current_pos_secs: f64,
    leftover: Vec<f32>, // samples decoded from last packet but not yet returned
}

impl FileAudioSource {
    pub fn new(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .context("unsupported format")?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .context("no audio track")?;

        let track_id    = track.id;
        let codec_params = track.codec_params.clone();
        let sample_rate  = codec_params.sample_rate.unwrap_or(44_100);
        let channels     = codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let n_frames     = codec_params.n_frames.unwrap_or(0);
        let duration_secs = n_frames as f64 / sample_rate as f64;

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .context("unsupported codec")?;

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            duration_secs,
            current_pos_secs: 0.0,
            leftover: Vec::new(),
        })
    }

    fn seek_internal(&mut self, secs: f64) {
        let seek_to = SeekTo::Time {
            time: Time::from(secs),
            track_id: Some(self.track_id),
        };
        // SeekMode::Accurate internally resets the decoder after coarse seek,
        // so we do not call decoder.reset() separately.
        let _ = self.format.seek(SeekMode::Accurate, seek_to);
        self.leftover.clear();
        self.current_pos_secs = secs;
    }
}

impl AudioSource for FileAudioSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        if (start_secs - self.current_pos_secs).abs() > SEEK_THRESHOLD {
            self.seek_internal(start_secs);
        }

        let mut result = Vec::with_capacity(num_samples);

        // Drain leftover samples from the previous packet first.
        let take = self.leftover.len().min(num_samples);
        result.extend_from_slice(&self.leftover[..take]);
        self.leftover.drain(..take);

        while result.len() < num_samples {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(_) => break,
            };
            if packet.track_id() != self.track_id { continue; }
            match self.decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let spec = *audio_buf.spec();
                    let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                    sample_buf.copy_interleaved_ref(audio_buf);
                    let samples = sample_buf.samples();
                    let needed = num_samples - result.len();
                    let take = needed.min(samples.len());
                    result.extend_from_slice(&samples[..take]);
                    // Save excess samples for the next read_at call.
                    self.leftover.extend_from_slice(&samples[take..]);
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(_) => break,
            }
        }

        self.current_pos_secs = start_secs
            + result.len() as f64 / (self.sample_rate as f64 * self.channels as f64);
        result
    }

    fn duration_secs(&self) -> f64 { self.duration_secs }
    fn sample_rate(&self) -> u32   { self.sample_rate }
    fn channels(&self) -> u16      { self.channels }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::NamedTempFile;

    fn write_temp_wav(frames: usize, sample_rate: u32) -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut w = WavWriter::create(tmp.path(), spec).unwrap();
        for i in 0..frames {
            w.write_sample(i as f32 / frames as f32).unwrap();
        }
        w.finalize().unwrap();
        tmp
    }

    #[test]
    fn file_source_returns_correct_duration() {
        let tmp = write_temp_wav(4410, 44_100); // 0.1s
        let src = FileAudioSource::new(tmp.path()).unwrap();
        assert!((src.duration_secs() - 0.1).abs() < 0.01);
    }

    #[test]
    fn file_source_sequential_read_returns_samples() {
        let tmp = write_temp_wav(100, 100); // 1s @100Hz mono
        let mut src = FileAudioSource::new(tmp.path()).unwrap();
        let out = src.read_at(0.0, 50);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn file_source_seek_then_read() {
        // Use 44100 frames so the file spans multiple internal packets,
        // making coarse/accurate seek land reliably near the target.
        let tmp = write_temp_wav(44_100, 44_100); // 1s @44100Hz
        let mut src = FileAudioSource::new(tmp.path()).unwrap();
        let _ = src.read_at(0.0, 100);
        let out = src.read_at(0.5, 100);
        assert_eq!(out.len(), 100);
        // values: i/44100, around sample 22050 → ≈ 0.5
        assert!(out[0] > 0.4 && out[0] < 0.6);
    }
}
