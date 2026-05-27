use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::{
    deserialize_processor_edits,
    edit::{EditKind, ProcessorEdit},
    services::{clip_service, file_service, preset_service},
};
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use uuid::Uuid;

use crate::output::{DetailItem::{Field, Section}, print_detail, print_json, print_result, print_section_header, print_table};
use super::processor_list_editor::{run as run_editor, SaveFn};

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
    /// Apply a preset's processor chain to a clip (replaces existing processors)
    ApplyPreset {
        clip_slug: String,
        preset_slug: String,
    },
    /// Remove all processors from a clip
    ClearProcessors {
        clip_slug: String,
    },
    /// Interactively edit processor chain for a clip
    Edit {
        slug: String,
    },
    /// Set notes for a clip (full replace)
    SetNotes {
        slug: String,
        notes: String,
    },
    /// Delete a clip from DB and remove it from its sidecar
    Delete {
        slug: String,
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
                    println!("No clips for '{slug}'. Sync or create clips via sidecar.");
                } else {
                    print_table(
                        "clips",
                        &["SLUG", "TITLE"],
                        clips.iter().map(|c| vec![c.slug.clone(), c.title.clone()]).collect(),
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
                        "clips",
                        &["SLUG", "TITLE", "FILE"],
                        clips.iter().map(|c| {
                            let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
                            vec![c.slug.clone(), c.title.clone(), file_slug.to_string()]
                        }).collect(),
                    );
                }
            }
        }
        ClipsCommand::Show { slug, json } => {
            let clip = clip_service::get_clip_by_slug(db, &slug).await?;
            let file = file_service::get_file_by_id(db, &clip.file_id).await?;

            if json {
                print_json(&serde_json::json!({ "clip": clip, "file": file }));
            } else {
                let processors = deserialize_processor_edits(&clip.processors);
                print_detail(&[
                    Section("clip"),
                    Field("slug", clip.slug.clone()),
                    Field("title", clip.title.clone()),
                    Field("duration", clip.duration.map_or("-".into(), |d| format!("{d:.3}s"))),
                    Field("notes", if clip.notes.is_empty() { "-".into() } else { clip.notes.clone() }),
                    Section("file"),
                    Field("slug", file.slug.clone()),
                    Field("path", file.path.clone()),
                    Field("duration", format!("{:.3}s", file.duration)),
                    Field("sample_rate", format!("{}Hz", file.sample_rate)),
                    Field("channels", file.channels.to_string()),
                    Field("mime", file.mime_type.clone()),
                ]);
                if processors.is_empty() {
                    print_section_header("processors");
                    println!("(none)");
                } else {
                    print_table(
                        "processors",
                        &["#", "TYPE", "PROCESSOR", "ENABLED", "PARAMS"],
                        processors.iter().enumerate().map(|(i, p)| {
                            let (type_str, proc_id, params_str) = match &p.kind {
                                EditKind::Structural { processor_id, params } => {
                                    let ps = params.iter()
                                        .map(|(k, v)| format!("{k}={v}"))
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    ("structural", processor_id.as_str(), ps)
                                }
                                EditKind::Plugin { plugin_id, params } => {
                                    let ps = params.iter()
                                        .map(|(k, v)| format!("{k}={v}"))
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    ("audio-plugin", plugin_id.as_str(), ps)
                                }
                            };
                            vec![
                                (i + 1).to_string(),
                                type_str.to_string(),
                                proc_id.to_string(),
                                p.enabled.to_string(),
                                params_str,
                            ]
                        }).collect(),
                    );
                }
            }
        }
        ClipsCommand::Create { file_slug, title } => {
            let clip = clip_service::create_clip(db, &file_slug, &title).await?;
            print_result("Created clip", &[
                Field("slug", clip.slug.clone()),
                Field("file", file_slug.clone()),
            ]);
        }

        ClipsCommand::ApplyPreset { clip_slug, preset_slug } => {
            let preset = preset_service::get_preset_by_slug(db, &preset_slug).await?;
            let source_processors = deserialize_processor_edits(&preset.processors);
            // Re-assign new UUIDs so each clip application is independent
            let new_processors: Vec<ProcessorEdit> = source_processors
                .into_iter()
                .map(|e| ProcessorEdit { uuid: Uuid::new_v4(), ..e })
                .collect();
            let count = new_processors.len();
            clip_service::update_clip_processors(db, &clip_slug, new_processors).await?;
            print_result("Applied preset", &[
                Field("clip", clip_slug.clone()),
                Field("preset", preset_slug.clone()),
                Field("processors", count.to_string()),
            ]);
        }

        ClipsCommand::ClearProcessors { clip_slug } => {
            clip_service::update_clip_processors(db, &clip_slug, vec![]).await?;
            print_result("Cleared processors", &[
                Field("clip", clip_slug.clone()),
            ]);
        }

        ClipsCommand::Edit { slug } => {
            let clip = clip_service::get_clip_by_slug(db, &slug).await?;
            let processors = deserialize_processor_edits(&clip.processors);
            let title = format!("Clip: {slug}");

            let save: SaveFn<'_> = Box::new(|procs| {
                let slug = slug.clone();
                Box::pin(async move {
                    clip_service::update_clip_processors(db, &slug, procs)
                        .await
                        .map_err(anyhow::Error::from)
                })
            });

            run_editor(&title, processors, save).await?;
        }
        ClipsCommand::SetNotes { slug, notes } => {
            clip_service::set_clip_notes(db, &slug, &notes).await?;
            print_result("Set notes", &[Field("clip", slug.clone())]);
        }
        ClipsCommand::Delete { slug } => {
            clip_service::delete_clip(db, &slug).await?;
            print_result("Deleted clip", &[Field("slug", slug.clone())]);
        }
    }
    Ok(())
}
