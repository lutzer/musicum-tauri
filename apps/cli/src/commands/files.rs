use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::{clip_service, file_service};
use sea_orm::DatabaseConnection;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct FilesArgs {
    #[command(subcommand)]
    pub command: FilesCommand,
}

#[derive(Debug, Subcommand)]
pub enum FilesCommand {
    /// List all files in the library
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show detail for one file including its clips
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(db: &DatabaseConnection, args: FilesArgs) -> Result<()> {
    match args.command {
        FilesCommand::List { json } => {
            let files = file_service::list_files(db).await?;
            if json {
                print_json(&files);
            } else if files.is_empty() {
                println!("No files. Run `musicum sync` first.");
            } else {
                print_table(
                    ("SLUG", "NAME  [DURATION  RATE  CH]"),
                    files
                        .iter()
                        .map(|f| {
                            (
                                f.slug.clone(),
                                format!(
                                    "{}  [{:.1}s  {}Hz  {}ch]",
                                    f.name, f.duration, f.sample_rate, f.channels
                                ),
                            )
                        })
                        .collect(),
                );
            }
        }
        FilesCommand::Show { slug, json } => {
            let file = file_service::get_file_by_slug(db, &slug).await?;
            let meta = file_service::get_file_metadata(db, &file.id).await?;
            let clips = clip_service::list_clips_for_file(db, &file.id).await?;

            if json {
                #[derive(serde::Serialize)]
                struct FileDetail {
                    file: musicum_core::db::entities::file::Model,
                    metadata: Option<musicum_core::db::entities::file_metadata::Model>,
                    clips: Vec<musicum_core::db::entities::clip::Model>,
                }
                print_json(&FileDetail { file, metadata: meta, clips });
            } else {
                print_detail(vec![
                    ("slug", file.slug.clone()),
                    ("name", file.name.clone()),
                    ("path", file.path.clone()),
                    ("duration", format!("{:.3}s", file.duration)),
                    ("sample_rate", format!("{}Hz", file.sample_rate)),
                    ("channels", file.channels.to_string()),
                    ("mime_type", file.mime_type.clone()),
                    ("hash", file.hash[..16].to_string() + "..."),
                ]);

                if let Some(m) = &meta {
                    println!();
                    print_detail(vec![
                        ("bpm", m.bpm.map_or("-".into(), |v| v.to_string())),
                        ("key", m.key.clone().unwrap_or_else(|| "-".into())),
                        ("rating", m.rating.map_or("-".into(), |v| v.to_string())),
                        ("tags", if m.tags.is_empty() { "-".into() } else { m.tags.clone() }),
                        ("notes", if m.notes.is_empty() { "-".into() } else { m.notes.clone() }),
                    ]);
                }

                if !clips.is_empty() {
                    println!("\nClips:");
                    print_table(
                        ("SLUG", "TITLE  [CACHED]"),
                        clips
                            .iter()
                            .map(|c| {
                                (c.slug.clone(), format!("{}  [{}]", c.title, c.cached))
                            })
                            .collect(),
                    );
                }
            }
        }
    }
    Ok(())
}
