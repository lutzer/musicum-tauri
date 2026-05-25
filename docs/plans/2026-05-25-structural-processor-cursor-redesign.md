# Structural Processor SDK — Cursor-Based Streaming Redesign

**Goal:** Replace the buffer-materialising `apply()` chain with a lazy, cursor-driven `AudioSource` / `StreamingProcessorInstance` design that feeds the ring buffer on demand.

**Architecture:** Three new abstractions — `AudioSource` (seekable sample source), `StreamingProcessorInstance` (stateful fill loop), and `build_chain` (folds processor instances over a file source into a single `AudioSource`). The player maintains a float cursor and calls `chain.read_at(cursor, CHUNK)` each fill cycle; seeks rebuild the chain from a fresh `FileAudioSource`. Static time-mapping and validation fn pointers stay on `ProcessorEntry` for UI use without constructing instances.

**Tech Stack:** Rust, symphonia 0.5 (decoding/seeking inside `FileAudioSource`), cpal 0.17 (audio output, untouched), `structural-processor-sdk` traits, `musicum-core` player.

---

## File Structure

| Status | Path | Responsibility |
|--------|------|----------------|
| New    | `libs/structural-processor-sdk/src/source.rs` | `AudioSource` trait, `secs_to_samples` helper, `VecAudioSource` test helper |
| Modify | `libs/structural-processor-sdk/src/processor.rs` | Remove `apply`, add `StreamingProcessorInstance` trait + `create` factory on `StructuralProcessor` |
| Modify | `libs/structural-processor-sdk/src/lib.rs` | Replace `StructuralProcessorEntry` → `ProcessorEntry` (swap `apply` for `create`), add `Registry` type alias, remove `implement_sp_chain!` macro entirely |
| Modify | `libs/structural-processor-sdk/src/chain.rs` | Update `Edit` fields, add `ProcessorSource` + `build_chain` + `chain_output_duration`, remove `apply_chain`, update helpers to `Registry`, update tests |
| Modify | `libs/structural-processors/src/processors/trim.rs` | Add `TrimInstance`, impl `StreamingProcessorInstance`, replace `apply` with `create`, update tests |
| Modify | `libs/structural-processors/src/processors/cut.rs` | Add `CutInstance`, impl `StreamingProcessorInstance`, replace `apply` with `create`, update tests |
| Modify | `libs/structural-processors/src/processors/slice.rs` | Add `SliceInstance`, impl `StreamingProcessorInstance`, replace `apply` with `create`, update tests |
| Modify | `libs/structural-processors/src/processors/crop.rs` | Add `CropInstance`, impl `StreamingProcessorInstance`, replace `apply` with `create`, update tests |
| Modify | `libs/structural-processors/src/lib.rs` | `registry()` returns `Registry`, remove `implement_sp_chain!`, update integration tests |
| Modify | `libs/structural-processors/Cargo.toml` | `crate-type = ["rlib"]` — no more `cdylib` |
| Modify | `libs/structural-processors/src/main.rs` | Rewrite: local `VecAudioSource` + `build_chain`, drop direct `apply_chain` usage |
| New    | `libs/musicum-core/src/audio/source.rs` | `FileAudioSource` wrapping symphonia |
| Modify | `libs/musicum-core/src/audio/mod.rs` | `pub mod source; pub use source::FileAudioSource;` |
| Modify | `libs/musicum-core/src/audio/player.rs` | `new(path, edits)`, replace decode loop with `build_chain` + float cursor + seek-by-rebuild |
| Modify | `apps/cli/src/commands/play.rs` | Pass `&[]` edits to `PlaybackEngine::new` |
| Modify | `apps/cli/src/commands/processors.rs` | Iterate `registry().values()` instead of `registry().iter()` |

---

## Task 1 — `AudioSource` trait + helpers

**Files:**
- Create: `libs/structural-processor-sdk/src/source.rs`
- Modify: `libs/structural-processor-sdk/src/lib.rs` (add `pub mod source`)

### Step 1.1 — Write the failing test first

Add to `libs/structural-processor-sdk/src/source.rs`:

```rust
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
```

### Step 1.2 — Run the test; confirm it fails to compile

```sh
cargo test -p structural-processor-sdk
# Expected: error — module `source` not found
```

### Step 1.3 — Wire the module into `lib.rs`

Add at the top of `libs/structural-processor-sdk/src/lib.rs`:

```rust
pub mod source;
pub use source::{AudioSource, VecAudioSource, secs_to_samples};
```

### Step 1.4 — Run tests; confirm they pass

```sh
cargo test -p structural-processor-sdk -- source
# Expected: 5 tests pass
```

---

## Task 2 — `StreamingProcessorInstance` trait + `StructuralProcessor::create`

**Files:**
- Modify: `libs/structural-processor-sdk/src/processor.rs`

### Step 2.1 — Write a minimal failing compile check

At the bottom of `processor.rs` add (inside `#[cfg(test)]`):

```rust
#[cfg(test)]
mod trait_shape_tests {
    use super::*;
    use crate::source::VecAudioSource;

    struct Passthrough;
    impl StreamingProcessorInstance for Passthrough {
        fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
            let n = crate::secs_to_samples(out_end - out_start, source.sample_rate(), source.channels());
            source.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn passthrough_fill_returns_correct_count() {
        let mut src = VecAudioSource::new((0..100).map(|i| i as f32).collect(), 100, 1);
        let mut proc = Passthrough;
        let out = proc.fill(0.0, 0.5, &mut src); // 0..0.5s @100Hz = 50 samples
        assert_eq!(out.len(), 50);
    }
}
```

### Step 2.2 — Run; confirm compile failure

```sh
cargo test -p structural-processor-sdk -- trait_shape_tests
# Expected: error[E0412]: cannot find type `StreamingProcessorInstance`
```

### Step 2.3 — Add `StreamingProcessorInstance` and update `StructuralProcessor`

Replace the full content of `libs/structural-processor-sdk/src/processor.rs`:

