use clap::{Args, Subcommand};
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

pub fn run(args: ProcessorsArgs) {
    match args.command {
        ProcessorsCommand::List { json } => {
            let registry = structural_processors::registry();
            let mut entries: Vec<_> = registry.values().collect();
            entries.sort_by_key(|e| (e.descriptor)().id);

            if json {
                let descriptors: Vec<_> = entries.iter().map(|e| (e.descriptor)()).collect();
                print_json(&descriptors);
            } else if entries.is_empty() {
                println!("No processors registered.");
            } else {
                print_table(
                    "processors",
                    &["ID", "NAME", "PARAMETERS"],
                    entries
                        .iter()
                        .map(|e| {
                            let d = (e.descriptor)();
                            let params = d.parameters.iter()
                                .map(|p| match p {
                                    ParameterDescriptor::Time { id, default, .. } =>
                                        format!("{id}={default} (time)"),
                                    ParameterDescriptor::Int { id, default, .. } =>
                                        format!("{id}={default} (int)"),
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            vec![d.id.to_string(), d.name.to_string(), params]
                        })
                        .collect(),
                );
            }
        }
    }
}
