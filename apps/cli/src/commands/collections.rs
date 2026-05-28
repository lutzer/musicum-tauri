use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::collection_service;
use sea_orm::DatabaseConnection;

use crate::output::{DetailItem::Field, print_detail, print_json, print_result,
                    print_section_header, print_table};

#[derive(Debug, Args)]
pub struct CollectionsArgs {
    #[command(subcommand)]
    pub command: CollectionsCommand,
}

#[derive(Debug, Subcommand)]
pub enum CollectionsCommand {
    /// Create a new collection
    Create {
        title: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Set (replace) description for a collection
    SetDescription {
        slug: String,
        description: String,
    },
    /// Delete a collection and all its clip memberships
    Delete {
        slug: String,
    },
    /// Add a clip to a collection
    AddClip {
        collection_slug: String,
        clip_slug: String,
        #[arg(long)]
        position: Option<i32>,
    },
    /// Remove a clip from a collection
    RemoveClip {
        collection_slug: String,
        clip_slug: String,
    },
    /// List all collections
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show detail for one collection including its clips
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(db: &DatabaseConnection, args: CollectionsArgs) -> Result<()> {
    match args.command {
        CollectionsCommand::Create { title, description } => {
            let desc = description.unwrap_or_default();
            let col = collection_service::create_collection(db, &title, &desc).await?;
            print_result("Created collection", &[
                Field("slug", col.slug),
                Field("title", col.title),
            ]);
        }

        CollectionsCommand::SetDescription { slug, description } => {
            collection_service::set_collection_description(db, &slug, &description).await?;
            print_result("Updated collection", &[Field("slug", slug)]);
        }

        CollectionsCommand::Delete { slug } => {
            collection_service::delete_collection(db, &slug).await?;
            print_result("Deleted collection", &[Field("slug", slug)]);
        }

        CollectionsCommand::AddClip { collection_slug, clip_slug, position } => {
            let row = collection_service::add_clip_to_collection(
                db, &collection_slug, &clip_slug, position,
            )
            .await?;
            print_result("Added clip", &[
                Field("collection", collection_slug),
                Field("clip", clip_slug),
                Field("position", row.position.to_string()),
            ]);
        }

        CollectionsCommand::RemoveClip { collection_slug, clip_slug } => {
            collection_service::remove_clip_from_collection(db, &collection_slug, &clip_slug)
                .await?;
            print_result("Removed clip", &[
                Field("collection", collection_slug),
                Field("clip", clip_slug),
            ]);
        }

        CollectionsCommand::List { json } => {
            let cols = collection_service::list_collections(db).await?;
            if json {
                print_json(&cols);
            } else if cols.is_empty() {
                println!("No collections.");
            } else {
                print_table(
                    "collections",
                    &["SLUG", "TITLE"],
                    cols.iter().map(|c| vec![c.slug.clone(), c.title.clone()]).collect(),
                );
            }
        }

        CollectionsCommand::Show { slug, json } => {
            let (col, clips) =
                collection_service::get_collection_with_clips(db, &slug).await?;

            if json {
                print_json(&serde_json::json!({
                    "collection": col,
                    "clips": clips,
                }));
            } else {
                print_detail(&[
                    Field("slug", col.slug.clone()),
                    Field("title", col.title.clone()),
                    Field("desc", if col.description.is_empty() { "-".into() } else { col.description.clone() }),
                ]);

                if clips.is_empty() {
                    print_section_header("clips");
                    println!("(none)");
                } else {
                    print_table(
                        "clips",
                        &["#", "SLUG", "TITLE"],
                        clips
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                vec![
                                    (i + 1).to_string(),
                                    c.slug.clone(),
                                    c.title.clone(),
                                ]
                            })
                            .collect(),
                    );
                }
            }
        }
    }
    Ok(())
}