```rust
use std::collections::HashMap;

use serde::Serialize;

use crate::source::AudioSource;

pub type Params = HashMap<String, f64>;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParameterDescriptor {
    Time { id: &'static str, name: &'static str, default: f64 },
    Int { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

#[derive(Serialize)]
pub struct ProcessorDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static [ParameterDescriptor],
}

impl ProcessorDescriptor {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("descriptor serialisation failed")
    }
}

/// Stateful, streaming processor instance. Created per playback session with
/// params baked in. `fill` is called repeatedly as the ring buffer needs data.
pub trait StreamingProcessorInstance {
    /// Produce interleaved f32 samples for output time `[out_start, out_end)`.
    /// May call `source.read_at` one or more times.
    fn fill(
        &mut self,
        out_start: f64,
        out_end: f64,
        source: &mut dyn AudioSource,
    ) -> Vec<f32>;

    /// Reset any internal state (e.g. overlap buffers). Chain rebuild on seek
    /// is preferred over calling this, but `reset` is provided for completeness.
    fn reset(&mut self);
}

/// Implemented by every structural processor type. Holds static/pure methods
/// used to populate a `ProcessorEntry` via `ProcessorEntry::of::<P>()`.
pub trait StructuralProcessor {
    fn descriptor() -> &'static ProcessorDescriptor;
    fn validate(params: &Params) -> bool;
    /// Construct a streaming instance with `params` baked in.
    fn create(params: Params) -> Box<dyn StreamingProcessorInstance>;
    fn output_duration(duration: f64, params: &Params) -> f64;
    fn map_time_back(t: f64, duration: f64, params: &Params) -> f64;
    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64;
}
```

### Step 2.4 — Run tests; confirm they pass

```sh
cargo test -p structural-processor-sdk -- trait_shape_tests
# Expected: 1 test passes
```

---

## Task 3 — `ProcessorEntry` + `Registry`, remove `implement_sp_chain!`

**Files:**
- Modify: `libs/structural-processor-sdk/src/lib.rs`

### Step 3.1 — Replace full `lib.rs` content

```rust
pub mod chain;
pub mod processor;
pub mod source;

pub use chain::{Edit, Registry, build_chain, chain_output_duration,
                map_time_forward, map_time_back, descriptors_json, validate_edit};
pub use processor::{
    ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor,
};
pub use source::{AudioSource, VecAudioSource, secs_to_samples};

use std::collections::HashMap;

/// Vtable entry for one structural processor. Static fn pointers allow
/// time-mapping and duration queries without constructing an instance.
pub struct ProcessorEntry {
    pub descriptor:       fn() -> &'static ProcessorDescriptor,
    pub validate:         fn(&Params) -> bool,
    pub create:           fn(Params) -> Box<dyn StreamingProcessorInstance>,
    pub output_duration:  fn(f64, &Params) -> f64,
    pub map_time_forward: fn(f64, f64, &Params) -> f64,
    pub map_time_back:    fn(f64, f64, &Params) -> f64,
}

impl ProcessorEntry {
    pub fn of<P: StructuralProcessor>() -> Self {
        Self {
            descriptor:       P::descriptor,
            validate:         P::validate,
            create:           P::create,
            output_duration:  P::output_duration,
            map_time_forward: P::map_time_forward,
            map_time_back:    P::map_time_back,
        }
    }
}

/// Processor registry: maps processor ID → entry. Built once at startup.
pub type Registry = HashMap<String, ProcessorEntry>;
```

### Step 3.2 — Compile check (will fail until chain.rs exports are updated)

```sh
cargo check -p structural-processor-sdk
# Expected: errors about missing chain exports — continue to Task 4
```

---

## Task 4 — Redesign `chain.rs`

**Files:**
- Modify: `libs/structural-processor-sdk/src/chain.rs`

### Step 4.1 — Write failing tests that describe the new API

Add these tests at the bottom of `chain.rs` (they will fail until implementation):

```rust
#[cfg(test)]
mod new_api_tests {
    use super::*;
    use crate::{ProcessorEntry, StructuralProcessor, StreamingProcessorInstance,
                AudioSource, VecAudioSource, secs_to_samples,
                ParameterDescriptor, ProcessorDescriptor, Params};
    use std::collections::HashMap;

    // ── Passthrough processor ────────────────────────────────────────────────
    static PASS_PARAMS: [ParameterDescriptor; 0] = [];
    static PASS_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "pass", name: "Pass", parameters: &PASS_PARAMS };

    struct PassInstance;
    impl StreamingProcessorInstance for PassInstance {
        fn fill(&mut self, out_start: f64, out_end: f64, src: &mut dyn AudioSource) -> Vec<f32> {
            let n = secs_to_samples(out_end - out_start, src.sample_rate(), src.channels());
            src.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }
    struct PassProcessor;
    impl StructuralProcessor for PassProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &PASS_DESC }
        fn validate(_: &Params) -> bool { true }
        fn create(_: Params) -> Box<dyn StreamingProcessorInstance> { Box::new(PassInstance) }
        fn output_duration(d: f64, _: &Params) -> f64 { d }
        fn map_time_forward(t: f64, _: f64, _: &Params) -> f64 { t }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    // ── Half processor: keeps first half ─────────────────────────────────────
    static HALF_PARAMS: [ParameterDescriptor; 0] = [];
    static HALF_DESC: ProcessorDescriptor =
        ProcessorDescriptor { id: "half", name: "Half", parameters: &HALF_PARAMS };

    struct HalfInstance;
    impl StreamingProcessorInstance for HalfInstance {
        fn fill(&mut self, out_start: f64, out_end: f64, src: &mut dyn AudioSource) -> Vec<f32> {
            let clamped_end = out_end.min(src.duration_secs() / 2.0);
            if clamped_end <= out_start { return vec![]; }
            let n = secs_to_samples(clamped_end - out_start, src.sample_rate(), src.channels());
            src.read_at(out_start, n)
        }
        fn reset(&mut self) {}
    }
    struct HalfProcessor;
    impl StructuralProcessor for HalfProcessor {
        fn descriptor() -> &'static ProcessorDescriptor { &HALF_DESC }
        fn validate(_: &Params) -> bool { true }
        fn create(_: Params) -> Box<dyn StreamingProcessorInstance> { Box::new(HalfInstance) }
        fn output_duration(d: f64, _: &Params) -> f64 { d / 2.0 }
        fn map_time_forward(t: f64, dur: f64, _: &Params) -> f64 { t.min(dur / 2.0) }
        fn map_time_back(t: f64, _: f64, _: &Params) -> f64 { t }
    }

    fn reg() -> Registry {
        let mut m = HashMap::new();
        m.insert("pass".to_string(), ProcessorEntry::of::<PassProcessor>());
        m.insert("half".to_string(), ProcessorEntry::of::<HalfProcessor>());
        m
    }

    fn vec_src(frames: usize) -> Box<dyn AudioSource> {
        Box::new(VecAudioSource::new(
            (0..frames).map(|i| i as f32).collect(),
            100,
            1,
        ))
    }

    fn edits(json: &str) -> Vec<Edit> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn build_chain_passthrough_returns_same_samples() {
        let es = edits(r#"[{"type":"pass","enabled":true,"parameters":{}}]"#);
        let mut chain = build_chain(vec_src(100), &es, &reg());
        let out = chain.read_at(0.0, 100);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[50] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn build_chain_empty_edits_is_passthrough() {
        let mut chain = build_chain(vec_src(50), &[], &reg());
        let out = chain.read_at(0.0, 50);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn build_chain_disabled_edit_is_skipped() {
        let es = edits(r#"[{"type":"half","enabled":false,"parameters":{}}]"#);
        let mut chain = build_chain(vec_src(100), &es, &reg());
        assert!((chain.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn build_chain_half_reduces_duration() {
        let es = edits(r#"[{"type":"half","enabled":true,"parameters":{}}]"#);
        let chain = build_chain(vec_src(100), &es, &reg());
        assert!((chain.duration_secs() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn chain_output_duration_two_halves() {
        let es = edits(r#"[
            {"type":"half","enabled":true,"parameters":{}},
            {"type":"half","enabled":true,"parameters":{}}
        ]"#);
        let dur = chain_output_duration(1.0, &es, &reg());
        assert!((dur - 0.25).abs() < 1e-9);
    }

    #[test]
    fn descriptors_json_contains_registered_ids() {
        let json = descriptors_json(&reg());
        assert!(json.contains("\"id\":\"pass\""));
        assert!(json.contains("\"id\":\"half\""));
    }

    #[test]
    fn validate_edit_known_type_passes() {
        assert!(validate_edit(&reg(), "pass", &HashMap::new()));
    }

    #[test]
    fn validate_edit_unknown_type_is_false() {
        assert!(!validate_edit(&reg(), "wormhole", &HashMap::new()));
    }

    #[test]
    fn map_time_forward_empty_is_identity() {
        assert!((map_time_forward(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_empty_is_identity() {
        assert!((map_time_back(&reg(), &[], 0.7, 1.0) - 0.7).abs() < 1e-9);
    }
}
```

