# Shell Completion Design

**Date:** 2026-05-29  
**Status:** Draft  
**Scope:** `apps/cli`

## Goal

Add tab-completion for the `musicum` CLI in **zsh** and **bash** that completes:
- Subcommand names and flags (via `clap_complete`)
- Slug argument values (by querying the SQLite DB at completion time)

## Approach

Static shell structure completion via `clap_complete` + dynamic slug completion via a hidden `_complete-slugs` subcommand. The `completions <shell>` subcommand outputs a single shell snippet that users source into their shell config.

## New Subcommands

### `musicum completions <shell>`

Public subcommand. Prints a completion script to stdout.

```
musicum completions zsh  >> ~/.zshrc && source ~/.zshrc
musicum completions bash >> ~/.bashrc && source ~/.bashrc
```

The output is a self-contained shell snippet that:
1. Contains the static clap-generated completion for subcommands and flags.
2. Appends a custom shell function that overrides slug argument positions to call `_complete-slugs` at tab-press time.

`shell` accepts `zsh` or `bash`. Any other value exits with an error.

### `musicum _complete-slugs --type <kinds>`  (hidden)

Hidden subcommand — does not appear in `musicum --help`. Called by the completion script at runtime.

```
musicum _complete-slugs --type clip
musicum _complete-slugs --type file,clip,collection
```

- `--type` accepts a comma-separated list of: `file`, `clip`, `collection`, `preset`.
- Queries the DB (same DB resolution as all other commands — reads config + optional `--library` override).
- Prints one slug per line to stdout.
- Exits 0 even if no slugs match (empty output is valid).
- Runs quickly; queries are simple `SELECT slug FROM …` fetches.

## Slug Type Mapping

The completion script maps each argument position to the correct `--type` value:

| Command path | Argument | `--type` |
|---|---|---|
| `play <target>` (no type flag) | target | `file,clip,collection` |
| `play --file <target>` | target | `file` |
| `play --clip <target>` | target | `clip` |
| `play --collection <slug>` | slug | `collection` |
| `clips list <file_slug>` | file_slug | `file` |
| `clips show <slug>` | slug | `clip` |
| `clips create <file_slug>` | file_slug | `file` |
| `clips apply-preset <clip_slug> <preset_slug>` | clip_slug | `clip` |
| `clips apply-preset <clip_slug> <preset_slug>` | preset_slug | `preset` |
| `clips clear-processors <clip_slug>` | clip_slug | `clip` |
| `clips edit <slug>` | slug | `clip` |
| `clips set-notes <slug>` | slug | `clip` |
| `clips delete <slug>` | slug | `clip` |
| `collections show <slug>` | slug | `collection` |
| `collections set-description <slug>` | slug | `collection` |
| `collections delete <slug>` | slug | `collection` |
| `collections add-clip <collection_slug> <clip_slug>` | collection_slug | `collection` |
| `collections add-clip <collection_slug> <clip_slug>` | clip_slug | `clip` |
| `collections remove-clip <collection_slug> <clip_slug>` | both | same as above |
| `presets show <slug>` | slug | `preset` |
| `presets delete <slug>` | slug | `preset` |
| `presets edit <slug>` | slug | `preset` |
| `presets add-processor <preset_slug> <processor_type>` | preset_slug | `preset` |
| `presets add-processor <preset_slug> <processor_type>` | processor_type | *(no completion — static processor IDs)* |
| `presets remove-processor <preset_slug> <instance_uuid>` | preset_slug | `preset` |
| `presets remove-processor <preset_slug> <instance_uuid>` | instance_uuid | *(no completion — UUID)* |
| `files show <slug>` | slug | `file` |
| `export <slug>` (no type flag) | slug | `file,clip` |
| `export --file <slug>` | slug | `file` |
| `export --clip <slug>` | slug | `clip` |

## Implementation Structure

All changes are in `apps/cli`.

### `apps/cli/Cargo.toml`
Add dependency:
```toml
clap_complete = "4"
```

### `apps/cli/src/commands/completions.rs`
New module with two public functions:

**`run_completions(shell: &str)`**  
Called when the user runs `musicum completions <shell>`. Uses `clap_complete::generate()` to write the static clap completion to a buffer, then appends the custom shell function that handles dynamic slug completion. Prints combined output to stdout.

**`run_complete_slugs(db, types: &str)`**  
Called when the user runs `musicum _complete-slugs --type <kinds>`. Parses the comma-separated type list. Queries the relevant service functions:
- `file` → `file_service::list_files(db)`
- `clip` → `clip_service::list_all_clips(db)`
- `collection` → `collection_service::list_collections(db)`
- `preset` → `preset_service::list_presets(db)`

Collects all slugs, deduplicates, prints one per line.

### `apps/cli/src/commands/mod.rs`
Add `pub mod completions;`

### `apps/cli/src/main.rs`
Add to `Commands` enum:
```rust
/// Generate shell completion script
Completions {
    /// Shell to generate completions for (zsh, bash)
    shell: String,
},

/// Internal: list slugs for shell completion (hidden)
#[command(hide = true)]
CompleteSlugs {
    #[arg(long)]
    r#type: String,
},
```

`CompleteSlugs` needs DB access, so it is dispatched after DB connection like other commands.  
`Completions` does not need DB access; it is dispatched before the DB connection block.

## Shell Script Strategy

The completion scripts use standard shell completion mechanisms:

**zsh:** The output defines a `_musicum` function using zsh's `compadd` and `_arguments` builtins. For slug positions, `$(_musicum_slugs <type>)` calls `musicum _complete-slugs --type <type>` and feeds results to `compadd`. The function is registered with `compdef _musicum musicum`.

**bash:** Uses `complete -F _musicum musicum`. The `_musicum` function uses `COMPREPLY` and `compgen`. For slug positions, it calls `musicum _complete-slugs --type <type>` and pipes into `compgen -W`.

Both scripts are embedded as string constants in `completions.rs` and printed verbatim (no templating needed since the binary name `musicum` is fixed).

## Non-Goals

- Fish shell support (not requested)
- Prefix filtering in `_complete-slugs` — shell-level filtering is sufficient
- Completing processor IDs or plugin IDs — these are static and not slug-based
- Auto-installing completion scripts — users run the one-time setup command themselves
