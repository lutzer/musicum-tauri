use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::preset_service;
use sea_orm::DatabaseConnection;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct PresetsArgs {
    #[command(subcommand)]
    pub command: PresetsCommand,
}

#[derive(Debug, Subcommand)]
pub enum PresetsCommand {
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

pub async fn run(db: &DatabaseConnection, args: PresetsArgs) -> Result<()> {
    match args.command {
        PresetsCommand::List { json } => {
            let presets = preset_service::list_presets(db).await?;
            if json {
                print_json(&presets);
            } else if presets.is_empty() {
                println!("No presets. Add a sidecar under .musicum/presets/ and run sync.");
            } else {
                print_table(
                    ("SLUG", "TITLE"),
                    presets.iter().map(|p| (p.slug.clone(), p.title.clone())).collect(),
                );
            }
        }
        PresetsCommand::Show { slug, json } => {
            let preset = preset_service::get_preset_by_slug(db, &slug).await?;
            if json {
                print_json(&preset);
            } else {
                let processors: serde_json::Value =
                    serde_json::from_str(&preset.processors).unwrap_or(serde_json::json!([]));
                print_detail(vec![
                    ("slug", preset.slug.clone()),
                    ("title", preset.title.clone()),
                    ("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
                    ("processors", serde_json::to_string_pretty(&processors).unwrap()),
                ]);
            }
        }
    }
    Ok(())
}
