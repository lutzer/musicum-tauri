use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

// ~2 seconds of stereo audio at 48 kHz
const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;

struct PlaybackState {
    paused: AtomicBool,
    finished: AtomicBool,
    seek_request: Mutex<Option<f64>>,
    position: AtomicU64, // frames output
    total_frames: AtomicU64,
    sample_rate: u32,
    buffer: Mutex<VecDeque<f32>>,
}

pub struct PlaybackEngine {
    state: Arc<PlaybackState>,
    title: String,
    _stream: cpal::Stream,
    _decode_thread: JoinHandle<()>,
}

impl PlaybackEngine {
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
            .context("unsupported audio format")?;

        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no audio track found"))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let sample_rate = codec_params.sample_rate.unwrap_or(44_100);
        let channels = codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let total_frames = codec_params.n_frames.unwrap_or(0);

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .context("unsupported codec")?;

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;

        let config = cpal::StreamConfig {
            channels,
            sample_rate, // SampleRate = u32
            buffer_size: cpal::BufferSize::Default,
        };

        let state = Arc::new(PlaybackState {
            paused: AtomicBool::new(true),
            finished: AtomicBool::new(false),
            seek_request: Mutex::new(None),
            position: AtomicU64::new(0),
            total_frames: AtomicU64::new(total_frames),
            sample_rate,
            buffer: Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
        });

        let state_cb = Arc::clone(&state);
        let ch = channels as usize;
        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if state_cb.paused.load(Ordering::Relaxed) {
                        output.fill(0.0);
                        return;
                    }
                    if let Ok(mut buf) = state_cb.buffer.try_lock() {
                        let n = output.len().min(buf.len());
                        for (out, s) in output[..n].iter_mut().zip(buf.drain(..n)) {
                            *out = s;
                        }
                        state_cb
                            .position
                            .fetch_add((n / ch.max(1)) as u64, Ordering::Relaxed);
                        for out in output[n..].iter_mut() {
                            *out = 0.0;
                        }
                    } else {
                        for out in output.iter_mut() {
                            *out = 0.0;
                        }
                    }
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .context("failed to open audio stream")?;

        stream.play().context("failed to start audio stream")?;

        let state_dec = Arc::clone(&state);
        let decode_thread = thread::spawn(move || {
            decode_loop(format, decoder, track_id, state_dec);
        });

        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(Self {
            state,
            title,
            _stream: stream,
            _decode_thread: decode_thread,
        })
    }

    pub fn play(&self) {
        self.state.paused.store(false, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        self.state.paused.store(true, Ordering::Relaxed);
    }

    pub fn toggle_pause(&self) {
        let was = self.state.paused.load(Ordering::Relaxed);
        self.state.paused.store(!was, Ordering::Relaxed);
    }

    pub fn seek(&self, secs: f64) {
        let clamped = secs.clamp(0.0, self.duration_secs().max(secs));
        if let Ok(mut req) = self.state.seek_request.lock() {
            *req = Some(clamped);
        }
    }

    pub fn position_secs(&self) -> f64 {
        self.state.position.load(Ordering::Relaxed) as f64 / self.state.sample_rate as f64
    }

    pub fn duration_secs(&self) -> f64 {
        let frames = self.state.total_frames.load(Ordering::Relaxed);
        if frames == 0 {
            return 0.0;
        }
        frames as f64 / self.state.sample_rate as f64
    }

    pub fn is_paused(&self) -> bool {
        self.state.paused.load(Ordering::Relaxed)
    }

    pub fn is_finished(&self) -> bool {
        self.state.finished.load(Ordering::Relaxed)
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

fn decode_loop(
    mut format: Box<dyn symphonia::core::formats::FormatReader>,
    mut decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    state: Arc<PlaybackState>,
) {
    loop {
        // Handle pending seek
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target_secs) = req.take() {
                let seek_to = SeekTo::Time {
                    time: Time::from(target_secs),
                    track_id: Some(track_id),
                };
                if format.seek(SeekMode::Coarse, seek_to).is_ok() {
                    if let Ok(mut buf) = state.buffer.lock() {
                        buf.clear();
                    }
                    let frame_pos = (target_secs * state.sample_rate as f64) as u64;
                    state.position.store(frame_pos, Ordering::Relaxed);
                    decoder.reset();
                }
            }
        }

        if state.paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        // Back-pressure: keep buffer at most BUFFER_CAPACITY samples
        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                state.finished.store(true, Ordering::Relaxed);
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(_) => {
                state.finished.store(true, Ordering::Relaxed);
                break;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(audio_buf);
                if let Ok(mut buf) = state.buffer.lock() {
                    buf.extend(sample_buf.samples());
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => {
                state.finished.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}
