# Structural Processor SDK — Cursor-Based Streaming Redesign

**Date:** 2026-05-25
**Status:** Approved

## Problem

The current SDK exposes an `apply()` method that materialises the entire processed audio buffer before playback begins. This is incompatible with streaming playback: you cannot start playing until the whole file has been processed, and memory usage scales with file length. Future processors like pitch-shift and time-stretch make this worse — their algorithms are inherently stateful and chunk-based.

## Goal

Redesign the SDK so processors produce samples lazily, on demand, as the ring buffer needs filling. The player maintains a cursor in processed-output time and requests chunks from the chain; no full-file pre-processing occurs.

## Constraints

- Processors run natively only (no WASM, no C-ABI exports required).
- Time-mapping functions (`map_time_back`, `map_time_forward`, `output_duration`) must be retained — they are used by the UI for playhead display and seek position calculation.
- Must support future sample-transforming processors: reverse, pitch-shift, time-stretch.

---

## Design

### 1. SDK Traits (`libs/structural-processor-sdk`)

#### `AudioSource`

What a processor reads from. Implemented by `FileAudioSource` in `musicum-core` and by `ProcessorSource` when composing a chain.

```rust
pub trait AudioSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32>;
    fn duration_secs(&self) -> f64;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
}
```

`read_at` is seekable — callers may request any `start_secs` in any order. Implementations seek internally when the position is non-sequential.

#### `StreamingProcessorInstance`

A stateful, streaming processor instance. Replaces `apply()`. Created per-playback-session with params baked in.

```rust
pub trait StreamingProcessorInstance {
    fn fill(
        &mut self,
        output_start: f64,
        output_end: f64,
        source: &mut dyn AudioSource,
    ) -> Vec<f32>;

    fn reset(&mut self);
}
```

- `fill` produces samples for the output time range `[output_start, output_end)`. It calls `source.read_at` for whatever source audio it needs (one call for time-selection, potentially multiple for reordering or transforms).
- `reset` is available but chain rebuild (see §3) is the preferred seek strategy.

#### `ProcessorEntry`

Replaces `StructuralProcessorEntry`. Holds static/pure functions as fn pointers plus a factory for creating streaming instances.

```rust
pub struct ProcessorEntry {
    pub descriptor:       fn() -> &'static ProcessorDescriptor,
    pub validate:         fn(&Params) -> bool,
    pub output_duration:  fn(f64, &Params) -> f64,
    pub map_time_forward: fn(f64, f64, &Params) -> f64,
    pub map_time_back:    fn(f64, f64, &Params) -> f64,
    pub create:           fn(Params) -> Box<dyn StreamingProcessorInstance>,
}
```

Static fns are kept as fn pointers (not on the instance) so the chain runner can call them without constructing an instance — required for output-duration pre-computation and UI time mapping.

---

### 2. Chain Composition (`libs/structural-processor-sdk/src/chain.rs`)

#### `ProcessorSource`

An adapter that wraps a `StreamingProcessorInstance` and its upstream `AudioSource` into a single `AudioSource`. This is the composition primitive.

```rust
struct ProcessorSource {
    processor: Box<dyn StreamingProcessorInstance>,
    inner: Box<dyn AudioSource>,
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
}

impl AudioSource for ProcessorSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        let end_secs = start_secs
            + num_samples as f64 / (self.sample_rate * self.channels as u32) as f64;
        self.processor.fill(start_secs, end_secs, &mut *self.inner)
    }
    fn duration_secs(&self) -> f64 { self.duration_secs }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn channels(&self) -> u16 { self.channels }
}
```

#### `build_chain`

Folds edits over a raw file source, nesting each enabled processor around the previous result.

