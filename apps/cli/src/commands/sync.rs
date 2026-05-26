use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

use crate::settings::AppSettings;

pub async fn run(db: &DatabaseConnection, settings: &AppSettings) -> Result<()> {
    println!("Syncing library: {}", settings.library_dir);

    let total = sync_service::count_audio_files(&settings.library_dir).unwrap_or(0);

    let pb = if total > 0 {
        let bar = ProgressBar::new(total as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "  {bar:40.cyan/blue} {pos}/{len}  {elapsed_precise}"
            )
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        bar
    } else {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} scanning…  {elapsed_precise}")
                .unwrap(),
        );
        bar
    };

    let pb_tick = pb.clone();
    let report = sync_service::sync_library(
        db,
        &settings.library_dir,
        move || pb_tick.inc(1),
    )
    .await?;

    pb.finish_and_clear();

    for name in &report.files_removed    { println!("  [removed] {name}"); }
    for name in &report.files_updated    { println!("  [updated] {name}"); }
    for name in &report.files_added      { println!("  [new]     {name}"); }
    for name in &report.sidecars_updated { println!("  [sidecar] {name}"); }
    for name in &report.presets_added    { println!("  [preset]  {name} (new)"); }
    for name in &report.presets_updated  { println!("  [preset]  {name} (updated)"); }

    let fa = report.files_added.len();
    let fu = report.files_updated.len();
    let fr = report.files_removed.len();
    let su = report.sidecars_updated.len();
    let pt = report.presets_added.len() + report.presets_updated.len();

    let mut parts: Vec<String> = Vec::new();
    if fa > 0 { parts.push(format!("{fa} added")); }
    if fu > 0 { parts.push(format!("{fu} updated")); }
    if fr > 0 { parts.push(format!("{fr} removed")); }
    if su > 0 { parts.push(format!("{su} sidecar")); }
    if pt > 0 { parts.push(format!("{pt} {}", if pt == 1 { "preset" } else { "presets" })); }

    if parts.is_empty() {
        println!("Done — nothing changed");
    } else {
        println!("Done — {}", parts.join(", "));
    }

    Ok(())
}
