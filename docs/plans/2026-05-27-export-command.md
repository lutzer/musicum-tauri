# Export Command Implementation Plan

**Goal:** Add a `musicum export <slug> <output_path>` CLI command that renders a file or clip to a WAV/MP3/FLAC/AIFF audio file via ffmpeg.

**Architecture:** The slug-resolution and edit-building logic is extracted from `play.rs` into `musicum-core/src/audio/mod.rs` so both commands share it. A new `export_service` in musicum-core handles steps 2–9 (validation → chain build → PCM drain → temp file → ffmpeg → cleanup → result). The CLI command in `apps/cli/src/commands/export.rs` is display-only: resolve slug, print progress, call service, print result.

**Tech Stack:** Rust, clap 4 (derive), anyhow, structural-processor-sdk (`build_chain`, `chain_output_duration`), uuid v4 (temp filename), `std::process::Command` (ffmpeg), existing `musicum-core` audio primitives.

---

## File Map

| Status | Path | Responsibility |
|--------|------|---------------|
| Modify | `libs/musicum-core/src/audio/mod.rs` | Add `pub fn sidecar_entries_to_edits` (extracted from `play.rs`); re-export it |
| Modify | `apps/cli/src/commands/play.rs` | Remove local `sidecar_entries_to_edits`; import from musicum-core |
| **Create** | `libs/musicum-core/src/services/export_service.rs` | `ExportOptions`, `ExportResult`, `export_audio(...)` |
| Modify | `libs/musicum-core/src/services/mod.rs` | `pub mod export_service;` |
| **Create** | `apps/cli/src/commands/export.rs` | `ExportArgs` (clap), `run()` |
| Modify | `apps/cli/src/commands/mod.rs` | `pub mod export;` |
| Modify | `apps/cli/src/main.rs` | `Commands::Export` variant + dispatch |

---

## Task 1: Extract `sidecar_entries_to_edits` into musicum-core

This function converts sidecar processor entries into the `Vec<Edit>` that the audio chain needs. It currently lives privately in `play.rs`. Both `play.rs` and the new `export.rs` need it, so it moves to the core library.

**Files:**
- Modify: `libs/musicum-core/src/audio/mod.rs`
- Modify: `apps/cli/src/commands/play.rs`

### Step 1.1 — Add the function to `audio/mod.rs`

Open `libs/musicum-core/src/audio/mod.rs`. Add the function and its imports **before** the existing `pub use` lines:

```rust
pub mod player;
pub mod source;

use crate::sidecar::ProcessorEntry;
use structural_processor_sdk::chain::Edit;

pub use player::PlaybackEngine;
pub use source::FileAudioSource;

/// Convert sidecar [`ProcessorEntry`] items into [`Edit`]s for the audio chain.
/// `AudioPlugin` entries are filtered out — only `Structural` entries are used.
pub fn sidecar_entries_to_edits(entries: &[ProcessorEntry]) -> Vec<Edit> {
    entries
        .iter()
        .filter_map(|e| {
            if let ProcessorEntry::Structural { enabled, processor, .. } = e {
                let params = processor
                    .params
                    .as_object()
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Edit { processor_id: processor.id.clone(), enabled: *enabled, params })
            } else {
                None
            }
        })
        .collect()
}
```

### Step 1.2 — Remove the duplicate from `play.rs`

In `apps/cli/src/commands/play.rs`, delete lines 24–44 (the local `fn sidecar_entries_to_edits`). Then add the import at the top with the other `musicum_core` imports:

```rust
use musicum_core::{
    audio::{sidecar_entries_to_edits, PlaybackEngine},
    services::{clip_service, file_service},
    sidecar::ProcessorEntry,
};
```