### Step 4.2 — Run; confirm they fail

```sh
cargo test -p structural-processor-sdk -- new_api_tests
# Expected: compile errors — build_chain, chain_output_duration not yet defined
```

### Step 4.3 — Replace full `chain.rs` content

```rust
use std::collections::HashMap;

use serde::Deserialize;

use crate::{AudioSource, Params, ProcessorDescriptor, Registry, secs_to_samples};

#[derive(Deserialize, Default, Clone)]
pub struct Edit {
    #[serde(rename = "type")]
    pub processor_id: String,
    pub enabled: bool,
    #[serde(rename = "parameters")]
    pub params: Params,
}

// ── ProcessorSource ───────────────────────────────────────────────────────────

struct ProcessorSource {
    processor: Box<dyn crate::StreamingProcessorInstance>,
    inner: Box<dyn AudioSource>,
    sample_rate: u32,
    channels: u16,
    duration_secs: f64,
}

impl AudioSource for ProcessorSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        let end_secs = start_secs
            + num_samples as f64 / (self.sample_rate as f64 * self.channels as f64);
        self.processor.fill(start_secs, end_secs, &mut *self.inner)
    }
    fn duration_secs(&self) -> f64 { self.duration_secs }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn channels(&self) -> u16 { self.channels }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fold `edits` over `source`, nesting each enabled processor as a `ProcessorSource`.
/// The returned `AudioSource` is the head of the chain; call `read_at` to pull samples.
pub fn build_chain(
    source: Box<dyn AudioSource>,
    edits: &[Edit],
    registry: &Registry,
) -> Box<dyn AudioSource> {
    edits.iter()
        .filter(|e| e.enabled)
        .fold(source, |inner, edit| {
            let Some(entry) = registry.get(&edit.processor_id) else {
                return inner;
            };
            let output_duration = (entry.output_duration)(inner.duration_secs(), &edit.params);
            let sample_rate = inner.sample_rate();
            let channels = inner.channels();
            let processor = (entry.create)(edit.params.clone());
            Box::new(ProcessorSource {
                processor,
                inner,
                sample_rate,
                channels,
                duration_secs: output_duration,
            })
        })
}

/// Compute the output duration (seconds) of a chain without constructing instances.
pub fn chain_output_duration(raw_duration: f64, edits: &[Edit], registry: &Registry) -> f64 {
    edits.iter()
        .filter(|e| e.enabled)
        .fold(raw_duration, |dur, edit| {
            registry.get(&edit.processor_id)
                .map(|entry| (entry.output_duration)(dur, &edit.params))
                .unwrap_or(dur)
        })
}

pub fn descriptors_json(registry: &Registry) -> String {
    let mut descriptors: Vec<&ProcessorDescriptor> =
        registry.values().map(|e| (e.descriptor)()).collect();
    // Sort by id for stable output
    descriptors.sort_by_key(|d| d.id);
    serde_json::to_string(&descriptors).expect("descriptor serialisation failed")
}

pub fn validate_edit(registry: &Registry, processor_id: &str, params: &Params) -> bool {
    registry.get(processor_id).is_some_and(|e| (e.validate)(params))
}

/// Map `t` forward through the edit chain. `duration` is the raw audio length in seconds.
pub fn map_time_forward(
    registry: &Registry,
    edits: &[Edit],
    t: f64,
    duration: f64,
) -> f64 {
    let mut current_t = t;
    let mut current_dur = duration;
    for edit in edits {
        if !edit.enabled { continue; }
        if let Some(entry) = registry.get(&edit.processor_id) {
            current_t = (entry.map_time_forward)(current_t, current_dur, &edit.params);
            current_dur = (entry.output_duration)(current_dur, &edit.params);
        }
    }
    current_t
}

/// Map `t` backward through the edit chain. `duration` is the raw audio length in seconds.
pub fn map_time_back(
    registry: &Registry,
    edits: &[Edit],
    t: f64,
    duration: f64,
) -> f64 {
    // Pre-compute input duration before each edit (needed for reverse traversal)
    let mut durations = Vec::with_capacity(edits.len() + 1);
    durations.push(duration);
    for edit in edits.iter() {
        let last = *durations.last().unwrap();
        let next = if !edit.enabled {
            last
        } else if let Some(entry) = registry.get(&edit.processor_id) {
            (entry.output_duration)(last, &edit.params)
        } else {
            last
        };
        durations.push(next);
    }

    let mut current_t = t;
    for (i, edit) in edits.iter().enumerate().rev() {
        if !edit.enabled { continue; }
        if let Some(entry) = registry.get(&edit.processor_id) {
            current_t = (entry.map_time_back)(current_t, durations[i], &edit.params);
        }
    }
    current_t
}
```

