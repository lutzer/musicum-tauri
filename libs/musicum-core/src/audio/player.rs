use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use structural_processor_sdk::{
    chain::{build_chain, chain_output_duration, Edit},
    AudioSource,
};

use super::source::FileAudioSource;

// ~2 seconds of stereo audio at 48 kHz
const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
const CHUNK_SAMPLES: usize = 4_096;

struct PlaybackState {
    paused:       AtomicBool,
    finished:     AtomicBool,
    seek_request: Mutex<Option<f64>>,
    position:     AtomicU64, // frames output so far
    total_frames: AtomicU64,
    sample_rate:  u32,
    buffer:       Mutex<VecDeque<f32>>,
}

pub struct PlaybackEngine {
    state:          Arc<PlaybackState>,
    title:          String,
    _stream:        cpal::Stream,
    _decode_thread: JoinHandle<()>,
}

impl PlaybackEngine {
    /// Construct a new playback engine for `path`, applying `edits` via
    /// `structural_processors::registry()`. Pass `edits: &[]` for raw file playback.
    pub fn new(path: &Path, edits: &[Edit]) -> Result<Self> {
        let source = FileAudioSource::new(path)?;
        let raw_duration  = source.duration_secs();
        let sample_rate   = source.sample_rate();
        let channels      = source.channels();

        let registry = structural_processors::registry();
        let output_duration = chain_output_duration(raw_duration, edits, &registry);
        let total_frames    = (output_duration * sample_rate as f64) as u64;

        let host   = cpal::default_host();
        let device = host.default_output_device()
            .ok_or_else(|| anyhow!("no audio output device"))?;
        let config = cpal::StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let state = Arc::new(PlaybackState {
            paused:       AtomicBool::new(true),
            finished:     AtomicBool::new(false),
            seek_request: Mutex::new(None),
            position:     AtomicU64::new(0),
            total_frames: AtomicU64::new(total_frames),
            sample_rate,
            buffer:       Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
        });

        let state_cb = Arc::clone(&state);
        let ch = channels as usize;
        let stream = device.build_output_stream(
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
                    state_cb.position.fetch_add((n / ch.max(1)) as u64, Ordering::Relaxed);
                    output[n..].fill(0.0);
                } else {
                    output.fill(0.0);
                }
            },
            |err| eprintln!("audio error: {err}"),
            None,
        ).context("failed to open audio stream")?;
        stream.play().context("failed to start audio stream")?;

        let state_dec   = Arc::clone(&state);
        let path_owned  = path.to_path_buf();
        let edits_owned = edits.to_vec();

        let decode_thread = thread::spawn(move || {
            decode_loop(path_owned, edits_owned, state_dec);
        });

        let title = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        Ok(Self { state, title, _stream: stream, _decode_thread: decode_thread })
    }

    pub fn play(&self)         { self.state.paused.store(false, Ordering::Relaxed); }
    pub fn pause(&self)        { self.state.paused.store(true,  Ordering::Relaxed); }
    pub fn toggle_pause(&self) {
        let was = self.state.paused.load(Ordering::Relaxed);
        self.state.paused.store(!was, Ordering::Relaxed);
    }
    pub fn seek(&self, secs: f64) {
        let clamped = secs.clamp(0.0, self.duration_secs());
        if let Ok(mut req) = self.state.seek_request.lock() { *req = Some(clamped); }
    }
    pub fn position_secs(&self) -> f64 {
        self.state.position.load(Ordering::Relaxed) as f64 / self.state.sample_rate as f64
    }
    pub fn duration_secs(&self) -> f64 {
        let frames = self.state.total_frames.load(Ordering::Relaxed);
        if frames == 0 { return 0.0; }
        frames as f64 / self.state.sample_rate as f64
    }
    pub fn is_paused(&self)   -> bool { self.state.paused.load(Ordering::Relaxed) }
    pub fn is_finished(&self) -> bool { self.state.finished.load(Ordering::Relaxed) }
    pub fn title(&self)       -> &str { &self.title }
}

fn build_fresh_chain(path: &Path, edits: &[Edit]) -> Result<Box<dyn AudioSource>> {
    let registry = structural_processors::registry();
    let source   = Box::new(FileAudioSource::new(path)?);
    Ok(build_chain(source, edits, &registry))
}

fn decode_loop(path: PathBuf, edits: Vec<Edit>, state: Arc<PlaybackState>) {
    let mut chain = match build_fresh_chain(&path, &edits) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("decode init error: {e}");
            state.finished.store(true, Ordering::Relaxed);
            return;
        }
    };

    let sample_rate = state.sample_rate;
    let ch = {
        let probe = FileAudioSource::new(&path);
        probe.map(|s| s.channels()).unwrap_or(2)
    };
    let mut cursor_secs = 0.0_f64;
    let total_secs = state.total_frames.load(Ordering::Relaxed) as f64 / sample_rate as f64;

    loop {
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target) = req.take() {
                match build_fresh_chain(&path, &edits) {
                    Ok(c) => {
                        chain = c;
                        cursor_secs = target;
                        let frame_pos = (target * sample_rate as f64) as u64;
                        state.position.store(frame_pos, Ordering::Relaxed);
                        if let Ok(mut buf) = state.buffer.lock() { buf.clear(); }
                    }
                    Err(e) => eprintln!("seek chain rebuild error: {e}"),
                }
            }
        }

        if state.paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        if cursor_secs >= total_secs {
            state.finished.store(true, Ordering::Relaxed);
            break;
        }

        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        let samples = chain.read_at(cursor_secs, CHUNK_SAMPLES);
        if samples.is_empty() {
            state.finished.store(true, Ordering::Relaxed);
            break;
        }

        cursor_secs += samples.len() as f64 / (sample_rate as f64 * ch as f64);
        if let Ok(mut buf) = state.buffer.lock() {
            buf.extend(&samples);
        }
    }
}
