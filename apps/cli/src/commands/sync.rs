use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use musicum_core::config::LibraryPaths;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, paths: &LibraryPaths) -> Result<()> {
    println!("Syncing library: {}", paths.library_dir.display());

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {pos} files scanned  {elapsed_precise}")
            .unwrap(),
    );

    let pb_tick = pb.clone();
    let report = sync_service::sync_library(db, paths, move || pb_tick.inc(1)).await?;

    pb.finish_and_clear();

    for name in &report.files_removed    { println!("  [removed] {name}"); }
    for name in &report.files_updated    { println!("  [updated] {name}"); }
    for name in &report.files_added      { println!("  [new]     {name}"); }
    for name in &report.sidecars_updated { println!("  [sidecar] {name}"); }

    let fa = report.files_added.len();
    let fu = report.files_updated.len();
    let fr = report.files_removed.len();
    let su = report.sidecars_updated.len();

    let mut parts: Vec<String> = Vec::new();
    if fa > 0 { parts.push(format!("{fa} added")); }
    if fu > 0 { parts.push(format!("{fu} updated")); }
    if fr > 0 { parts.push(format!("{fr} removed")); }
    if su > 0 { parts.push(format!("{su} sidecar")); }

    if parts.is_empty() {
        println!("Done — nothing changed");
    } else {
        println!("Done — {}", parts.join(", "));
    }

    Ok(())
}