### Step 4.4 — Run new tests; confirm they pass

```sh
cargo test -p structural-processor-sdk -- new_api_tests
# Expected: all 10 tests pass
```

### Step 4.5 — Remove the old tests

Delete the `#[cfg(test)] mod tests { ... }` block at the bottom of `chain.rs` that referenced `apply_chain`, `StructuralProcessorEntry`, and `apply`. The `new_api_tests` block replaces them entirely.

### Step 4.6 — Run full SDK test suite

```sh
cargo test -p structural-processor-sdk
# Expected: all tests pass (trait_shape_tests + new_api_tests + source::tests)
```

---

## Task 5 — `TrimInstance` / `StreamingProcessorInstance` for Trim

**Files:**
- Modify: `libs/structural-processors/src/processors/trim.rs`

### Step 5.1 — Write failing `fill` test

Add at the bottom of `trim.rs`:

```rust
#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_no_trim_is_passthrough() {
        let p = params(0.0, 0.0);
        let mut inst = TrimInstance { params: p };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn fill_with_start_trim_shifts_source() {
        // trim start=0.5s on a 1s 100Hz clip → fill(0.0, 0.5) = source frames 50..100
        let p = params(0.5, 0.0);
        let mut inst = TrimInstance { params: p };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_reads_correct_sub_range() {
        // trim start=0.2, end=0.2 on 1s clip → output 0.6s; fill(0.1, 0.4) → source 0.3..0.6
        let p = params(0.2, 0.2);
        let mut inst = TrimInstance { params: p };
        let mut src = mono_src(100);
        let out = inst.fill(0.1, 0.4, &mut src);
        assert_eq!(out.len(), 30); // 0.3s * 100Hz
        assert!((out[0] - 30.0).abs() < 1e-6); // source frame 30 (0.1+0.2=0.3s)
    }
}
```

### Step 5.2 — Run; confirm compile failure

```sh
cargo test -p structural-processors -- fill_tests::trim
# Expected: error: TrimInstance not found
```

### Step 5.3 — Add `TrimInstance` and update `TrimProcessor`

Replace the full content of `trim.rs`:

```rust
use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static TRIM_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "start", name: "Start", default: 0.0 },
    ParameterDescriptor::Time { id: "end", name: "End", default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "trim",
    name: "Trim",
    parameters: &TRIM_PARAMS,
};

pub struct TrimInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for TrimInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = TrimProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = TrimProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct TrimProcessor;

impl StructuralProcessor for TrimProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
        start >= 0.0 && end >= 0.0
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(TrimInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
        (duration - start.min(duration) - end.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        t + start
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let start = params.get("start").copied().unwrap_or(0.0);
        let end   = params.get("end").copied().unwrap_or(0.0);
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

    #[test]
    fn validate_accepts_valid_params() {
        assert!(TrimProcessor::validate(&params(0.5, 1.5)));
        assert!(TrimProcessor::validate(&params(0.0, 0.0)));
    }

    #[test]
    fn validate_rejects_negative_start() {
        assert!(!TrimProcessor::validate(&params(-0.1, 1.0)));
    }

    #[test]
    fn output_duration_basic() {
        assert!((TrimProcessor::output_duration(1.0, &params(0.2, 0.2)) - 0.6).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_adds_start() {
        let p = params(1.0, 2.0);
        assert!((TrimProcessor::map_time_back(0.5, 10.0, &p) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_and_shifts() {
        let p = params(1.0, 3.0);
        assert!((TrimProcessor::map_time_forward(1.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((TrimProcessor::map_time_forward(0.0, 10.0, &p) - 0.0).abs() < 1e-9);
        assert!((TrimProcessor::map_time_forward(8.0, 10.0, &p) - 6.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(start: f64, end: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("start".into(), start);
        m.insert("end".into(), end);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_no_trim_is_passthrough() {
        let mut inst = TrimInstance { params: params(0.0, 0.0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn fill_with_start_trim_shifts_source() {
        let mut inst = TrimInstance { params: params(0.5, 0.0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_reads_correct_sub_range() {
        let mut inst = TrimInstance { params: params(0.2, 0.2) };
        let mut src = mono_src(100);
        let out = inst.fill(0.1, 0.4, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 30.0).abs() < 1e-6);
    }
}
```

### Step 5.4 — Run fill tests; confirm they pass

```sh
cargo test -p structural-processors -- trim
# Expected: all trim tests pass
```

---

## Task 6 — `CutInstance`

**Files:**
- Modify: `libs/structural-processors/src/processors/cut.rs`

The cut processor removes a range `[from, to)` from the audio. `fill` must handle three cases: the requested output range is entirely before the cut, entirely after, or spans the cut (requiring two `read_at` calls).

### Step 6.1 — Write failing tests

Add at bottom of `cut.rs`:

```rust
#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(from: f64, to: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_entirely_before_cut() {
        // cut [0.5, 1.0) on 2s clip; request output [0.0, 0.3) → source [0.0, 0.3)
        let mut inst = CutInstance { params: params(0.5, 1.0) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 0.3, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn fill_entirely_after_cut() {
        // cut [0.3, 0.5) on 2s clip (gap=0.2s); request output [0.3, 0.7) → source [0.5, 0.9)
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.3, 0.7, &mut src);
        assert_eq!(out.len(), 40);
        assert!((out[0] - 50.0).abs() < 1e-6); // source frame 50
    }

    #[test]
    fn fill_spanning_cut_concatenates() {
        // cut [0.3, 0.5) on 2s clip; request output [0.2, 0.5) spans the cut
        // → part1: source [0.2, 0.3) = 10 frames; part2: source [0.5, 0.7) = 20 frames
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0]  - 20.0).abs() < 1e-6); // source frame 20
        assert!((out[10] - 50.0).abs() < 1e-6); // source frame 50 (first after cut)
    }
}
```

### Step 6.2 — Run; confirm compile failure

```sh
cargo test -p structural-processors -- cut::fill_tests
# Expected: CutInstance not found
```

### Step 6.3 — Replace full `cut.rs` content

