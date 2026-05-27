use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::{clip_service, file_service};
use sea_orm::DatabaseConnection;

use crate::output::{DetailItem::{self, Field, Section}, print_detail, print_json, print_result, print_table};

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
    /// Set notes for a file (full replace)
    SetNotes {
        slug: String,
        notes: String,
    },
    /// Set tags for a file (full replace, comma-separated string)
    SetTags {
        slug: String,
        tags: String,
    },
    /// Delete a file from DB and remove its sidecar
    Delete {
        slug: String,
        /// Also delete the audio file from disk
        #[arg(long)]
        delete_audio: bool,
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
                    "files",
                    &["SLUG", "NAME  [DURATION  RATE  CH]"],
                    files
                        .iter()
                        .map(|f| vec![
                            f.slug.clone(),
                            format!("{}  [{:.1}s  {}Hz  {}ch]", f.name, f.duration, f.sample_rate, f.channels),
                        ])
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
                let mut items: Vec<DetailItem> = vec![
                    Section("file"),
                    Field("slug", file.slug.clone()),
                    Field("name", file.name.clone()),
                    Field("path", file.path.clone()),
                    Field("duration", format!("{:.3}s", file.duration)),
                    Field("sample_rate", format!("{}Hz", file.sample_rate)),
                    Field("channels", file.channels.to_string()),
                    Field("mime_type", file.mime_type.clone()),
                    Field("hash", file.hash[..16].to_string() + "..."),
                ];
                if let Some(m) = &meta {
                    items.push(Section("metadata"));
                    items.push(Field("bpm", m.bpm.map_or("-".into(), |v| v.to_string())));
                    items.push(Field("key", m.key.clone().unwrap_or_else(|| "-".into())));
                    items.push(Field("rating", m.rating.map_or("-".into(), |v| v.to_string())));
                    items.push(Field("tags", if m.tags.is_empty() { "-".into() } else { m.tags.clone() }));
                    items.push(Field("notes", if m.notes.is_empty() { "-".into() } else { m.notes.clone() }));
                }
                print_detail(&items);

                if !clips.is_empty() {
                    print_table(
                        "clips",
                        &["SLUG", "TITLE"],
                        clips
                            .iter()
                            .map(|c| vec![c.slug.clone(), c.title.clone()])
                            .collect(),
                    );
                }
            }
        }
        FilesCommand::SetNotes { slug, notes } => {
            file_service::set_file_notes(db, &slug, &notes).await?;
            print_result("Set notes", &[Field("file", slug.clone())]);
        }
        FilesCommand::SetTags { slug, tags } => {
            file_service::set_file_tags(db, &slug, &tags).await?;
            print_result("Set tags", &[Field("file", slug.clone())]);
        }
        FilesCommand::Delete { slug, delete_audio } => {
            let clip_count = file_service::delete_file(db, &slug, delete_audio).await?;
            print_result("Deleted file", &[
                Field("slug", slug.clone()),
                Field("clips", clip_count.to_string()),
                Field("audio", if delete_audio { "deleted" } else { "kept" }.into()),
            ]);
        }
    }
    Ok(())
}
