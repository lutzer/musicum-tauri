use audio_plugin_sdk::PluginParameter;
use clap::{Args, Subcommand};
use musicum_core::EditRegistry;
use serde::Serialize;
use structural_processor_sdk::processor::ParameterDescriptor;

use crate::output::{print_json, print_table};

#[derive(Debug, Args)]
pub struct ProcessorsArgs {
    #[command(subcommand)]
    pub command: ProcessorsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProcessorsCommand {
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Serialize)]
struct ProcessorListEntry {
    id:         String,
    #[serde(rename = "type")]
    kind:       String,
    name:       String,
    parameters: Vec<String>,
}

pub fn run(args: ProcessorsArgs) {
    match args.command {
        ProcessorsCommand::List { json } => {
            let registry = EditRegistry::default();
            let mut entries: Vec<ProcessorListEntry> = Vec::new();

            for entry in registry.structural.values() {
                let d = (entry.descriptor)();
                let parameters = d
                    .parameters
                    .iter()
                    .map(|p| match p {
                        ParameterDescriptor::Time { id, default, .. } =>
                            format!("{id}={default} (time)"),
                        ParameterDescriptor::Int { id, default, .. } =>
                            format!("{id}={default} (int)"),
                    })
                    .collect();
                entries.push(ProcessorListEntry {
                    id:   d.id.to_string(),
                    kind: "structural".to_string(),
                    name: d.name.to_string(),
                    parameters,
                });
            }

            for (id, entry) in &registry.plugins {
                let d = (entry.descriptor)();
                let parameters = d
                    .parameters
                    .iter()
                    .filter_map(|p| match p {
                        PluginParameter::Float { id, default, .. } =>
                            Some(format!("{id}={default} (float)")),
                        PluginParameter::Bool { id, default, .. } =>
                            Some(format!("{id}={} (bool)", if *default { 1 } else { 0 })),
                        PluginParameter::Action { .. } | PluginParameter::Canvas { .. } => None,
                    })
                    .collect();
                entries.push(ProcessorListEntry {
                    id:   id.clone(),
                    kind: "audio-plugin".to_string(),
                    name: d.name.to_string(),
                    parameters,
                });
            }

            entries.sort_by(|a, b| a.id.cmp(&b.id));

            if json {
                print_json(&entries);
            } else if entries.is_empty() {
                println!("No processors registered.");
            } else {
                print_table(
                    "processors",
                    &["ID", "TYPE", "NAME", "PARAMETERS"],
                    entries
                        .iter()
                        .map(|e| {
                            vec![
                                e.id.clone(),
                                e.kind.clone(),
                                e.name.clone(),
                                e.parameters.join(", "),
                            ]
                        })
                        .collect(),
                );
            }
        }
    }
}
