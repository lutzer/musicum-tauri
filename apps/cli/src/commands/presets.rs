use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use musicum_core::{
    deserialize_processor_edits,
    edit::{EditKind, ProcessorEdit},
    services::preset_service,
};
use sea_orm::DatabaseConnection;
use slug::slugify;
use structural_processor_sdk::processor::ParameterDescriptor;
use uuid::Uuid;

use crate::output::{DetailItem::Field, print_detail, print_json, print_result, print_section_header, print_table};

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
    Delete {
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

pub async fn run(db: &DatabaseConnection, _catalog_dir: &std::path::Path, args: PresetsArgs) -> Result<()> {
    match args.command {
        PresetsCommand::List { json } => {
            let presets = preset_service::list_presets(db).await?;
            if json {
                print_json(&presets);
            } else if presets.is_empty() {
                println!("No presets. Create one with 'presets create --title <name>'.");
            } else {
                print_table(
                    "presets",
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
                let processors = deserialize_processor_edits(&preset.processors);
                print_detail(&[
                    Field("slug", preset.slug.clone()),
                    Field("title", preset.title.clone()),
                    Field("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
                ]);
                if processors.is_empty() {
                    print_section_header("processors");
                    println!("(none)");
                } else {
                    print_table(
                        "processors",
                        &["UUID", "KIND", "PROC", "ENABLED", "PARAMS"],
                        processors.iter().map(|entry| {
                            let (kind, proc_id, params_str) = match &entry.kind {
                                EditKind::Structural { processor_id, params } => {
                                    let ps = params.iter()
                                        .map(|(k, v)| format!("{k}={v}"))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    ("structural", processor_id.as_str(), ps)
                                }
                                EditKind::Plugin { plugin_id, params } => {
                                    let ps = params.iter()
                                        .map(|(k, v)| format!("{k}={v}"))
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    ("audio-plugin", plugin_id.as_str(), ps)
                                }
                            };
                            let short_uuid = &entry.uuid.to_string()[..8];
                            vec![short_uuid.to_string(), kind.to_string(), proc_id.to_string(),
                                 entry.enabled.to_string(), params_str]
                        }).collect(),
                    );
                }
            }
        }

        PresetsCommand::Create { title, description } => {
            let slug = slugify(&title);
            preset_service::create_preset(db, &slug, &title, &description).await?;
            print_result("Created preset", &[
                Field("slug", slug.clone()),
                Field("title", title.clone()),
                Field("description", if description.is_empty() { "-".into() } else { description.clone() }),
                Field("processors", format!("(none — use 'presets add-processor {slug} <type>' to add)")),
            ]);
        }

        PresetsCommand::Delete { slug } => {
            preset_service::delete_preset(db, &slug).await?;
            print_result(&format!("Deleted preset '{slug}'"), &[]);
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
            let mut default_params = std::collections::HashMap::new();
            for p in descriptor.parameters {
                let (param_id, val) = match p {
                    ParameterDescriptor::Time { id, default, .. } => (id, *default),
                    ParameterDescriptor::Int { id, default, .. } => (id, *default as f64),
                };
                default_params.insert(param_id.to_string(), val);
            }

            let instance_uuid = Uuid::new_v4();
            let new_entry = ProcessorEdit {
                uuid:    instance_uuid,
                enabled: true,
                kind:    EditKind::Structural {
                    processor_id: processor_type.clone(),
                    params:       default_params,
                },
            };

            let preset = preset_service::get_preset_by_slug(db, &preset_slug).await?;
            let mut processors = deserialize_processor_edits(&preset.processors);
            processors.push(new_entry);
            preset_service::update_preset_processors(db, &preset_slug, processors).await?;

            print_result("Added processor", &[
                Field("id", instance_uuid.to_string()),
                Field("preset", preset_slug.clone()),
                Field("type", processor_type.clone()),
            ]);
        }

        PresetsCommand::Edit { slug } => {
            super::presets_editor::run_editor(db, &slug).await?;
        }

        PresetsCommand::SetParam { preset_slug, instance_uuid, key, value } => {
            let parsed = parse_param_value(&value);
            preset_service::set_processor_param(db, &preset_slug, &instance_uuid, &key, parsed).await?;
            print_result("Set parameter", &[
                Field("preset", preset_slug.clone()),
                Field("processor", instance_uuid.clone()),
                Field("key", key.clone()),
                Field("value", value.clone()),
            ]);
        }

        PresetsCommand::RemoveProcessor { preset_slug, instance_uuid } => {
            let preset = preset_service::get_preset_by_slug(db, &preset_slug).await?;
            let mut processors = deserialize_processor_edits(&preset.processors);
            let original_len = processors.len();
            processors.retain(|e| e.uuid.to_string() != instance_uuid);
            if processors.len() == original_len {
                bail!("processor '{instance_uuid}' not found in preset '{preset_slug}'");
            }
            preset_service::update_preset_processors(db, &preset_slug, processors).await?;
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
