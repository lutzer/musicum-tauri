mod commands;
mod output;
mod settings;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "musicum",
    about = "Musicum audio library CLI",
    version
)]
struct Cli {
    /// Override the library directory for this invocation
    #[arg(long, global = true)]
    library: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Walk the library directory and sync DB + sidecars
    Sync,
    /// File operations
    Files(commands::files::FilesArgs),
    /// Clip operations
    Clips(commands::clips::ClipsArgs),
    /// Collection operations
    Collections(commands::collections::CollectionsArgs),
    /// Preset operations
    Presets(commands::presets::PresetsArgs),
    /// Print the path to the settings file
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut app_settings = settings::load()?;
    if let Some(lib) = cli.library {
        app_settings.library_dir = lib;
    }

    match cli.command {
        Commands::Config => {
            println!("Settings file: {}", settings::settings_path().display());
            println!("Library dir:   {}", app_settings.library_dir);
            if let Some(gen) = &app_settings.generated_dir {
                println!("Generated dir: {gen}");
            }
            return Ok(());
        }
        _ => {}
    }

    let db = musicum_core::db::connect(&app_settings.library_dir).await?;

    match cli.command {
        Commands::Sync => commands::sync::run(&db, &app_settings).await?,
        Commands::Files(args) => commands::files::run(&db, args).await?,
        Commands::Clips(args) => commands::clips::run(&db, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args) => commands::presets::run(&db, args).await?,
        Commands::Config => unreachable!(),
    }

    Ok(())
}
