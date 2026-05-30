use std::io::Write;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use musicum_core::config::LibraryPaths;
use musicum_core::services::sync_service;
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, paths: &LibraryPaths, force: bool) -> Result<()> {
    println!("Syncing library: {}", paths.library_dir.display());

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.cyan} {pos} files scanned  {elapsed_precise}")
            .unwrap(),
    );

    let pb_tick = pb.clone();
    let report = sync_service::sync_library(db, paths, move || pb_tick.inc(1)).await?;

    pb.finish_and_clear();

    // ── Handle unresolvable orphaned sidecars ─────────────────────────────
    let mut cleaned = 0usize;
    for orphan in &report.orphaned_sidecars {
        let remove = if force {
            true
        } else {
            print!(
                "  Orphaned sidecar: \"{}\" (no matching audio found)\n  Remove sidecar + DB entry? [y/N] ",
                orphan.name
            );
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        };

        if remove {
            sync_service::remove_orphaned_sidecar(db, &orphan.sidecar_path, &orphan.db_id)
                .await?;
            println!("  [cleaned]  {}", orphan.name);
            cleaned += 1;
        } else {
            println!("  [skipped]  {} (orphaned sidecar kept)", orphan.name);
        }
    }

    // ── Print change lines ────────────────────────────────────────────────
    for name  in &report.files_removed    { println!("  [removed]  {name}"); }
    for name  in &report.files_updated    { println!("  [updated]  {name}"); }
    for name  in &report.files_added      { println!("  [new]      {name}"); }
    for name  in &report.sidecars_updated { println!("  [sidecar]  {name}"); }
    for entry in &report.files_repaired   { println!("  [repaired] {entry}"); }

    // ── Summary line ──────────────────────────────────────────────────────
    let fa = report.files_added.len();
    let fu = report.files_updated.len();
    let fr = report.files_removed.len();
    let su = report.sidecars_updated.len();
    let rp = report.files_repaired.len();

    let mut parts: Vec<String> = Vec::new();
    if fa > 0 { parts.push(format!("{fa} added")); }
    if fu > 0 { parts.push(format!("{fu} updated")); }
    if fr > 0 { parts.push(format!("{fr} removed")); }
    if su > 0 { parts.push(format!("{su} sidecar")); }
    if rp > 0 { parts.push(format!("{rp} repaired")); }
    if cleaned > 0 { parts.push(format!("{cleaned} cleaned")); }

    if parts.is_empty() {
        println!("Done — nothing changed");
    } else {
        println!("Done — {}", parts.join(", "));
    }

    Ok(())
}
