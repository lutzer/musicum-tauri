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
use audio_plugin_sdk::PluginProcessor;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use structural_processor_sdk::{
    chain::{build_chain, chain_output_duration, StructuralEdit},
    AudioSource,
};
use uuid::Uuid;

use crate::audio::registry::EditRegistry;
use crate::audio::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};

// ~2 seconds of stereo audio at 48 kHz
const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
const CHUNK_SAMPLES: usize = 4_096;

// ── Plugin handle ─────────────────────────────────────────────────────────────

/// Shared handle for a single plugin instance.
/// Owned by `PlaybackEngine` and also cloned into the decode thread.
struct PluginHandle {
    uuid:      Uuid,
    enabled:   AtomicBool,
    processor: Mutex<Box<dyn PluginProcessor>>,
}

// ── Playback state ────────────────────────────────────────────────────────────

struct PlaybackState {
    paused:       AtomicBool,
    looping:      AtomicBool,
    finished:     AtomicBool,
    seek_request: Mutex<Option<f64>>,
    position:     AtomicU64,
    total_frames: AtomicU64,
    sample_rate:  u32,
    buffer:       Mutex<VecDeque<f32>>,
}

// ── PlaybackEngine ────────────────────────────────────────────────────────────

pub struct PlaybackEngine {
    state:               Arc<PlaybackState>,
    plugin_handles:      Vec<Arc<PluginHandle>>,
    /// Snapshot of structural edits; updated by `set_edit_param` / `set_edit_enabled`
    /// for structural UUIDs. Changes take effect on the next `PlaybackEngine::new`.
    structural_snapshot: Mutex<Vec<ProcessorEdit>>,
    title:               String,
    _stream:             cpal::Stream,
    _decode_thread:      JoinHandle<()>,
}