```rust
use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static CUT_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to",   name: "To",   default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "cut",
    name: "Cut",
    parameters: &CUT_PARAMS,
};

pub struct CutInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for CutInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let from = self.params.get("from").copied().unwrap_or(0.0);
        let to   = self.params.get("to").copied().unwrap_or(0.0);

        if out_end <= from || out_start >= from {
            // No cut boundary crossed: simple mapped read
            let src_start = CutProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
            let n = secs_to_samples(out_end - out_start, source.sample_rate(), source.channels());
            return source.read_at(src_start, n);
        }

        // Range spans the cut: read before-cut portion then after-cut portion
        let part1_n = secs_to_samples(from - out_start, source.sample_rate(), source.channels());
        let mut result = source.read_at(out_start, part1_n);

        let part2_n = secs_to_samples(out_end - from, source.sample_rate(), source.channels());
        result.extend(source.read_at(to, part2_n));
        result
    }
    fn reset(&mut self) {}
}

pub struct CutProcessor;

impl StructuralProcessor for CutProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        from >= 0.0 && to > from
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(CutInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        (duration - (to - from).clamp(0.0, duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        if t >= from { t + (to - from) } else { t }
    }

    fn map_time_forward(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        if t < from      { t }
        else if t < to   { from }
        else             { t - (to - from) }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn params(from: f64, to: f64) -> Params {
        let mut m = HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    #[test]
    fn validate_accepts_valid_params() { assert!(CutProcessor::validate(&params(0.5, 1.5))); }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CutProcessor::validate(&params(1.0, 0.5)));
        assert!(!CutProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn map_time_back_adds_gap_for_times_at_or_after_from() {
        let p = params(1.0, 2.0);
        assert!((CutProcessor::map_time_back(1.0, 10.0, &p) - 2.0).abs() < 1e-9);
        assert!((CutProcessor::map_time_back(0.5, 10.0, &p) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_snaps_cut_region_to_boundary() {
        let p = params(1.0, 2.0);
        assert!((CutProcessor::map_time_forward(0.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((CutProcessor::map_time_forward(1.5, 10.0, &p) - 1.0).abs() < 1e-9);
        assert!((CutProcessor::map_time_forward(2.5, 10.0, &p) - 1.5).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(from: f64, to: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_entirely_before_cut() {
        let mut inst = CutInstance { params: params(0.5, 1.0) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 0.3, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn fill_entirely_after_cut() {
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.3, 0.7, &mut src);
        assert_eq!(out.len(), 40);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_spanning_cut_concatenates() {
        let mut inst = CutInstance { params: params(0.3, 0.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0]  - 20.0).abs() < 1e-6);
        assert!((out[10] - 50.0).abs() < 1e-6);
    }
}
```

### Step 6.4 — Run cut tests; confirm they pass

```sh
cargo test -p structural-processors -- cut
# Expected: all cut tests pass
```

---

## Task 7 — `SliceInstance`

**Files:**
- Modify: `libs/structural-processors/src/processors/slice.rs`

Slice is a single-range selection (like trim), so `fill` is the same pattern as `TrimInstance`.

### Step 7.1 — Write failing tests (add at bottom of `slice.rs`)

```rust
#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(slices: i64, select: i64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("slices".into(), slices as f64);
        m.insert("select_slice".into(), select as f64);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_selects_correct_slice() {
        // 100 frames, 4 slices → each 25 frames; select slice 2 → source frames 50..75
        let mut inst = SliceInstance { params: params(4, 2) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.25, &mut src); // output 0..0.25s = 25 frames
        assert_eq!(out.len(), 25);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_first_slice_reads_from_zero() {
        let mut inst = SliceInstance { params: params(2, 0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }
}
```

### Step 7.2 — Replace full `slice.rs` content

```rust
use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static SLICE_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Int { id: "slices",       name: "Slices",       default: 2,  min: 1, max: 64 },
    ParameterDescriptor::Int { id: "select_slice", name: "Select Slice", default: 0,  min: 0, max: 63 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "slice",
    name: "Slice",
    parameters: &SLICE_PARAMS,
};

pub struct SliceInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for SliceInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = SliceProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = SliceProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct SliceProcessor;

impl StructuralProcessor for SliceProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let slices = params.get("slices").copied().unwrap_or(0.0) as i64;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as i64;
        slices >= 1 && select >= 0 && select < slices
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(SliceInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        duration / slices as f64
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let slices = params.get("slices").copied().unwrap_or(1.0).max(1.0) as usize;
        let select = params.get("select_slice").copied().unwrap_or(0.0) as usize;
        let slice_dur   = duration / slices as f64;
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

    #[test]
    fn validate_accepts_valid_params() {
        assert!(SliceProcessor::validate(&params(4, 0)));
        assert!(SliceProcessor::validate(&params(4, 3)));
    }

    #[test]
    fn validate_rejects_out_of_bounds_select() {
        assert!(!SliceProcessor::validate(&params(4, 4)));
    }

    #[test]
    fn validate_rejects_zero_slices() {
        assert!(!SliceProcessor::validate(&params(0, 0)));
    }

    #[test]
    fn output_duration_is_slice_fraction() {
        assert!((SliceProcessor::output_duration(1.0, &params(4, 0)) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_into_selected_slice() {
        let p = params(4, 2);
        assert!((SliceProcessor::map_time_forward(0.6, 1.0, &p) - 0.1).abs() < 1e-9);
        assert!((SliceProcessor::map_time_forward(0.0, 1.0, &p) - 0.0).abs() < 1e-9);
        assert!((SliceProcessor::map_time_forward(0.9, 1.0, &p) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn map_time_back_adds_slice_offset() {
        let p = params(4, 2);
        assert!((SliceProcessor::map_time_back(0.1, 1.0, &p) - 0.6).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(slices: i64, select: i64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("slices".into(), slices as f64);
        m.insert("select_slice".into(), select as f64);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_selects_correct_slice() {
        let mut inst = SliceInstance { params: params(4, 2) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.25, &mut src);
        assert_eq!(out.len(), 25);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_first_slice_reads_from_zero() {
        let mut inst = SliceInstance { params: params(2, 0) };
        let mut src = mono_src(100);
        let out = inst.fill(0.0, 0.5, &mut src);
        assert_eq!(out.len(), 50);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }
}
```

### Step 7.3 — Run slice tests; confirm they pass

```sh
cargo test -p structural-processors -- slice
```

---

## Task 8 — `CropInstance`

**Files:**
- Modify: `libs/structural-processors/src/processors/crop.rs`

Crop is also a single-range selection. Same `fill` pattern as Trim and Slice.

### Step 8.1 — Write failing tests (add at bottom of `crop.rs`)

```rust
#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(from: f64, to: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_returns_correct_range() {
        // crop [0.5, 1.5] on 2s clip; fill(0.0, 1.0) → source [0.5, 1.5)
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_partial_read_in_range() {
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src); // output 0.2..0.5 → source 0.7..1.0
        assert_eq!(out.len(), 30);
        assert!((out[0] - 70.0).abs() < 1e-6);
    }
}
```

### Step 8.2 — Replace full `crop.rs` content

