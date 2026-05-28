use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use audio_plugin_sdk::PluginProcessor;
use structural_processor_sdk::{
    chain::{build_chain, StructuralEdit},
    AudioSource,
};
use uuid::Uuid;

use super::registry::EditRegistry;
use super::source::FileAudioSource;
use crate::edit::{EditKind, ProcessorEdit};

pub(super) const BUFFER_CAPACITY: usize = 48_000 * 2 * 2;
pub(super) const CHUNK_SAMPLES:   usize = 4_096;

pub(super) struct PluginHandle {
    pub uuid:      Uuid,
    pub enabled:   AtomicBool,
    pub processor: Mutex<Box<dyn PluginProcessor>>,
}

/// Instantiate one `PluginHandle` per plugin edit (enabled and disabled alike).
pub(super) fn build_plugin_handles(
    edits: &[ProcessorEdit],
    registry: &EditRegistry,
) -> Vec<Arc<PluginHandle>> {
    let mut handles = Vec::new();
    for edit in edits {
        if let EditKind::Plugin { plugin_id, params } = &edit.kind {
            if let Some(entry) = registry.plugins.get(plugin_id) {
                let mut instance = (entry.create)();
                for (id, &val) in params {
                    instance.set_parameter(id, val);
                }
                handles.push(Arc::new(PluginHandle {
                    uuid:      edit.uuid,
                    enabled:   AtomicBool::new(edit.enabled),
                    processor: Mutex::new(instance),
                }));
            } else {
                eprintln!("warning: unknown plugin '{plugin_id}' — skipped");
            }
        }
    }
    handles
}

pub(super) fn build_fresh_chain(
    path: &Path,
    edits: &[StructuralEdit],
    registry: &structural_processor_sdk::Registry,
) -> Result<Box<dyn AudioSource>> {
    let source = Box::new(FileAudioSource::new(path)?);
    Ok(build_chain(source, edits, registry))
}

pub(super) fn decode_loop(
    path: PathBuf,
    structural_edits: Vec<StructuralEdit>,
    plugin_handles: Vec<Arc<PluginHandle>>,
    state: Arc<super::player::PlaybackState>,
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
        if let Ok(mut req) = state.seek_request.lock() {
            if let Some(target) = req.take() {
                match build_fresh_chain(&path, &structural_edits, &registry) {
                    Ok(c) => {
                        chain = c;
                        cursor_secs = target;
                        let frame_pos = (target * sample_rate as f64) as u64;
                        state.position.store(frame_pos, Ordering::Relaxed);
                        if let Ok(mut buf) = state.buffer.lock() { buf.clear(); }
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
                Err(e) => {
                    eprintln!("decode rebuild error: {e}");
                    state.finished.store(true, Ordering::Relaxed);
                    return;
                }
            };
            cursor_secs = 0.0;
            continue;
        }

        if state.buffer.lock().map(|b| b.len()).unwrap_or(0) >= BUFFER_CAPACITY {
            thread::sleep(Duration::from_millis(5));
            continue;
        }

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
                Err(e) => {
                    eprintln!("decode rebuild error: {e}");
                    state.finished.store(true, Ordering::Relaxed);
                    return;
                }
            };
            cursor_secs = 0.0;
            continue;
        }

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
