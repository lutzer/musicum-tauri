use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::{clip_service, file_service};
use sea_orm::DatabaseConnection;
use std::collections::HashMap;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct ClipsArgs {
    #[command(subcommand)]
    pub command: ClipsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClipsCommand {
    /// List clips — all clips, or only for a specific file if FILE_SLUG is given
    List {
        file_slug: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show detail for one clip
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a new clip for a file
    Create {
        file_slug: String,
        title: String,
    },
}

pub async fn run(db: &DatabaseConnection, args: ClipsArgs) -> Result<()> {
    match args.command {
        ClipsCommand::List { file_slug, json } => {
            if let Some(slug) = file_slug {
                let file = file_service::get_file_by_slug(db, &slug).await?;
                let clips = clip_service::list_clips_for_file(db, &file.id).await?;

                if json {
                    print_json(&clips);
                } else if clips.is_empty() {
                    println!("No clips for '{}'. Sync or create clips via sidecar.", slug);
                } else {
                    print_table(
                        ("SLUG", "TITLE  [CACHED]"),
                        clips.iter().map(|c| (c.slug.clone(), format!("{}  [{}]", c.title, c.cached))).collect(),
                    );
                }
            } else {
                let clips = clip_service::list_all_clips(db).await?;

                if json {
                    print_json(&clips);
                } else if clips.is_empty() {
                    println!("No clips found. Sync your library or create clips with `clips create`.");
                } else {
                    let files = file_service::list_files(db).await?;
                    let file_slugs: HashMap<String, String> =
                        files.into_iter().map(|f| (f.id, f.slug)).collect();
                    print_table(
                        ("SLUG", "FILE  TITLE  [CACHED]"),
                        clips.iter().map(|c| {
                            let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
                            (c.slug.clone(), format!("{}  {}  [{}]", file_slug, c.title, c.cached))
                        }).collect(),
                    );
                }
            }
        }
        ClipsCommand::Show { slug, json } => {
            let clip = clip_service::get_clip_by_slug(db, &slug).await?;

            if json {
                print_json(&clip);
            } else {
                let processors: serde_json::Value =
                    serde_json::from_str(&clip.processors).unwrap_or(serde_json::json!([]));
                print_detail(vec![
                    ("slug", clip.slug.clone()),
                    ("title", clip.title.clone()),
                    ("file_id", clip.file_id.clone()),
                    ("cached", clip.cached.clone()),
                    (
                        "cached_path",
                        clip.cached_path.clone().unwrap_or_else(|| "-".into()),
                    ),
                    (
                        "duration",
                        clip.duration
                            .map_or("-".into(), |d| format!("{d:.3}s")),
                    ),
                    ("processors", serde_json::to_string_pretty(&processors).unwrap()),
                    ("notes", if clip.notes.is_empty() { "-".into() } else { clip.notes.clone() }),
                ]);
            }
        }
        ClipsCommand::Create { file_slug, title } => {
            let clip = clip_service::create_clip(db, &file_slug, &title).await?;
            println!("Created clip '{}' for file '{}'", clip.slug, file_slug);
        }
    }
    Ok(())
}