```rust
use structural_processor_sdk::{
    AudioSource, ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor, secs_to_samples,
};

static CROP_PARAMS: [ParameterDescriptor; 2] = [
    ParameterDescriptor::Time { id: "from", name: "From", default: 0.0 },
    ParameterDescriptor::Time { id: "to",   name: "To",   default: 0.0 },
];

static DESCRIPTOR: ProcessorDescriptor = ProcessorDescriptor {
    id: "crop",
    name: "Crop",
    parameters: &CROP_PARAMS,
};

pub struct CropInstance {
    pub params: Params,
}

impl StreamingProcessorInstance for CropInstance {
    fn fill(&mut self, out_start: f64, out_end: f64, source: &mut dyn AudioSource) -> Vec<f32> {
        let src_start = CropProcessor::map_time_back(out_start, source.duration_secs(), &self.params);
        let src_end   = CropProcessor::map_time_back(out_end,   source.duration_secs(), &self.params);
        let n = secs_to_samples(src_end - src_start, source.sample_rate(), source.channels());
        source.read_at(src_start, n)
    }
    fn reset(&mut self) {}
}

pub struct CropProcessor;

impl StructuralProcessor for CropProcessor {
    fn descriptor() -> &'static ProcessorDescriptor { &DESCRIPTOR }

    fn validate(params: &Params) -> bool {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(0.0);
        from >= 0.0 && to > from
    }

    fn create(params: Params) -> Box<dyn StreamingProcessorInstance> {
        Box::new(CropInstance { params })
    }

    fn output_duration(duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(duration);
        (to.min(duration) - from.min(duration)).max(0.0)
    }

    fn map_time_back(t: f64, _duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        t + from
    }

    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64 {
        let from = params.get("from").copied().unwrap_or(0.0);
        let to   = params.get("to").copied().unwrap_or(duration);
        t.max(from).min(to) - from
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn params(from: f64, to: f64) -> Params {
        let mut m = HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    #[test]
    fn validate_accepts_valid_params() { assert!(CropProcessor::validate(&params(0.5, 1.5))); }

    #[test]
    fn validate_rejects_to_lte_from() {
        assert!(!CropProcessor::validate(&params(1.0, 0.5)));
        assert!(!CropProcessor::validate(&params(1.0, 1.0)));
    }

    #[test]
    fn map_time_back_adds_from() {
        let p = params(2.0, 5.0);
        assert!((CropProcessor::map_time_back(1.0, 10.0, &p) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn map_time_forward_clamps_and_shifts() {
        let p = params(2.0, 5.0);
        assert!((CropProcessor::map_time_forward(2.5, 10.0, &p) - 0.5).abs() < 1e-9);
        assert!((CropProcessor::map_time_forward(1.0, 10.0, &p) - 0.0).abs() < 1e-9);
        assert!((CropProcessor::map_time_forward(6.0, 10.0, &p) - 3.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod fill_tests {
    use super::*;
    use structural_processor_sdk::{VecAudioSource, AudioSource};

    fn params(from: f64, to: f64) -> Params {
        let mut m = std::collections::HashMap::new();
        m.insert("from".into(), from);
        m.insert("to".into(), to);
        m
    }

    fn mono_src(frames: usize) -> VecAudioSource {
        VecAudioSource::new((0..frames).map(|i| i as f32).collect(), 100, 1)
    }

    #[test]
    fn fill_returns_correct_range() {
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.0, 1.0, &mut src);
        assert_eq!(out.len(), 100);
        assert!((out[0] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn fill_partial_read_in_range() {
        let mut inst = CropInstance { params: params(0.5, 1.5) };
        let mut src = mono_src(200);
        let out = inst.fill(0.2, 0.5, &mut src);
        assert_eq!(out.len(), 30);
        assert!((out[0] - 70.0).abs() < 1e-6);
    }
}
```

### Step 8.3 — Run crop tests; confirm they pass

```sh
cargo test -p structural-processors -- crop
```

---

## Task 9 — Update `structural-processors/src/lib.rs` + `Cargo.toml`

**Files:**
- Modify: `libs/structural-processors/src/lib.rs`
- Modify: `libs/structural-processors/Cargo.toml`

### Step 9.1 — Write failing integration test

The integration tests currently call `apply_chain`. Replace them with `build_chain` tests. Add to `lib.rs`:

```rust
#[cfg(test)]
mod integration_tests {
    use structural_processor_sdk::{
        build_chain, chain_output_duration, VecAudioSource, AudioSource,
        map_time_forward, map_time_back, validate_edit,
        chain::Edit,
    };
    use std::collections::HashMap;

    fn edits(json: &str) -> Vec<Edit> {
        serde_json::from_str(json).unwrap()
    }

    fn mono_src(frames: usize) -> Box<dyn AudioSource> {
        Box::new(VecAudioSource::new(
            (0..frames).map(|i| i as f32).collect(),
            100,
            1,
        ))
    }

    #[test]
    fn chain_trim_then_read_correct_length() {
        // 100-frame @100Hz; trim start=0.2s end=0.2s → output 0.6s = 60 frames
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":0.2,"end":0.2}}]"#);
        let mut chain = build_chain(mono_src(100), &es, &super::registry());
        let out = chain.read_at(0.0, 60);
        assert_eq!(out.len(), 60);
        assert!((out[0] - 20.0).abs() < 1e-6); // first source frame after trim start
    }

    #[test]
    fn chain_trim_then_cut() {
        // 100-frame @100Hz; trim start=0.2 end=0.2 → 60 frames; cut [0.1, 0.3] → 40 frames
        let es = edits(r#"[
            {"type":"trim","enabled":true,"parameters":{"start":0.2,"end":0.2}},
            {"type":"cut","enabled":true,"parameters":{"from":0.1,"to":0.3}}
        ]"#);
        let output_dur = chain_output_duration(1.0, &es, &super::registry());
        assert!((output_dur - 0.4).abs() < 1e-9);
    }

    #[test]
    fn chain_skips_disabled_edits() {
        let es = edits(r#"[{"type":"trim","enabled":false,"parameters":{"start":0.5,"end":0.9}}]"#);
        let chain = build_chain(mono_src(100), &es, &super::registry());
        assert!((chain.duration_secs() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn chain_unknown_type_is_passthrough() {
        let es = edits(r#"[{"type":"wormhole","enabled":true,"parameters":{}}]"#);
        let chain = build_chain(mono_src(50), &es, &super::registry());
        assert!((chain.duration_secs() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn validate_edit_trim_valid() {
        let mut p = HashMap::new();
        p.insert("start".into(), 0.5_f64);
        p.insert("end".into(), 1.5_f64);
        assert!(validate_edit(&super::registry(), "trim", &p));
    }

    #[test]
    fn validate_edit_unknown_type_is_false() {
        assert!(!validate_edit(&super::registry(), "wormhole", &HashMap::new()));
    }

    #[test]
    fn map_time_forward_trim_identity_at_zero() {
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":0.0,"end":0.0}}]"#);
        let result = map_time_forward(&super::registry(), &es, 0.0, 1.0);
        assert!(result.abs() < 1e-9);
    }

    #[test]
    fn map_time_back_trim_adds_start() {
        let es = edits(r#"[{"type":"trim","enabled":true,"parameters":{"start":1.0,"end":2.0}}]"#);
        let result = map_time_back(&super::registry(), &es, 0.5, 2.0);
        assert!((result - 1.5).abs() < 1e-9);
    }
}
```

