mod commands;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};
use musicum_core::config::{self, Config};

#[derive(Parser)]
#[command(
    name = "musicum",
    about = "Musicum audio library CLI",
    version
)]
struct Cli {
    /// Path to a config file (overrides the default ~/.musicum/config.toml)
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Walk the library directory and sync DB + sidecars
    Sync {
        /// Auto-remove unresolvable orphaned sidecars without prompting
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
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
        /// Slug or file path to play; also resolves collection slugs automatically
        target: Option<String>,
        /// Force resolution as a collection (use when slug is ambiguous across types)
        #[arg(long, conflicts_with_all = ["file", "clip"])]
        collection: Option<String>,
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
    /// Generate shell completion script
    Completions {
        /// Shell to generate completions for (zsh, bash)
        shell: String,
    },
    /// Internal: list slugs for shell completion
    #[command(hide = true, name = "_complete-slugs")]
    CompleteSlugs {
        /// Comma-separated slug types: file, clip, collection, preset
        #[arg(long = "type")]
        slug_type: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Commands::Completions { shell } = &cli.command {
        commands::completions::run_completions::<Cli>(shell)?;
        return Ok(());
    }

    config::init(cli.config);

    if let Commands::Config = cli.command {
        let cfg = Config::get();
        println!("Config file:   {}", config::default_config_path().display());
        println!("Files dir:     {}", cfg.library.files_dir.display());
        println!("Catalog dir:   {}", cfg.library.catalog_dir.display());
        println!("Generated dir: {}", cfg.library.generated_dir.display());
        return Ok(());
    }

    let db = musicum_core::db::connect(&Config::get().library.catalog_dir).await?;

    match cli.command {
        Commands::Sync { force }    => commands::sync::run(&db, force).await?,
        Commands::Files(args)       => commands::files::run(&db, args).await?,
        Commands::Clips(args)       => commands::clips::run(&db, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args)     => commands::presets::run(&db, args).await?,
        Commands::Processors(args)  => commands::processors::run(args),
        Commands::Play { target, collection, file, clip, loop_mode } => {
            commands::play::run(&db, target, collection, file, clip, loop_mode).await?
        }
        Commands::Export(args) => commands::export::run(&db, args).await?,
        Commands::Config => unreachable!(),
        Commands::CompleteSlugs { slug_type } => {
            commands::completions::run_complete_slugs(&db, &slug_type).await?
        }
        Commands::Completions { .. } => unreachable!(),
    }

    Ok(())
}
