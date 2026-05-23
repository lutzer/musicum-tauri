use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::collection_service;
use sea_orm::DatabaseConnection;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct CollectionsArgs {
    #[command(subcommand)]
    pub command: CollectionsCommand,
}

#[derive(Debug, Subcommand)]
pub enum CollectionsCommand {
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(db: &DatabaseConnection, args: CollectionsArgs) -> Result<()> {
    match args.command {
        CollectionsCommand::List { json } => {
            let cols = collection_service::list_collections(db).await?;
            if json {
                print_json(&cols);
            } else if cols.is_empty() {
                println!("No collections. Add a sidecar under .musicum/collections/ and run sync.");
            } else {
                print_table(
                    ("SLUG", "TITLE"),
                    cols.iter().map(|c| (c.slug.clone(), c.title.clone())).collect(),
                );
            }
        }
        CollectionsCommand::Show { slug, json } => {
            let col = collection_service::get_collection_by_slug(db, &slug).await?;
            if json {
                print_json(&col);
            } else {
                print_detail(vec![
                    ("slug", col.slug.clone()),
                    ("title", col.title.clone()),
                    ("description", if col.description.is_empty() { "-".into() } else { col.description.clone() }),
                ]);
            }
        }
    }
    Ok(())
}