### Step 9.2 — Run; confirm compile failure

```sh
cargo test -p structural-processors -- integration_tests
# Expected: errors about build_chain, registry() type mismatch
```

### Step 9.3 — Replace full `lib.rs` content

```rust
pub mod processors;

use structural_processor_sdk::{ProcessorEntry, Registry};
use processors::{
    crop::CropProcessor, cut::CutProcessor,
    slice::SliceProcessor, trim::TrimProcessor,
};

pub fn registry() -> Registry {
    let mut m = Registry::new();
    m.insert("trim".to_string(),  ProcessorEntry::of::<TrimProcessor>());
    m.insert("cut".to_string(),   ProcessorEntry::of::<CutProcessor>());
    m.insert("slice".to_string(), ProcessorEntry::of::<SliceProcessor>());
    m.insert("crop".to_string(),  ProcessorEntry::of::<CropProcessor>());
    m
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_registry_has_four_entries() {
        let r = super::registry();
        assert_eq!(r.len(), 4);
        assert!(r.contains_key("trim"));
        assert!(r.contains_key("cut"));
        assert!(r.contains_key("slice"));
        assert!(r.contains_key("crop"));
    }
}

// (integration_tests module placed here — see plan step 9.1)
```

> Add the `integration_tests` module from Step 9.1 at the bottom of this file.

### Step 9.4 — Update `Cargo.toml`

In `libs/structural-processors/Cargo.toml`, change:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

to:

```toml
[lib]
crate-type = ["rlib"]
```

### Step 9.5 — Run all structural-processors tests

```sh
cargo test -p structural-processors
# Expected: all tests pass (registry + integration_tests + per-processor tests)
```

---

## Task 10 — Rewrite `structural-processors/src/main.rs`

**Files:**
- Modify: `libs/structural-processors/src/main.rs`

The CLI becomes a thin test harness: load a WAV → `VecAudioSource` → `build_chain` → `read_at(0.0, all_samples)` → write output WAV. It no longer calls `apply_chain`.

### Step 10.1 — Replace full `main.rs` content

```rust
mod processors;

use std::io::{self, Read};

use structural_processor_sdk::{
    AudioSource, VecAudioSource, build_chain,
    chain::{descriptors_json, Edit},
};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

fn read_wav(path: &str) -> (Vec<f32>, u32, u16) {
    let mut reader = WavReader::open(path).expect("failed to open input WAV");
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        SampleFormat::Int => {
            let scale = (1_u32 << (spec.bits_per_sample as u32 - 1)) as f32;
            reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
        }
    };
    (samples, spec.sample_rate, spec.channels)
}

fn write_wav(path: &str, samples: &[f32], sample_rate: u32, channels: u16) {
    let spec = WavSpec { channels, sample_rate, bits_per_sample: 32, sample_format: SampleFormat::Float };
    let mut w = WavWriter::create(path, spec).expect("failed to create output WAV");
    for &s in samples { w.write_sample(s).unwrap(); }
    w.finalize().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && args[1] == "--descriptors" {
        println!("{}", descriptors_json(&structural_processors::registry()));
        return;
    }

    if args.len() != 3 {
        eprintln!("Usage: structural-processor <input.wav> <output.wav>");
        eprintln!("       structural-processor --descriptors");
        std::process::exit(1);
    }

    let (samples, sample_rate, channels) = read_wav(&args[1]);
    let total = samples.len();
    let source: Box<dyn AudioSource> = Box::new(VecAudioSource::new(samples, sample_rate, channels));

    let mut edits_json = String::new();
    io::stdin().read_to_string(&mut edits_json).expect("failed to read stdin");
    let edits: Vec<Edit> = serde_json::from_str(&edits_json).expect("invalid edits JSON");

    let mut chain = build_chain(source, &edits, &structural_processors::registry());
    let output = chain.read_at(0.0, total); // upper bound; actual output may be shorter
    write_wav(&args[2], &output, sample_rate, channels);
}
```

### Step 10.2 — Build the binary

```sh
cargo build -p structural-processors
# Expected: compiles without warnings
```

### Step 10.3 — Smoke test

```sh
# generate a 1-second sine wave WAV with sox (or use any test WAV)
echo '[]' | cargo run -p structural-processors -- /tmp/test_input.wav /tmp/test_output.wav
# Expected: writes output WAV of the same length
```

---

## Task 11 — `FileAudioSource` in `musicum-core`

**Files:**
- Create: `libs/musicum-core/src/audio/source.rs`
- Modify: `libs/musicum-core/src/audio/mod.rs`

`FileAudioSource` wraps a symphonia format reader + decoder and implements `AudioSource`. Seek is by time; sequential reads continue from the current position.

### Step 11.1 — Write failing tests (to be added to `source.rs`)

These tests require a real WAV file. Use `hound` (already in dev-dependencies) to write a temp WAV, then test `FileAudioSource` against it.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::io::Cursor;
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
        let tmp = write_temp_wav(100, 100);
        let mut src = FileAudioSource::new(tmp.path()).unwrap();
        // First read: frames 0..10
        let _ = src.read_at(0.0, 10);
        // Seek to 0.5s and read 10 frames
        let out = src.read_at(0.5, 10);
        assert_eq!(out.len(), 10);
        // Values should be ~0.5 (i/frames = 50/100..59/100)
        assert!(out[0] > 0.4 && out[0] < 0.6);
    }
}
```

### Step 11.2 — Run; confirm compile failure

```sh
cargo test -p musicum-core -- audio::source
# Expected: module not found
```

### Step 11.3 — Create `source.rs`

```rust
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
        })
    }

    fn seek_internal(&mut self, secs: f64) {
        let seek_to = SeekTo::Time {
            time: Time::from(secs),
            track_id: Some(self.track_id),
        };
        let _ = self.format.seek(SeekMode::Coarse, seek_to);
        self.decoder.reset();
        self.current_pos_secs = secs;
    }
}

