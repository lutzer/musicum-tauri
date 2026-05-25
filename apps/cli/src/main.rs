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
    /// List registered structural processors
    Processors(commands::processors::ProcessorsArgs),
    /// Play a file or clip (slug or file path)
    Play {
        /// Slug or file path to play
        target: String,
        /// Resolve target as a file slug (skips clip lookup)
        #[arg(long, conflicts_with = "clip")]
        file: bool,
        /// Resolve target as a clip slug (skips file lookup)
        #[arg(long, conflicts_with = "file")]
        clip: bool,
        /// Start playback with looping enabled
        #[arg(long = "loop")]
        loop_mode: bool,
    },
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

    if let Commands::Config = cli.command {
        println!("Settings file: {}", settings::settings_path().display());
        println!("Library dir:   {}", app_settings.library_dir);
        if let Some(gen) = &app_settings.generated_dir {
            println!("Generated dir: {gen}");
        }
        return Ok(());
    }

    let db = musicum_core::db::connect(&app_settings.library_dir).await?;
    let library_dir = app_settings.library_dir.as_str();

    match cli.command {
        Commands::Sync              => commands::sync::run(&db, &app_settings).await?,
        Commands::Files(args)       => commands::files::run(&db, args).await?,
        Commands::Clips(args)       => commands::clips::run(&db, library_dir, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args)     => commands::presets::run(&db, library_dir, args).await?,
        Commands::Processors(args)  => commands::processors::run(args),
        Commands::Play { target, file, clip, loop_mode } => commands::play::run(&db, target, file, clip, loop_mode).await?,
        Commands::Config => unreachable!(),
    }

    Ok(())
}
