mod commands;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use musicum_core::config::{self, LibraryPaths};

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
    /// Export a file or clip to an audio file
    Export(commands::export::ExportArgs),
    /// Print config and resolved library paths
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let paths = if let Some(lib) = cli.library {
        LibraryPaths::from_override(&lib)
    } else {
        config::load()?.library_paths()
    };

    if let Commands::Config = cli.command {
        println!("Config file:   {}", config::config_path().display());
        println!("Library dir:   {}", paths.library_dir.display());
        println!("Files dir:     {}", paths.files_dir.display());
        println!("Catalog dir:   {}", paths.catalog_dir.display());
        println!("Generated dir: {}", paths.generated_dir.display());
        return Ok(());
    }

    let db = musicum_core::db::connect(&paths.catalog_dir).await?;

    match cli.command {
        Commands::Sync              => commands::sync::run(&db, &paths).await?,
        Commands::Files(args)       => commands::files::run(&db, &paths.files_dir, args).await?,
        Commands::Clips(args)       => commands::clips::run(&db, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args)     => commands::presets::run(&db, &paths.catalog_dir, args).await?,
        Commands::Processors(args)  => commands::processors::run(args),
        Commands::Play { target, file, clip, loop_mode } => {
            commands::play::run(&db, target, file, clip, loop_mode).await?
        }
        Commands::Export(args) => commands::export::run(&db, args).await?,
        Commands::Config => unreachable!(),
    }

    Ok(())
}
