# Export Command Design

**Date:** 2026-05-27  
**Status:** Approved

## Overview

Add a top-level `export` CLI command that renders a file or clip to an audio file on disk. When exporting a file, the raw audio is written unchanged. When exporting a clip, the clip's structural processor chain is applied before encoding. Format is inferred from the output path extension. All encoding is delegated to the ffmpeg system binary (consistent with the existing clip-caching pattern).

---

## CLI Interface

```
musicum export <slug> <output_path> [OPTIONS]

ARGS:
  <slug>          File or clip slug to export. Auto-detects: tries file slug
                  first, then clip slug. Identical to `play` resolution.
  <output_path>   Destination file path. Format inferred from extension.

OPTIONS:
  --file              Force slug to resolve as a file (no processors applied)
  --clip              Force slug to resolve as a clip (processors applied)
  --samplerate <hz>   Resample output to this sample rate (e.g. 44100, 48000).
                      Omit to use the source file's native rate.
  --channels <n>      Remix to this channel count (1 = mono, 2 = stereo).
                      Omit to use the source file's native channel count.
  --bitrate <kbps>    Target bitrate for lossy formats (e.g. 192 for MP3).
                      Silently ignored for lossless formats (WAV, FLAC, AIFF).
  --overwrite         Overwrite output file if it already exists.
                      Without this flag, export aborts with an error if the
                      output path already exists.
```

**Supported output extensions:** `.wav`, `.mp3`, `.flac`, `.aiff`, `.aif`  
Any other extension produces an immediate error listing the supported ones.

---

## Resolution Logic

Identical to the `play` command's `resolve_target`:

- `--file` → look up by file slug; apply no edits.
- `--clip` → look up by clip slug; apply structural processor edits from the clip.
- Neither flag → try file slug first; if not found, try clip slug; if not found, return error.

**Processor filtering:** only `ProcessorEntry::Structural` entries are included in the edit chain. `AudioPlugin` entries are skipped (same behaviour as `play`).

The shared helper `sidecar_entries_to_edits` is extracted from `play.rs` into `musicum-core` (see Code Layout) so both commands use the same implementation.

---

## Export Pipeline

Executed in `export_service::export_audio`:

1. **Resolve slug** → `(file_path: PathBuf, edits: Vec<Edit>)`  
   Returned by the CLI layer (mirrors `play.rs`).

2. **Check output path**  
   If `output_path` already exists and `options.overwrite` is `false`, return an error immediately. If `overwrite` is `true`, ffmpeg will overwrite via `-y`.

3. **Validate extension**  
   Extract the file extension from `output_path`. If it is not one of the supported values, return a descriptive error.

4. **Build audio chain**  
   ```rust
   let source = Box::new(FileAudioSource::new(&file_path)?);
   let registry = structural_processors::registry();
   let chain = build_chain(source, &edits, &registry);
   let src_rate = chain.sample_rate();
   let src_channels = chain.channels();
   let total_duration = chain.duration_secs();
   ```

5. **Drain samples**  
   Read the chain in `CHUNK_SAMPLES`-sized chunks, advancing `cursor_secs` after each read, until the chain returns an empty vec or `cursor_secs >= total_duration`. Collect all `f32` samples into a single `Vec<f32>`.

6. **Write temp PCM file**  
   Create a uniquely named file in `std::env::temp_dir()` (e.g. `musicum-export-<uuid>.pcm`). Write every `f32` sample as 4 bytes little-endian (`f.to_le_bytes()`).

7. **Invoke ffmpeg**  
   ```
   ffmpeg [-y]
     -f f32le
     -ar <src_rate>
     -ac <src_channels>
     -i <tmp_path>
     [-ar <target_samplerate>]     ← only if --samplerate supplied
     [-ac <target_channels>]       ← only if --channels supplied
     [-b:a <bitrate>k]             ← only if --bitrate supplied AND format is MP3
     <output_path>
   ```
   - ffmpeg stdout is suppressed. stderr is captured.
   - On non-zero exit code, the captured stderr is surfaced as the error message.
   - "ffmpeg not found" (OS error) produces a friendly message pointing to the system dependency requirement.

8. **Cleanup**  
   Delete the temp PCM file (best-effort; failure is silently ignored).

9. **Return result**  
   Return metadata for the CLI to display: output path, format, duration, effective sample rate, effective channels, bitrate (if applicable).

---

## ExportOptions

```rust
pub struct ExportOptions {
    pub sample_rate:  Option<u32>,   // None → use source rate
    pub channels:     Option<u16>,   // None → use source channels
    pub bitrate_kbps: Option<u32>,   // None or lossless format → omit -b:a
    pub overwrite:    bool,
}
```

---

## Output

Before calling the service, the CLI prints a single progress line:

```
Exporting <slug> → <output_path>...
```

On success, `print_result` is called with:

```
Exported
    slug: my-clip
  output: /path/to/output.mp3
  format: mp3
duration: 3.240s
    rate: 44100Hz
channels: 2
 bitrate: 192kbps        ← omitted for lossless formats
```

`print_result` and `DetailItem::Field` are reused unchanged from `output.rs`.

---

## Error Cases

| Condition | Error message |
|---|---|
| Output file exists, no `--overwrite` | `"output file already exists: <path>. Use --overwrite to replace it."` |
| Unsupported extension | `"unsupported output format '<ext>'. Supported: wav, mp3, flac, aiff"` |
| Slug not found | `"'<slug>' is not a known file or clip slug"` |
| ffmpeg not installed | `"ffmpeg not found. Install ffmpeg to use the export command."` |
| ffmpeg exits non-zero | ffmpeg's captured stderr, prefixed with `"ffmpeg error: "` |

---

## Code Layout

| Path | Responsibility |
|---|---|
| `apps/cli/src/commands/export.rs` | `ExportArgs` (clap struct), `run()` — resolves slug, prints progress line, calls service, prints result |
| `apps/cli/src/main.rs` | Add `Export` variant to `Commands` enum; wire to `commands::export::run()` |
| `libs/musicum-core/src/services/export_service.rs` | `export_audio(file_path, edits, output_path, options) -> Result<ExportResult>` — steps 2–9 above |
| `libs/musicum-core/src/services/mod.rs` | `pub mod export_service;` |
| `libs/musicum-core/src/audio/mod.rs` | Extract `sidecar_entries_to_edits` here (currently private in `play.rs`); re-export so both `play.rs` and `export.rs` can use it |

No new crate dependencies are required.

---

## Constraints & Non-Goals

- **ffmpeg required.** All four formats depend on ffmpeg. The command fails gracefully with a clear message if ffmpeg is absent.
- **Structural processors only.** Audio plugins are not applied during export (same as `play`).
- **No audio plugin export.** Out of scope; can be added later.
- **No progress bar.** The pre-export `println!` is sufficient for v1. A spinner (`indicatif`) can be added later if large-file exports prove slow in practice.
- **No streaming write.** All samples are collected in memory before writing the temp file. Suitable for typical clip/sample lengths; revisit if multi-minute full-file exports become common.
