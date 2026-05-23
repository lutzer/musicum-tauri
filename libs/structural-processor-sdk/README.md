# structural-processor-sdk

Shared Rust crate defining the `StructuralProcessor` trait, chain execution logic, and the `implement_sp_chain!` macro used by all structural processor implementations.

## Overview

This crate is the foundation of Musicum's structural audio editing system. It defines the contract every processor must implement and provides the generic chain-runner that composes multiple processors sequentially, including **bidirectional time mapping** so downstream code (playhead, markers) can stay in sync with edited audio.

## Crate Layout

```
src/
  lib.rs          — public exports and implement_sp_chain! macro
  processor.rs    — StructuralProcessor trait + supporting types
  chain.rs        — chain execution and time mapping
```

## Core Trait

```rust
pub trait StructuralProcessor {
    fn descriptor() -> &'static ProcessorDescriptor;

    /// Returns false if params are out of range / inconsistent.
    fn validate(params: &Params) -> bool;

    /// Applies the edit. Input/output are interleaved f32 samples.
    fn apply(samples: &[f32], sample_rate: u32, channels: u16, params: &Params) -> Vec<f32>;

    /// Duration of the output audio given an input duration.
    fn output_duration(duration: f64, params: &Params) -> f64;

    /// Maps a time in the source domain forward to the processed domain.
    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64;

    /// Maps a time in the processed domain back to the source domain.
    fn map_time_back(t: f64, duration: f64, params: &Params) -> f64;
}
```

Interleaved f32 sample layout: `[L0, R0, L1, R1, …]` for stereo; `[S0, S1, …]` for mono.

### Types

```rust
pub struct ProcessorDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static [ParameterDescriptor],
}

pub enum ParameterDescriptor {
    Time  { id: &'static str, name: &'static str, default: f64 },
    Int   { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

pub type Params = serde_json::Map<String, serde_json::Value>;

pub struct Edit {
    pub id: String,
    #[serde(rename = "type")]
    pub processor_type: String,
    pub enabled: bool,
    pub parameters: Params,
}
```

## Chain Execution

`apply_chain` runs edits sequentially, skipping disabled ones:

```rust
pub fn apply_chain(
    registry: &[ProcessorEntry],
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    edits: &[Edit],
) -> Vec<f32>
```

Time mapping traverses the same chain:

```rust
pub fn map_time_forward(registry: &[ProcessorEntry], edits: &[Edit], t: f64, duration: f64) -> f64;
pub fn map_time_back(registry: &[ProcessorEntry], edits: &[Edit], t: f64, duration: f64) -> f64;
```

## WASM Export Macro

`implement_sp_chain!(P1, P2, …)` generates all C-ABI exports needed by the JS SDK:

```rust
// In the implementations crate's lib.rs:
implement_sp_chain!(TrimProcessor, CutProcessor, SliceProcessor, CropProcessor);
```

**Generated exports:**

| Symbol | Description |
|---|---|
| `__sp_alloc(size) -> ptr` | Allocate bytes in WASM heap |
| `__sp_free(ptr, len)` | Free a previous allocation |
| `__sp_apply_chain(samplesPtr, samplesLen, sampleRate, channels, editsPtr, editsLen)` | Run the full edit chain |
| `__sp_result_ptr() -> ptr` | Pointer to last result buffer |
| `__sp_result_len() -> u32` | Byte length of last result buffer |
| `__sp_descriptors_init()` | Serialise descriptors into static buffer |
| `__sp_descriptors_ptr() -> ptr` | Pointer to descriptor JSON bytes |
| `__sp_descriptors_len() -> u32` | Byte length of descriptor JSON |
| `__sp_validate_edit(typePtr, typeLen, paramsPtr, paramsLen) -> u32` | 1 if valid, 0 if not |
| `__sp_map_time_forward(editsPtr, editsLen, t, duration) -> f64` | Forward time mapping |
| `__sp_map_time_back(editsPtr, editsLen, t, duration) -> f64` | Backward time mapping |

## JS SDK

The companion TypeScript package lives in `js/` — see [`js/README.md`](js/README.md).

## Adding a New Processor

1. Implement `StructuralProcessor` for your type.
2. Add it to the `implement_sp_chain!` invocation in the implementations crate.
3. Run `build.sh` to regenerate the WASM binary and descriptor JSON.
