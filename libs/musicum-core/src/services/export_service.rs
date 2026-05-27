use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use structural_processor_sdk::chain::{build_chain, StructuralEdit};
use uuid::Uuid;

use crate::audio::FileAudioSource;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct ExportOptions {
    pub sample_rate:  Option<u32>,
    pub channels:     Option<u16>,
    pub bitrate_kbps: Option<u32>,
    pub overwrite:    bool,
}

#[derive(Debug)]
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

const CHUNK_SAMPLES: usize = 4_096;

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
        bail!("unsupported output format '{ext}'. Supported: wav, mp3, flac, aiff")
    }
}

// ── ffmpeg helper ─────────────────────────────────────────────────────────────

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

// ── Main entry point ──────────────────────────────────────────────────────────

pub async fn export_audio(
    file_path: &Path,
    edits: &[StructuralEdit],
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
