## Musicum Tauri

Musicum is an app for organizing sound recordings, editing them non-destructively, and exporting collections of them.

### Features

- Sync a directory of audio files into a managed library
- Non-destructive clip editing: trim, apply plugin chains and structural processors
- Reusable processor presets
- Group clips into named collections
- Play files and clips directly from the CLI with real-time plugin processing
- Export clips and files to WAV, FLAC, AIFF, or MP3 (with ffmpeg)
- JSON output on all list/show commands for scripting

## CLI

The `musicum` binary links directly to `musicum-core` and provides library management without the desktop app.

```bash
cargo build -p musicum-cli
./target/debug/musicum --help
```

All commands accept `--library <path>` to override the configured library directory.  
Settings are read from `~/.config/com.musicum.app/settings.json`.

### Commands

| Command | Description |
|---------|-------------|
| `musicum config` | Print settings file path and resolved library paths |
| `musicum sync` | Walk the library directory and sync DB + sidecars |
| **files** | |
| `musicum files list [--json]` | List all files in the library |
| `musicum files show <slug> [--json]` | Show file detail including clips |
| `musicum files set-notes <slug> <notes>` | Set notes on a file |
| `musicum files set-tags <slug> <tags>` | Set tags on a file (comma-separated) |
| `musicum files delete <slug> [--delete-audio]` | Remove file from DB (optionally delete audio) |
| **clips** | |
| `musicum clips list [<file-slug>] [--json]` | List all clips, or only clips for a file |
| `musicum clips show <slug> [--json]` | Show clip detail including processor chain |
| `musicum clips create <file-slug>` | Create a new clip for a file |
| `musicum clips edit <slug>` | Interactively edit a clip's processor chain |
| `musicum clips apply-preset <clip-slug> <preset-slug>` | Apply a preset's processor chain to a clip |
| `musicum clips clear-processors <clip-slug>` | Remove all processors from a clip |
| `musicum clips set-notes <slug> <notes>` | Set notes on a clip |
| `musicum clips delete <slug>` | Delete a clip |
| **collections** | |
| `musicum collections list [--json]` | List all collections |
| `musicum collections show <slug> [--json]` | Show collection detail including clips |
| `musicum collections create <title>` | Create a new collection |
| `musicum collections set-description <slug> <description>` | Set collection description |
| `musicum collections add-clip <collection-slug> <clip-slug>` | Add a clip to a collection |
| `musicum collections remove-clip <collection-slug> <clip-slug>` | Remove a clip from a collection |
| `musicum collections delete <slug>` | Delete a collection |
| **presets** | |
| `musicum presets list [--json]` | List all presets |
| `musicum presets show <slug> [--json]` | Show preset detail including processor chain |
| `musicum presets create --title <title>` | Create a new preset |
| `musicum presets edit <slug>` | Interactively edit processor parameters |
| `musicum presets add-processor <preset-slug> <processor-type>` | Add a processor to a preset |
| `musicum presets remove-processor <preset-slug> <instance-uuid>` | Remove a processor from a preset |
| `musicum presets set-param <preset-slug> <instance-uuid> <key> <value>` | Set a processor parameter |
| `musicum presets delete <slug>` | Delete a preset |
| **processors** | |
| `musicum processors list` | List available structural processors |
| **play / export** | |
| `musicum play [<slug>] [--file\|--clip] [--collection <slug>] [--loop]` | Play a file, clip, or collection |
| `musicum export <slug> <output> [--file\|--clip] [--samplerate N] [--channels N] [--bitrate N]` | Export to audio file |

### Shell completion

Tab-completion for subcommands, flags, and slugs is available for zsh and bash.

**Setup (zsh)**

Add a shell function and enable completions in your `~/.zshrc`, after the `compinit` call:

```zsh
musicum() {
    cargo run --manifest-path /path/to/musicum-tauri/apps/cli/Cargo.toml -- "$@"
}

eval "$(musicum completions zsh)"
```

If you have a compiled binary instead:

```zsh
eval "$(musicum completions zsh)"
```

**Setup (bash)**

```bash
eval "$(musicum completions bash)"
```

Or generate once and source it:

```bash
musicum completions bash >> ~/.bashrc
```

Slug completions (file, clip, collection, preset slugs) are fetched live from the library at tab-press time.

## Development

Build by @lutzer and Claude Code in 2026
