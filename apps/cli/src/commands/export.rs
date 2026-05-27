use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::Args;
use musicum_core::{
    audio::sidecar_entries_to_edits,
    services::{
        clip_service, file_service,
        export_service::{export_audio, ExportOptions},
    },
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
