use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use musicum_core::services::preset_service;
use musicum_core::sidecar::{self, ProcessorEntry, ProcessorRef};
use sea_orm::DatabaseConnection;
use slug::slugify;
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;
use std::path::Path;

use crate::output::{DetailItem::Field, print_detail, print_json, print_result, print_table};

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
    /// Interactively edit processor parameters with an arrow-key UI
    Edit {
        slug: String,
    },
    /// Set a single processor parameter by key/value
    SetParam {
        preset_slug: String,
        instance_uuid: String,
        key: String,
        value: String,
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
                    &["SLUG", "TITLE"],
                    presets.iter().map(|p| vec![p.slug.clone(), p.title.clone()]).collect(),
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
                print_detail(&[
                    Field("slug", preset.slug.clone()),
                    Field("title", preset.title.clone()),
                    Field("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
                ]);
                if processors.is_empty() {
                    println!("\nprocessors: (none)");
                } else {
                    println!("\nprocessors:");
                    print_table(
                        &["UUID", "KIND", "PROC", "ENABLED", "PARAMS"],
                        processors.iter().map(|entry| {
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
                            vec![id.to_string(), kind.to_string(), proc_id.to_string(),
                                 enabled.to_string(), params]
                        }).collect(),
                    );
                }
            }
        }

        PresetsCommand::Create { title, description } => {
            let slug = slugify(&title);
            preset_service::create_preset(db, library_dir, &slug, &title, &description).await?;
            print_result("Created preset", &[
                Field("slug", slug.clone()),
                Field("title", title.clone()),
                Field("description", if description.is_empty() { "-".into() } else { description.clone() }),
                Field("processors", format!("(none — use 'presets add-processor {slug} <type>' to add)")),
            ]);
        }

        PresetsCommand::Remove { slug } => {
            preset_service::delete_preset(db, library_dir, &slug).await?;
            print_result(&format!("Removed preset '{slug}'"), &[]);
        }

        PresetsCommand::AddProcessor { preset_slug, processor_type } => {
            let registry = structural_processors::registry();
            let entry = registry
                .values()
                .find(|e| (e.descriptor)().id == processor_type)
                .ok_or_else(|| {
                    let mut valid: Vec<&str> = registry.values().map(|e| (e.descriptor)().id).collect();
                    valid.sort_unstable();
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
                        (id, serde_json::json!(default))
                    }
                    ParameterDescriptor::Int { id, default, .. } => {
                        (id, serde_json::json!(default))
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

            print_result("Added processor", &[
                Field("id", instance_id.clone()),
                Field("preset", preset_slug.clone()),
                Field("type", processor_type.clone()),
            ]);
        }

        PresetsCommand::Edit { slug } => {
            super::presets_editor::run_editor(db, library_dir, &slug).await?;
        }

        PresetsCommand::SetParam { preset_slug, instance_uuid, key, value } => {
            let parsed = parse_param_value(&value);
            preset_service::set_processor_param(db, library_dir, &preset_slug, &instance_uuid, &key, parsed).await?;
            print_result("Set parameter", &[
                Field("preset", preset_slug.clone()),
                Field("processor", instance_uuid.clone()),
                Field("key", key.clone()),
                Field("value", value.clone()),
            ]);
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
            print_result(&format!("Removed processor '{instance_uuid}'"), &[
                Field("preset", preset_slug.clone()),
            ]);
        }
    }
    Ok(())
}

fn parse_param_value(s: &str) -> serde_json::Value {
    if let Ok(i) = s.parse::<i64>() {
        return serde_json::Value::Number(i.into());
    }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(n);
        }
    }
    serde_json::Value::String(s.to_string())
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