impl AudioSource for FileAudioSource {
    fn read_at(&mut self, start_secs: f64, num_samples: usize) -> Vec<f32> {
        if (start_secs - self.current_pos_secs).abs() > SEEK_THRESHOLD {
            self.seek_internal(start_secs);
        }

        let mut result = Vec::with_capacity(num_samples);
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
                    result.extend_from_slice(sample_buf.samples());
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(_) => break,
            }
        }
        result.truncate(num_samples);
        self.current_pos_secs = start_secs
            + result.len() as f64 / (self.sample_rate as f64 * self.channels as f64);
        result
    }

    fn duration_secs(&self) -> f64 { self.duration_secs }
    fn sample_rate(&self) -> u32   { self.sample_rate }
    fn channels(&self) -> u16      { self.channels }
}
```

Add the `#[cfg(test)]` block from Step 11.1 at the bottom of this file.

### Step 11.4 — Update `audio/mod.rs`

```rust
pub mod player;
pub mod source;

pub use player::PlaybackEngine;
pub use source::FileAudioSource;
```

### Step 11.5 — Run tests

```sh
cargo test -p musicum-core -- audio::source
# Expected: 3 tests pass
```

---

## Task 12 — Update `PlaybackEngine` to use `build_chain` + cursor

**Files:**
- Modify: `libs/musicum-core/src/audio/player.rs`

The player's `new()` now takes `edits: &[Edit]`. Internally it builds the registry from `structural_processors::registry()`. The decode thread holds the chain and uses a float cursor.

### Step 12.1 — Note: no pre-existing test to fail first

The player is integration-tested by running the CLI (`apps/cli`). We'll verify correctness by running the CLI play command after this task.

### Step 12.2 — Replace full `player.rs` content

```rust
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
            sample_rate: cpal::SampleRate(sample_rate),
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

        let state_dec  = Arc::clone(&state);
        let path_owned = path.to_path_buf();
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
        // peek channels from first FileAudioSource
        let probe = FileAudioSource::new(&path);
        probe.map(|s| s.channels()).unwrap_or(2)
    };
    let mut cursor_secs = 0.0_f64;
    let total_secs = state.total_frames.load(Ordering::Relaxed) as f64 / sample_rate as f64;

    loop {
        // Handle seek
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

        // Back-pressure
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
```

### Step 12.3 — Compile check

```sh
cargo build -p musicum-core
# Expected: compiles. Fix any type errors (e.g. SampleRate wrapping).
```

---

## Task 13 — Update CLI callers

**Files:**
- Modify: `apps/cli/src/commands/play.rs`
- Modify: `apps/cli/src/commands/processors.rs`

### Step 13.1 — Update `play.rs`

Change the `PlaybackEngine::new` call:

```rust
// Before:
let engine = PlaybackEngine::new(&path)?;

// After:
let engine = PlaybackEngine::new(&path, &[])?;
```

No other changes needed. Clip-edit wiring is deferred to a follow-up.

### Step 13.2 — Update `processors.rs`

The registry is now a `HashMap`. Change `.iter()` to `.values()` and sort for stable display:

```rust
pub fn run(args: ProcessorsArgs) {
    match args.command {
        ProcessorsCommand::List { json } => {
            let registry = structural_processors::registry();
            let mut entries: Vec<_> = registry.values().collect();
            entries.sort_by_key(|e| (e.descriptor)().id);

            if json {
                let descriptors: Vec<_> = entries.iter().map(|e| (e.descriptor)()).collect();
                print_json(&descriptors);
            } else if entries.is_empty() {
                println!("No processors registered.");
            } else {
                let rows: Vec<(String, String, String)> = entries
                    .iter()
                    .map(|e| {
                        let d = (e.descriptor)();
                        let params = d.parameters.iter()
                            .map(|p| match p {
                                ParameterDescriptor::Time { id, default, .. } =>
                                    format!("{id}={default} (time)"),
                                ParameterDescriptor::Int { id, default, .. } =>
                                    format!("{id}={default} (int)"),
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        (d.id.to_string(), d.name.to_string(), params)
                    })
                    .collect();
                print_table_3col(("ID", "NAME", "PARAMETERS"), rows);
            }
        }
    }
}
```

### Step 13.3 — Build the CLI

```sh
cargo build -p musicum-cli
# Expected: compiles without warnings
```

---

## Task 14 — Full test suite + lint

### Step 14.1 — Run all tests

```sh
cargo test -p structural-processor-sdk
# Expected: source::tests (5), trait_shape_tests (1), new_api_tests (10) — 16 total

cargo test -p structural-processors
# Expected: 4 per-processor unit + fill tests + registry (1) + integration_tests (8)

cargo test -p musicum-core
# Expected: existing tests + audio::source (3) all pass
```

### Step 14.2 — Clippy

```sh
cargo clippy -p structural-processor-sdk -p structural-processors -p musicum-core -p musicum-cli -- -D warnings
# Expected: no warnings
```

### Step 14.3 — Smoke-test the CLI

```sh
cargo run -p musicum-cli -- processors list
# Expected: table showing trim, cut, slice, crop

# If you have a WAV file:
cargo run -p musicum-cli -- play /path/to/audio.wav
# Expected: TUI player opens and audio plays
```

---

## Removals Summary

| What | Where | Replaced by |
|------|-------|-------------|
| `StructuralProcessor::apply()` | `processor.rs` | `StreamingProcessorInstance::fill()` |
| `StructuralProcessorEntry` | `lib.rs` | `ProcessorEntry` (with `create` instead of `apply`) |
| `apply_chain()` | `chain.rs` | `build_chain()` + `ProcessorSource` |
| `implement_sp_chain!` macro | `lib.rs` | — (native-only, no WASM needed) |
| All `__sp_*` C-ABI exports | `lib.rs` | — |
| `cdylib` crate-type | `structural-processors/Cargo.toml` | `rlib` only |
| Old `chain.rs` tests | `chain.rs` | `new_api_tests` module |
| `apply_*` tests in each processor | per-processor files | `fill_*` tests |
| Integration tests using `apply_chain` | `structural-processors/lib.rs` | `integration_tests` using `build_chain` |

Time-mapping functions (`map_time_forward`, `map_time_back`, `output_duration`) and `validate_edit`, `descriptors_json` are **retained** in `chain.rs`, updated to take `&Registry` instead of `&[StructuralProcessorEntry]`.
