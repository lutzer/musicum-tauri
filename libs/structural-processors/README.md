# structural-processors

Concrete implementations of the four built-in Musicum structural audio processors: **Trim**, **Cut**, **Slice**, and **Crop**. Compiles to both a WASM library (for the browser) and a native CLI binary (for the backend and build tooling).

## Overview

Structural edits are non-destructive, time-aware operations on audio: they remove or select regions of a waveform without touching the source file. This crate wires together the `structural-processor-sdk` trait with four concrete processors, then exports everything via `implement_sp_chain!` so the JS SDK and backend can use the same logic.

## Processors

| type | Parameters | Effect |
|---|---|---|
| `trim` | `start` (s), `end` (s) | Removes `start` seconds from the beginning and `end` seconds from the end |
| `cut` | `from` (s), `to` (s) | Removes the region `[from, to]` and concatenates the remaining audio |
| `slice` | `slices` (int, ≥ 1), `select_slice` (int, 0-based) | Divides audio into `slices` equal parts and keeps part `select_slice` |
| `crop` | `from` (s), `to` (s) | Keeps only the region `[from, to]` |

All processors work on **interleaved f32** samples: `[L0, R0, L1, R1, …]` for stereo, `[S0, S1, …]` for mono.

## Crate Layout

```
src/
  lib.rs               — WASM entry point; calls implement_sp_chain!
  main.rs              — Native CLI binary
  processors/
    mod.rs
    trim.rs
    cut.rs
    slice.rs
    crop.rs
```

## Native CLI Binary

```bash
# Print processor descriptors as JSON (used by build.sh)
structural-processor --descriptors

# Apply an edit chain to a WAV file
structural-processor --input in.wav --output out.wav --edits '[
  {"id":"1","type":"trim","enabled":true,"parameters":{"start":1.0,"end":0.5}}
]'
```

## Dependency

This crate depends on `structural-processor-sdk` (the shared trait and macro). See its [README](../structural-processor-sdk/README.md) for the trait contract and WASM ABI details.

The JS SDK that consumes the WASM output lives in [`structural-processor-sdk/js/`](../structural-processor-sdk/js/README.md).

## Testing

```bash
cargo test
```

Each processor module contains unit tests covering validation, `apply`, and both time-mapping directions. Chain-level tests live in `lib.rs`.

## Adding a New Processor

1. Create `src/processors/<name>.rs` and implement `StructuralProcessor`.
2. Register it in `src/processors/mod.rs`.
3. Add it to the `implement_sp_chain!` call in `src/lib.rs`.
4. Run `build.sh` to regenerate the WASM binary and descriptor JSON.