Also remove the now-unused `use musicum_core::sidecar::ProcessorEntry;` line (it's merged above) and the `use structural_processor_sdk::chain::Edit;` import (no longer needed directly in play.rs since `Edit` is only used via `sidecar_entries_to_edits`).

### Step 1.3 — Verify it compiles

```bash
cargo clippy -p musicum-core -p musicum-cli --all 2>&1 | head -30
```

Expected: zero errors. Warnings about unused imports are acceptable temporarily.

---

## Task 2: Add `ExportOptions` and `ExportResult` to the core service

**Files:**
- Create: `libs/musicum-core/src/services/export_service.rs`
- Modify: `libs/musicum-core/src/services/mod.rs`

### Step 2.1 — Add `pub mod export_service;` to the module list

Open `libs/musicum-core/src/services/mod.rs`. Append:

```rust
pub mod export_service;
```

### Step 2.2 — Create the service file with data types only

Create `libs/musicum-core/src/services/export_service.rs`:

```rust
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use structural_processor_sdk::chain::Edit;

// ── Public types ──────────────────────────────────────────────────────────────

pub struct ExportOptions {
    pub sample_rate:  Option<u32>,
    pub channels:     Option<u16>,
    pub bitrate_kbps: Option<u32>,
    pub overwrite:    bool,
}

pub struct ExportResult {
    pub output_path: PathBuf,
    pub format:      String,
    pub duration:    f64,
    pub sample_rate: u32,
    pub channels:    u16,
    pub bitrate_kbps: Option<u32>,
}

// ── Supported formats ─────────────────────────────────────────────────────────

const SUPPORTED_EXTS: &[&str] = &["wav", "mp3", "flac", "aiff", "aif"];

fn is_lossless(ext: &str) -> bool {
    matches!(ext, "wav" | "flac" | "aiff" | "aif")
}

fn validate_extension(output_path: &Path) -> Result<String> {
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if SUPPORTED_EXTS.contains(&ext.as_str()) {
        Ok(ext)
    } else {
        bail!(
            "unsupported output format '{}'. Supported: wav, mp3, flac, aiff",
            ext
        )
    }
}

// ── Main entry point (stub — filled in Task 4) ───────────────────────────────

pub async fn export_audio(
    _file_path: &Path,
    _edits: &[Edit],
    _output_path: &Path,
    _options: ExportOptions,
) -> Result<ExportResult> {
    todo!("implemented in Task 4")
}
```

### Step 2.3 — Write unit tests for validation helpers

Append a `#[cfg(test)]` block to `export_service.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn validate_extension_rejects_unknown() {
        let err = validate_extension(Path::new("/out/file.ogg")).unwrap_err();
        assert!(err.to_string().contains("unsupported output format"));
        assert!(err.to_string().contains("ogg"));
        assert!(err.to_string().contains("wav, mp3, flac, aiff"));
    }

    #[test]
    fn validate_extension_accepts_all_supported() {
        for ext in &["wav", "mp3", "flac", "aiff", "aif"] {
            let path = Path::new("/out/file").with_extension(ext);
            assert!(
                validate_extension(&path).is_ok(),
                "should accept .{ext}"
            );
        }
    }

    #[test]
    fn is_lossless_mp3_is_false() {
        assert!(!is_lossless("mp3"));
    }

    #[test]
    fn is_lossless_wav_flac_aiff_are_true() {
        assert!(is_lossless("wav"));
        assert!(is_lossless("flac"));
        assert!(is_lossless("aiff"));
        assert!(is_lossless("aif"));
    }
}
```

### Step 2.4 — Run the tests (they should pass now)

```bash
cargo test -p musicum-core -- export_service 2>&1
```

Expected output:
```
test services::export_service::tests::validate_extension_accepts_all_supported ... ok
test services::export_service::tests::validate_extension_rejects_unknown ... ok
test services::export_service::tests::is_lossless_mp3_is_false ... ok
test services::export_service::tests::is_lossless_wav_flac_aiff_are_true ... ok
```

---

## Task 3: Implement `export_audio` — validation phase

Replace the stub `export_audio` with the real implementation, one logical block at a time.

**Files:**
- Modify: `libs/musicum-core/src/services/export_service.rs`

### Step 3.1 — Write failing tests for the overwrite and file-exists guard

Add to the `tests` module in `export_service.rs`:

```rust
    #[tokio::test]
    async fn export_fails_if_output_exists_and_no_overwrite() {
        use tempfile::NamedTempFile;
        // Create a real file at the output path so the check fires.
        let tmp = NamedTempFile::new().unwrap();
        let out_path = tmp.path().with_extension("wav");
        std::fs::write(&out_path, b"dummy").unwrap();

        let opts = ExportOptions {
            sample_rate: None,
            channels: None,
            bitrate_kbps: None,
            overwrite: false,
        };
        let result = export_audio(
            Path::new("/nonexistent/source.wav"),
            &[],
            &out_path,
            opts,
        ).await;

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already exists"));
        assert!(msg.contains("--overwrite"));

        let _ = std::fs::remove_file(&out_path);
    }
```

### Step 3.2 — Run the test to confirm it fails (panics on `todo!`)

```bash
cargo test -p musicum-core -- export_audio 2>&1 | tail -10
```

Expected: the test panics at `todo!("implemented in Task 4")`. That's the expected red state.

### Step 3.3 — Replace the stub with the validation body

Replace the `export_audio` stub in `export_service.rs` with the full validation phase (steps 2–3 from the spec). Keep the rest as `todo!` for now:

```rust
pub async fn export_audio(
    file_path: &Path,
    edits: &[Edit],
    output_path: &Path,
    options: ExportOptions,
) -> Result<ExportResult> {
    // ── Step 2: Check output path ─────────────────────────────────────────
    if output_path.exists() && !options.overwrite {
        bail!(
            "output file already exists: {}. Use --overwrite to replace it.",
            output_path.display()
        );
    }

    // ── Step 3: Validate extension ────────────────────────────────────────
    let ext = validate_extension(output_path)?;

    todo!("audio pipeline — Task 4")
}
```

> **Note:** `file_path`, `edits`, and `ext` will be used in Task 4; suppress unused-variable warnings temporarily with `let _ext = ext;` if clippy complains. We'll remove that in Task 4.

### Step 3.4 — Run the validation tests

```bash
cargo test -p musicum-core -- export_service 2>&1
```

Expected: `export_fails_if_output_exists_and_no_overwrite` now passes (the other tests from Task 2 still pass).

---

## Task 4: Implement the audio pipeline, PCM temp file, and ffmpeg call

This is the core of the service. Replace `todo!("audio pipeline — Task 4")`.

**Files:**
- Modify: `libs/musicum-core/src/services/export_service.rs`

### Step 4.1 — Add the required imports at the top of the file

```rust
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use structural_processor_sdk::chain::{build_chain, Edit};
use uuid::Uuid;

use crate::audio::FileAudioSource;
```

> Note: `sidecar_entries_to_edits` lives in `musicum-core::audio` but is called by the CLI layer before passing `edits` to `export_audio` — no need to import it here. `chain_output_duration` is not needed either since we use `chain.duration_secs()` on the built chain directly.

### Step 4.2 — Add the `CHUNK_SAMPLES` constant

```rust
const CHUNK_SAMPLES: usize = 4_096;
```

Place it near the top of the file, after imports.

### Step 4.3 — Replace `todo!("audio pipeline — Task 4")` with the full body

```rust
    // ── Step 4: Build audio chain ─────────────────────────────────────────
    let source = Box::new(
        FileAudioSource::new(file_path)
            .with_context(|| format!("cannot open source file: {}", file_path.display()))?,
    );
    let registry = structural_processors::registry();
    let mut chain = build_chain(source, edits, &registry);

    let src_rate     = chain.sample_rate();
    let src_channels = chain.channels();
    let total_duration = chain.duration_secs();

    // ── Step 5: Drain samples ─────────────────────────────────────────────
    let mut all_samples: Vec<f32> = Vec::new();
    let mut cursor_secs = 0.0_f64;
    loop {
        let chunk = chain.read_at(cursor_secs, CHUNK_SAMPLES);
        if chunk.is_empty() || cursor_secs >= total_duration {
            break;
        }
        cursor_secs += chunk.len() as f64 / (src_rate as f64 * src_channels as f64);
        all_samples.extend_from_slice(&chunk);
    }

    // ── Step 6: Write temp PCM file ───────────────────────────────────────
    let tmp_path = std::env::temp_dir().join(format!("musicum-export-{}.pcm", Uuid::new_v4()));
    {
        let mut f = std::fs::File::create(&tmp_path)
            .context("failed to create temp PCM file")?;
        for s in &all_samples {
            f.write_all(&s.to_le_bytes())
                .context("failed to write temp PCM file")?;
        }
    }

    // ── Step 7: Invoke ffmpeg ─────────────────────────────────────────────
    let ffmpeg_result = invoke_ffmpeg(
        &tmp_path,
        output_path,
        src_rate,
        src_channels,
        &ext,
        &options,
    );

    // ── Step 8: Cleanup (best-effort) ─────────────────────────────────────
    let _ = std::fs::remove_file(&tmp_path);

    // ── Step 9: Return result ─────────────────────────────────────────────
    ffmpeg_result?;

    let effective_rate     = options.sample_rate.unwrap_or(src_rate);
    let effective_channels = options.channels.unwrap_or(src_channels);
    let bitrate = if is_lossless(&ext) { None } else { options.bitrate_kbps };

    Ok(ExportResult {
        output_path: output_path.to_path_buf(),
        format: ext,
        duration: total_duration,
        sample_rate: effective_rate,
        channels: effective_channels,
        bitrate_kbps: bitrate,
    })
}
```

### Step 4.4 — Add the `invoke_ffmpeg` helper function

Add this **above** `export_audio` (or below it — just not inside the test module):

```rust
fn invoke_ffmpeg(
    tmp_path: &Path,
    output_path: &Path,
    src_rate: u32,
    src_channels: u16,
    ext: &str,
    options: &ExportOptions,
) -> Result<()> {
    let mut cmd = Command::new("ffmpeg");

    if options.overwrite {
        cmd.arg("-y");
    }

    cmd.args(["-f", "f32le"])
        .arg("-ar").arg(src_rate.to_string())
        .arg("-ac").arg(src_channels.to_string())
        .arg("-i").arg(tmp_path);

    if let Some(rate) = options.sample_rate {
        cmd.arg("-ar").arg(rate.to_string());
    }
    if let Some(ch) = options.channels {
        cmd.arg("-ac").arg(ch.to_string());
    }
    if let Some(kbps) = options.bitrate_kbps {
        if !is_lossless(ext) {
            cmd.arg("-b:a").arg(format!("{kbps}k"));
        }
    }

    cmd.arg(output_path);

    // Suppress stdout; capture stderr for error messages.
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("ffmpeg not found. Install ffmpeg to use the export command.")
        } else {
            anyhow::anyhow!("failed to run ffmpeg: {e}")
        }
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg error: {stderr}")
    }
}
```

### Step 4.5 — Add `structural_processors` to the import

`structural_processors` is used via `structural_processors::registry()`. It's already a dependency of musicum-core (see `Cargo.toml`). Add:

```rust
use structural_processor_sdk::chain::{build_chain, Edit};
```

(Remove `chain_output_duration` if not used directly in export_service.)

### Step 4.6 — Compile and run existing tests

```bash
cargo test -p musicum-core -- export_service 2>&1
```

Expected: all 5 tests pass. The `export_audio` stub tests now exercise real code paths up to the point where ffmpeg would be called.

---

## Task 5: Create the CLI `export` command

**Files:**
- Create: `apps/cli/src/commands/export.rs`
- Modify: `apps/cli/src/commands/mod.rs`

### Step 5.1 — Add `pub mod export;` to the commands module

Open `apps/cli/src/commands/mod.rs`. Append:

```rust
pub mod export;
```

### Step 5.2 — Create `export.rs`

Create `apps/cli/src/commands/export.rs` with the complete file contents below. This is the full module — write it all at once:

```rust
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Args;
use musicum_core::{
    audio::sidecar_entries_to_edits,
    services::{clip_service, export_service::{export_audio, ExportOptions}, file_service},
    sidecar::ProcessorEntry,
};
use sea_orm::DatabaseConnection;
use structural_processor_sdk::chain::Edit;

use crate::output::{DetailItem, print_result};

#[derive(Args)]
pub struct ExportArgs {
    /// File or clip slug to export (auto-detects file first, then clip)
    pub slug: String,

    /// Destination file path; format inferred from extension (.wav .mp3 .flac .aiff .aif)
    pub output: PathBuf,

    /// Resolve slug as a file (no processors applied)
    #[arg(long, conflicts_with = "clip")]
    pub file: bool,

    /// Resolve slug as a clip (processors applied)
    #[arg(long, conflicts_with = "file")]
    pub clip: bool,

    /// Resample output to this sample rate (e.g. 44100)
    #[arg(long)]
    pub samplerate: Option<u32>,

    /// Remix to this channel count (1=mono, 2=stereo)
    #[arg(long)]
    pub channels: Option<u16>,

    /// Target bitrate in kbps for lossy formats (e.g. 192); ignored for lossless
    #[arg(long)]
    pub bitrate: Option<u32>,

    /// Overwrite output file if it already exists
    #[arg(long)]
    pub overwrite: bool,
}

pub async fn run(db: &DatabaseConnection, args: ExportArgs) -> Result<()> {
    let (file_path, edits) = resolve_target(db, &args.slug, args.file, args.clip).await?;

    println!("Exporting {} → {}...", args.slug, args.output.display());

    let options = ExportOptions {
        sample_rate:  args.samplerate,
        channels:     args.channels,
        bitrate_kbps: args.bitrate,
        overwrite:    args.overwrite,
    };

    let result = export_audio(&file_path, &edits, &args.output, options).await?;

    let mut items = vec![
        DetailItem::Field("slug",     args.slug.clone()),
        DetailItem::Field("output",   result.output_path.display().to_string()),
        DetailItem::Field("format",   result.format.clone()),
        DetailItem::Field("duration", format!("{:.3}s", result.duration)),
        DetailItem::Field("rate",     format!("{}Hz", result.sample_rate)),
        DetailItem::Field("channels", result.channels.to_string()),
    ];
    if let Some(kbps) = result.bitrate_kbps {
        items.push(DetailItem::Field("bitrate", format!("{kbps}kbps")));
    }

    print_result("Exported", &items);
    Ok(())
}

async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<Edit>)> {
    if force_file {
        let file = file_service::get_file_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no file with slug '{target}'"))?;
        return Ok((PathBuf::from(file.path), vec![]));
    }

    if force_clip {
        let clip = clip_service::get_clip_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no clip with slug '{target}'"))?;
        let file = file_service::get_file_by_id(db, &clip.file_id)
            .await
            .map_err(|_| anyhow!("parent file for clip '{target}' not found"))?;
        let entries: Vec<ProcessorEntry> = serde_json::from_str(&clip.processors)
            .unwrap_or_default();
        return Ok((PathBuf::from(file.path), sidecar_entries_to_edits(&entries)));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok((PathBuf::from(file.path), vec![]));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            let entries: Vec<ProcessorEntry> = serde_json::from_str(&clip.processors)
                .unwrap_or_default();
            return Ok((PathBuf::from(file.path), sidecar_entries_to_edits(&entries)));
        }
    }

    Err(anyhow!("'{}' is not a known file or clip slug", target))
}
```

### Step 5.3 — Verify CLI compiles

```bash
cargo clippy -p musicum-cli 2>&1 | head -30
```

Expected: zero errors.

---

## Task 6: Wire `export` into `main.rs`

**Files:**
- Modify: `apps/cli/src/main.rs`

### Step 6.1 — Add the `Export` variant to `Commands`

Add after the `Play` block in the `Commands` enum:

```rust
    /// Export a file or clip to an audio file
    Export(commands::export::ExportArgs),
```

### Step 6.2 — Dispatch in `match cli.command`

Add after the `Commands::Play` arm in the `match`:

```rust
        Commands::Export(args) => commands::export::run(&db, args).await?,
```

### Step 6.3 — Verify the full binary compiles and shows help

```bash
cargo build -p musicum-cli 2>&1 | tail -5
```

Then:

```bash
cargo run -p musicum-cli -- export --help
```

Expected output:

```
Export a file or clip to an audio file

Usage: musicum export [OPTIONS] <SLUG> <OUTPUT>

Arguments:
  <SLUG>    File or clip slug to export (auto-detects file first, then clip)
  <OUTPUT>  Destination file path; format inferred from extension (.wav .mp3 .flac .aiff .aif)

Options:
      --file              Resolve slug as a file (no processors applied)
      --clip              Resolve slug as a clip (processors applied)
      --samplerate <HZ>   Resample output to this sample rate (e.g. 44100)
      --channels <N>      Remix to this channel count (1=mono, 2=stereo)
      --bitrate <KBPS>    Target bitrate in kbps for lossy formats (e.g. 192); ignored for lossless
      --overwrite         Overwrite output file if it already exists
  -h, --help              Print help
```

---

## Task 7: Smoke test end-to-end

### Step 7.1 — Sync the library and find a file slug

```bash
cargo run -p musicum-cli -- sync
cargo run -p musicum-cli -- files list
```

Note a file slug from the output (e.g. `my-kick`).

### Step 7.2 — Export to WAV

```bash
cargo run -p musicum-cli -- export my-kick /tmp/test-export.wav
```

Expected output:
```
Exporting my-kick → /tmp/test-export.wav...
Exported
    slug: my-kick
  output: /tmp/test-export.wav
  format: wav
duration: 1.234s
    rate: 44100Hz
channels: 2
```

Verify the file was created:

```bash
ls -lh /tmp/test-export.wav
```

### Step 7.3 — Export to MP3 with bitrate

```bash
cargo run -p musicum-cli -- export my-kick /tmp/test-export.mp3 --bitrate 192
```

Expected: same output structure, plus `bitrate: 192kbps`.

### Step 7.4 — Test error cases

**Overwrite guard:**
```bash
cargo run -p musicum-cli -- export my-kick /tmp/test-export.wav
```
Expected error:
```
Error: output file already exists: /tmp/test-export.wav. Use --overwrite to replace it.
```

**With --overwrite:**
```bash
cargo run -p musicum-cli -- export my-kick /tmp/test-export.wav --overwrite
```
Expected: succeeds.

**Bad extension:**
```bash
cargo run -p musicum-cli -- export my-kick /tmp/test.xyz
```
Expected error:
```
Error: unsupported output format 'xyz'. Supported: wav, mp3, flac, aiff
```

**Unknown slug:**
```bash
cargo run -p musicum-cli -- export nonexistent-slug /tmp/test.wav
```
Expected error:
```
Error: 'nonexistent-slug' is not a known file or clip slug
```

### Step 7.5 — Run all tests one final time

```bash
cargo test -p musicum-core 2>&1 | tail -10
```

Expected: all existing tests plus the new export_service tests pass.

```bash
cargo clippy --all 2>&1 | grep -c "^error"
```

Expected: `0`

---

## Checklist

- [ ] `sidecar_entries_to_edits` extracted to `musicum-core/src/audio/mod.rs`
- [ ] `play.rs` imports from core (no local duplicate)
- [ ] `export_service.rs` compiled with all validation + pipeline logic
- [ ] All 5 `export_service` unit tests pass
- [ ] `export.rs` CLI command compiles with correct clap types
- [ ] `main.rs` dispatches `Export` variant
- [ ] `musicum export --help` shows correct usage
- [ ] WAV export end-to-end works
- [ ] MP3 with `--bitrate` works
- [ ] Overwrite guard works
- [ ] Bad extension error works
- [ ] Unknown slug error works
- [ ] `cargo clippy --all` is clean