impl PlaybackEngine {
    /// Create a new playback engine for `path`, applying `edits` via `registry`.
    ///
    /// `edits` is the full edit list (structural + plugin). Structural edits build
    /// the decode chain once; plugin edits are instantiated and applied live per chunk.
    /// Pass `edits: &[]` for raw file playback.
    pub fn new(path: &Path, edits: &[ProcessorEdit], registry: &EditRegistry) -> Result<Self> {
        let source = FileAudioSource::new(path)?;
        let raw_duration = source.duration_secs();
        let sample_rate  = source.sample_rate();
        let channels     = source.channels();

        // Split edits by kind
        let structural_edits: Vec<StructuralEdit> = edits
            .iter()
            .filter_map(|e| {
                if let EditKind::Structural { processor_id, params } = &e.kind {
                    Some(StructuralEdit {
                        processor_id: processor_id.clone(),
                        enabled: e.enabled,
                        params: params.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let output_duration = chain_output_duration(raw_duration, &structural_edits, &registry.structural);
        let total_frames    = (output_duration * sample_rate as f64) as u64;

        // Build plugin handles (only enabled plugins get an instance)
        let mut plugin_handles: Vec<Arc<PluginHandle>> = Vec::new();
        for edit in edits {
            if let EditKind::Plugin { plugin_id, params } = &edit.kind {
                if let Some(entry) = registry.plugins.get(plugin_id) {
                    let mut instance = (entry.create)();
                    for (id, &val) in params {
                        instance.set_parameter(id, val);
                    }
                    plugin_handles.push(Arc::new(PluginHandle {
                        uuid:      edit.uuid,
                        enabled:   AtomicBool::new(edit.enabled),
                        processor: Mutex::new(instance),
                    }));
                } else {
                    eprintln!("warning: unknown plugin '{plugin_id}' — skipped");
                }
            }
        }

        // Structural snapshot for set_edit_param
        let structural_snapshot: Vec<ProcessorEdit> = edits
            .iter()
            .filter(|e| matches!(e.kind, EditKind::Structural { .. }))
            .cloned()
            .collect();

        // Audio device setup
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
            looping:      AtomicBool::new(false),
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

        let state_dec          = Arc::clone(&state);
        let path_owned         = path.to_path_buf();
        let struct_owned       = structural_edits;
        let plugin_handles_dec = plugin_handles.clone(); // Arc clones, cheap

        let decode_thread = thread::spawn(move || {
            decode_loop(path_owned, struct_owned, plugin_handles_dec, state_dec);
        });

        let title = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        Ok(Self {
            state,
            plugin_handles,
            structural_snapshot: Mutex::new(structural_snapshot),
            title,
            _stream: stream,
            _decode_thread: decode_thread,
        })
    }

    // ── Playback controls ─────────────────────────────────────────────────────

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
    pub fn toggle_loop(&self) {
        let was = self.state.looping.load(Ordering::Relaxed);
        self.state.looping.store(!was, Ordering::Relaxed);
    }
    pub fn is_paused(&self)   -> bool { self.state.paused.load(Ordering::Relaxed) }
    pub fn is_looping(&self)  -> bool { self.state.looping.load(Ordering::Relaxed) }
    pub fn is_finished(&self) -> bool { self.state.finished.load(Ordering::Relaxed) }
    pub fn title(&self)       -> &str { &self.title }

    // ── Live parameter API ────────────────────────────────────────────────────

    /// Update a parameter on the edit identified by `uuid`.
    ///
    /// - **Plugin UUID:** calls `set_parameter` on the live instance immediately.
    ///   Takes effect within one decoded chunk (~85 ms).
    /// - **Structural UUID:** updates the engine's internal snapshot only.
    ///   Takes effect on the next `PlaybackEngine::new` call (after pause + restart).
    pub fn set_edit_param(&self, uuid: Uuid, param_id: &str, value: f32) {
        // Try plugin handles first (hot path)
        for handle in &self.plugin_handles {
            if handle.uuid == uuid {
                if let Ok(mut p) = handle.processor.lock() {
                    p.set_parameter(param_id, value);
                }
                return;
            }
        }
        // Structural: update snapshot
        if let Ok(mut snapshot) = self.structural_snapshot.lock() {
            for edit in snapshot.iter_mut() {
                if edit.uuid == uuid {
                    if let EditKind::Structural { params, .. } = &mut edit.kind {
                        params.insert(param_id.to_string(), value as f64);
                    }
                    break;
                }
            }
        }
    }

    /// Enable or disable the edit identified by `uuid`.
    ///
    /// - **Plugin UUID:** flips the `AtomicBool` gate; skipped on the next chunk.
    /// - **Structural UUID:** updates the snapshot. Takes effect on the next engine creation.
    pub fn set_edit_enabled(&self, uuid: Uuid, enabled: bool) {
        for handle in &self.plugin_handles {
            if handle.uuid == uuid {
                handle.enabled.store(enabled, Ordering::Relaxed);
                return;
            }
        }
        if let Ok(mut snapshot) = self.structural_snapshot.lock() {
            for edit in snapshot.iter_mut() {
                if edit.uuid == uuid {
                    edit.enabled = enabled;
                    break;
                }
            }
        }
    }
}

// ── Decode helpers ────────────────────────────────────────────────────────────

fn build_fresh_chain(
    path: &Path,
    edits: &[StructuralEdit],
    registry: &structural_processor_sdk::Registry,
) -> Result<Box<dyn AudioSource>> {
    let source = Box::new(FileAudioSource::new(path)?);
    Ok(build_chain(source, edits, registry))
}

fn decode_loop(
    path: PathBuf,
    structural_edits: Vec<StructuralEdit>,
    plugin_handles: Vec<Arc<PluginHandle>>,
    state: Arc<PlaybackState>,
) {
    let registry = structural_processors::registry();

    let mut chain = match build_fresh_chain(&path, &structural_edits, &registry) {
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
    } as usize;

    let mut cursor_secs = 0.0_f64;
    let total_secs = state.total_frames.load(Ordering::Relaxed) as f64 / sample_rate as f64;

    loop {
        // ── Seek ──────────────────────────────────────────────────────────────
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target) = req.take() {
                match build_fresh_chain(&path, &structural_edits, &registry) {
                    Ok(c) => {
                        chain = c;
                        cursor_secs = target;
                        let frame_pos = (target * sample_rate as f64) as u64;
                        state.position.store(frame_pos, Ordering::Relaxed);
                        if let Ok(mut buf) = state.buffer.lock() { buf.clear(); }
                        // Reset plugin state (flush delay lines, reverb tails)
                        for handle in &plugin_handles {
                            if let Ok(mut p) = handle.processor.lock() { p.reset(); }
                        }
                    }
                    Err(e) => eprintln!("seek chain rebuild error: {e}"),
                }
            }
        }

        if state.paused.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        // ── End of stream ─────────────────────────────────────────────────────
        if cursor_secs >= total_secs {
            while state.buffer.lock().map(|b| b.len()).unwrap_or(1) > 0 {
                thread::sleep(Duration::from_millis(5));
            }
            state.position.store(0, Ordering::Relaxed);
            if !state.looping.load(Ordering::Relaxed) {
                state.paused.store(true, Ordering::Relaxed);
            }
            chain = match build_fresh_chain(&path, &structural_edits, &registry) {
                Ok(c) => c,
                Err(e) => { eprintln!("decode rebuild error: {e}"); state.finished.store(true, Ordering::Relaxed); return; }
            };
            cursor_secs = 0.0;
            continue;
        }

        // ── Buffer full — wait ────────────────────────────────────────────────
        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

        // ── Decode + apply plugins ────────────────────────────────────────────
        let mut samples = chain.read_at(cursor_secs, CHUNK_SAMPLES);
        if samples.is_empty() {
            while state.buffer.lock().map(|b| b.len()).unwrap_or(1) > 0 {
                thread::sleep(Duration::from_millis(5));
            }
            state.position.store(0, Ordering::Relaxed);
            if !state.looping.load(Ordering::Relaxed) {
                state.paused.store(true, Ordering::Relaxed);
            }
            chain = match build_fresh_chain(&path, &structural_edits, &registry) {
                Ok(c) => c,
                Err(e) => { eprintln!("decode rebuild error: {e}"); state.finished.store(true, Ordering::Relaxed); return; }
            };
            cursor_secs = 0.0;
            continue;
        }

        // Apply plugin chain in order
        for handle in &plugin_handles {
            if !handle.enabled.load(Ordering::Relaxed) { continue; }
            if let Ok(mut plugin) = handle.processor.lock() {
                plugin.process(&mut samples, ch, sample_rate as f32, cursor_secs);
            }
        }

        cursor_secs += samples.len() as f64 / (sample_rate as f64 * ch as f64);
        if let Ok(mut buf) = state.buffer.lock() {
            buf.extend(&samples);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::registry::EditRegistry;
    use crate::edit::{EditKind, ProcessorEdit};
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::collections::HashMap;
    use tempfile::NamedTempFile;
    use uuid::Uuid;

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
    fn new_with_no_edits_creates_engine() {
        let tmp = write_temp_wav(4410, 44_100);
        let reg = EditRegistry::default();
        let engine = PlaybackEngine::new(tmp.path(), &[], &reg);
        assert!(engine.is_ok());
    }

    #[test]
    fn new_with_plugin_edit_creates_engine() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 0.5_f32);
        let edits = vec![ProcessorEdit {
            uuid: Uuid::new_v4(),
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg);
        assert!(engine.is_ok());
    }

    #[test]
    fn set_edit_param_on_plugin_does_not_panic() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let uuid = Uuid::new_v4();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 1.0_f32);
        let edits = vec![ProcessorEdit {
            uuid,
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg).unwrap();
        // Should not panic; takes effect on next decoded chunk
        engine.set_edit_param(uuid, "gain", 0.5);
    }

    #[test]
    fn set_edit_enabled_on_plugin_does_not_panic() {
        let tmp = write_temp_wav(44_100, 44_100);
        let reg = EditRegistry::default();
        let uuid = Uuid::new_v4();
        let mut params = HashMap::new();
        params.insert("gain".to_string(), 1.0_f32);
        let edits = vec![ProcessorEdit {
            uuid,
            enabled: true,
            kind: EditKind::Plugin { plugin_id: "gain".to_string(), params },
        }];
        let engine = PlaybackEngine::new(tmp.path(), &edits, &reg).unwrap();
        engine.set_edit_enabled(uuid, false);
        engine.set_edit_enabled(uuid, true);
    }
}