```rust
pub fn build_chain(
    source: Box<dyn AudioSource>,
    edits: &[Edit],
    registry: &Registry,
) -> Box<dyn AudioSource> {
    edits.iter()
        .filter(|e| e.enabled)
        .fold(source, |inner, edit| {
            let entry = registry.get(&edit.processor_id)
                .expect("unknown processor");
            let source_duration = inner.duration_secs();
            let output_duration = (entry.output_duration)(source_duration, &edit.params);
            let processor = (entry.create)(edit.params.clone());
            let sample_rate = inner.sample_rate();
            let channels = inner.channels();
            Box::new(ProcessorSource {
                processor,
                inner,
                sample_rate,
                channels,
                duration_secs: output_duration,
            })
        })
}
```

`Registry` is `HashMap<String, ProcessorEntry>`, populated by each processor crate's `registry()` function (same pattern as today).

---

### 3. Player Integration (`libs/musicum-core/src/audio/`)

#### `FileAudioSource`

Wraps the existing symphonia decoder. Lives in `musicum-core`, not the SDK (keeps the SDK dependency-free).

```rust
pub struct FileAudioSource {
    decoder: SymphoniaDecoder,
    current_pos_secs: f64,
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
}

impl AudioSource for FileAudioSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        if (start_secs - self.current_pos_secs).abs() > SEEK_THRESHOLD {
            self.decoder.seek(start_secs);
            self.current_pos_secs = start_secs;
        }
        self.decoder.read(num_samples)
    }
}
```

`SEEK_THRESHOLD` (e.g. one sample duration) avoids spurious seeks caused by floating-point drift in sequential reads.

#### Ring Buffer Fill Loop

```rust
// initialise
let mut chain = build_chain(
    Box::new(FileAudioSource::new(&path)?),
    &clip.edits,
    &registry,
);
let mut cursor_secs = 0.0;

// fill loop (runs in decode thread)
let samples = chain.read_at(cursor_secs, CHUNK_SAMPLES);
ring_buffer.extend(&samples);
cursor_secs += CHUNK_SAMPLES as f64 / (sample_rate * channels as u32) as f64;

// seek
chain = build_chain(Box::new(FileAudioSource::new(&path)?), &clip.edits, &registry);
cursor_secs = seek_target_secs;
```

Seek rebuilds the chain from scratch. This is the simplest correct strategy: it costs one file-open and a symphonia seek, which is negligible. Stateful processors (future pitch-shift) need a cold start after a seek anyway.

---

### 4. Implementing a Processor

Time-selection processors (trim, cut, crop, slice) implement `fill` trivially:

```rust
impl StreamingProcessorInstance for TrimInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = TrimProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = TrimProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let num_samples = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, num_samples)
    }
    fn reset(&mut self) {}
}
```

`CutProcessor` calls `source.read_at` twice (once before, once after the cut) and concatenates. Future processors with internal state (e.g. an overlap-add buffer for time-stretch) store that state on the instance struct.

---

## Removals

| Item | Location | Reason |
|---|---|---|
| `StructuralProcessor::apply()` | `processor.rs` | Replaced by `StreamingProcessorInstance::fill()` |
| `apply_chain()` | `chain.rs` | Replaced by `build_chain()` + `ProcessorSource` |
| `implement_sp_chain!` macro | `lib.rs` | Was WASM/C-ABI only |
| All `__sp_*` C-ABI exports | `lib.rs` | Native-only, no longer needed |
| `StructuralProcessorEntry` vtable | `lib.rs` | Replaced by `ProcessorEntry` with `create` factory |
| CLI `main.rs` in structural-processors | `src/main.rs` | Rewrite as thin test harness using `FileAudioSource` |

Time-mapping functions (`map_time_forward`, `map_time_back`, `output_duration`) are retained in `ProcessorEntry` as static fn pointers.

---

## Non-Goals

- No migration system for existing sidecar data — processor IDs and param keys are unchanged.
- No change to the audio plugin SDK (`libs/audio-plugin-sdk`) — that is a separate system.
- No lazy evaluation of audio plugin chains — out of scope for this redesign.
