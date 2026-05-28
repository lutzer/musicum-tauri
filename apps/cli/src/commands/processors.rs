use clap::{Args, Subcommand};
use musicum_core::{EditRegistry, EditType, ParamInfo};
use serde::Serialize;

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
            let mut entries: Vec<ProcessorListEntry> = registry
                .list_entries()
                .into_iter()
                .map(|e| {
                    let kind = match e.edit_type {
                        EditType::Structural => "structural",
                        EditType::Plugin     => "audio-plugin",
                    }
                    .to_string();
                    let parameters = e
                        .parameters
                        .iter()
                        .map(|p| match p {
                            ParamInfo::Float { id, default, .. } => format!("{id}={default} (float)"),
                            ParamInfo::Bool  { id, default, .. } => format!("{id}={} (bool)", *default as u8),
                            ParamInfo::Time  { id, default, .. } => format!("{id}={default} (time)"),
                            ParamInfo::Int   { id, default, .. } => format!("{id}={default} (int)"),
                        })
                        .collect();
                    ProcessorListEntry { id: e.id, kind, name: e.name.to_string(), parameters }
                })
                .collect();

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
                        .map(|e| vec![
                            e.id.clone(),
                            e.kind.clone(),
                            e.name.clone(),
                            e.parameters.join(", "),
                        ])
                        .collect(),
                );
            }
        }
    }
}
