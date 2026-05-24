use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use musicum_core::services::preset_service;
use musicum_core::sidecar::{self, ProcessorEntry, ProcessorRef};
use sea_orm::DatabaseConnection;
use slug::slugify;
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;
use std::path::Path;

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
    Create {
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Remove {
        slug: String,
    },
    AddProcessor {
        preset_slug: String,
        processor_type: String,
    },
    RemoveProcessor {
        preset_slug: String,
        instance_uuid: String,
    },
}

pub async fn run(db: &DatabaseConnection, library_dir: &str, args: PresetsArgs) -> Result<()> {
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
                let processors: Vec<ProcessorEntry> =
                    serde_json::from_str(&preset.processors).unwrap_or_else(|_| vec![]);
                print_detail(vec![
                    ("slug", preset.slug.clone()),
                    ("title", preset.title.clone()),
                    ("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
                ]);
                if processors.is_empty() {
                    println!("\nprocessors: (none)");
                } else {
                    println!("\nprocessors:");
                    let uuid_w = 36;
                    let kind_w = 12;
                    let proc_w = 6;
                    println!("  {:<uuid_w$}  {:<kind_w$}  {:<proc_w$}  ENABLED  PARAMS",
                        "UUID", "KIND", "PROC");
                    println!("  {}", "─".repeat(uuid_w + kind_w + proc_w + 30));
                    for entry in &processors {
                        let (id, kind, proc_id, enabled, params) = match entry {
                            ProcessorEntry::Structural { id, enabled, processor } => (
                                id.as_str(), "structural", processor.id.as_str(), *enabled,
                                format_params(&processor.params),
                            ),
                            ProcessorEntry::AudioPlugin { id, enabled, processor } => (
                                id.as_str(), "audio-plugin", processor.id.as_str(), *enabled,
                                format_params(&processor.params),
                            ),
                        };
                        println!(
                            "  {:<uuid_w$}  {:<kind_w$}  {:<proc_w$}  {:<7}  {}",
                            id, kind, proc_id, enabled, params
                        );
                    }
                }
            }
        }

        PresetsCommand::Create { title, description } => {
            let slug = slugify(&title);
            preset_service::create_preset(db, library_dir, &slug, &title, &description).await?;
            println!("Created preset '{title}'");
            println!("  slug: {slug}");
            if !description.is_empty() {
                println!("  description: {description}");
            }
            println!("  processors: (none — use 'presets add-processor {slug} <type>' to add)");
        }

        PresetsCommand::Remove { slug } => {
            preset_service::delete_preset(db, library_dir, &slug).await?;
            println!("removed '{slug}'");
        }

        PresetsCommand::AddProcessor { preset_slug, processor_type } => {
            let registry = structural_processors::registry();
            let entry = registry
                .iter()
                .find(|e| (e.descriptor)().id == processor_type)
                .ok_or_else(|| {
                    let valid: Vec<&str> = registry.iter().map(|e| (e.descriptor)().id).collect();
                    anyhow::anyhow!(
                        "unknown processor type '{}'. Valid types: {}",
                        processor_type,
                        valid.join(", ")
                    )
                })?;

            let descriptor = (entry.descriptor)();
            let mut default_params = serde_json::Map::new();
            for p in descriptor.parameters {
                let (param_id, val) = match p {
                    ParameterDescriptor::Time { id, default, .. } => {
                        (*id, serde_json::json!(*default))
                    }
                    ParameterDescriptor::Int { id, default, .. } => {
                        (*id, serde_json::json!(*default))
                    }
                };
                default_params.insert(param_id.to_string(), val);
            }

            let instance_id = Uuid::new_v4().to_string();
            let new_entry = ProcessorEntry::Structural {
                id: instance_id.clone(),
                enabled: true,
                processor: ProcessorRef {
                    id: processor_type.clone(),
                    params: serde_json::Value::Object(default_params),
                },
            };

            let lib = Path::new(library_dir);
            let mut sc = sidecar::read_preset_sidecar(lib, &preset_slug)?;
            sc.processors.push(new_entry);
            sidecar::write_preset_sidecar(lib, &sc)?;
            preset_service::update_preset_processors(db, library_dir, &preset_slug, sc.processors).await?;

            println!("{instance_id}");
        }

        PresetsCommand::RemoveProcessor { preset_slug, instance_uuid } => {
            let lib = Path::new(library_dir);
            let mut sc = sidecar::read_preset_sidecar(lib, &preset_slug)?;
            let original_len = sc.processors.len();
            sc.processors.retain(|e| {
                let id = match e {
                    ProcessorEntry::Structural { id, .. } => id.as_str(),
                    ProcessorEntry::AudioPlugin { id, .. } => id.as_str(),
                };
                id != instance_uuid
            });
            if sc.processors.len() == original_len {
                bail!("processor '{instance_uuid}' not found in preset '{preset_slug}'");
            }
            sidecar::write_preset_sidecar(lib, &sc)?;
            preset_service::update_preset_processors(db, library_dir, &preset_slug, sc.processors).await?;
            println!("removed processor '{instance_uuid}'");
        }
    }
    Ok(())
}

fn format_params(params: &serde_json::Value) -> String {
    match params.as_object() {
        None => "{}".to_string(),
        Some(map) => map
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
