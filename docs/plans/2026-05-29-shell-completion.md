# Shell Completion Implementation Plan

**Goal:** Add tab-completion for `musicum` in zsh and bash that covers subcommands/flags via `clap_complete` and slug arguments via a hidden `_complete-slugs` subcommand.

**Architecture:** `clap_complete::generate()` produces the static completion (subcommands, flags) directly from the live clap command tree — no command structure is duplicated. A Rust `const` registry `SLUG_COMPLETIONS` is the single source of truth for slug argument positions: it maps `(subcommand_path, arg_index) → slug_types`. For zsh, the registry generates function overrides that sit on top of clap's modular per-subcommand function structure. For bash, it generates a case-statement detector that wraps the clap-generated function. Adding slug completion for a new command requires only one new registry entry — no shell script strings are edited.

**Tech Stack:** `clap_complete 4`, `clap::CommandFactory`, zsh `_arguments` / `compdef`, bash `complete` / `COMPREPLY`.

---

## File Map

| Status | Path | Responsibility |
|--------|------|----------------|
| Modify | `apps/cli/Cargo.toml` | Add `clap_complete = "4"` |
| **Create** | `apps/cli/src/commands/completions.rs` | `SLUG_COMPLETIONS` registry, `run_completions<C>`, `run_complete_slugs`, shell code generators |
| Modify | `apps/cli/src/commands/mod.rs` | `pub mod completions;` |
| Modify | `apps/cli/src/main.rs` | `Completions` + `CompleteSlugs` variants; dispatch before/after DB |

---

## Task 1: Add `clap_complete` dependency

**Files:**
- Modify: `apps/cli/Cargo.toml`

### Step 1.1 — Add the dependency

Under `[dependencies]`:

```toml
clap_complete = "4"
```

### Step 1.2 — Verify it resolves

```
cargo check -p musicum-cli
```

Expected: no errors.

---

## Task 2: Register the module

**Files:**
- Modify: `apps/cli/src/commands/mod.rs`

### Step 2.1 — Add module declaration

```rust
pub mod completions;
```

---

## Task 3: Add the two new commands to `main.rs`

**Files:**
- Modify: `apps/cli/src/main.rs`

### Step 3.1 — Add variants to the `Commands` enum

```rust
    /// Generate shell completion script
    Completions {
        /// Shell to generate completions for (zsh, bash)
        shell: String,
    },

    /// Internal: list slugs for shell completion
    #[command(hide = true, name = "_complete-slugs")]
    CompleteSlugs {
        /// Comma-separated slug types: file, clip, collection, preset
        #[arg(long, name = "type")]
        slug_type: String,
    },
```

### Step 3.2 — Dispatch `Completions` before DB connection

Insert immediately after `let cli = Cli::parse();`, before the `config::load` call:

```rust
    if let Commands::Completions { shell } = &cli.command {
        commands::completions::run_completions::<Cli>(shell)?;
        return Ok(());
    }
```

`Completions` needs the `Cli` type (to call `Cli::command()` via `CommandFactory`) but no DB, so it exits before the DB setup block.

### Step 3.3 — Dispatch `CompleteSlugs` in the match block

Add these two arms to the existing `match cli.command { ... }`:

```rust
        Commands::CompleteSlugs { slug_type } => {
            commands::completions::run_complete_slugs(&db, &slug_type).await?
        }
        Commands::Completions { .. } => unreachable!(),
```

### Step 3.4 — Verify

```
cargo check -p musicum-cli
```

Expected: error about missing `completions` items. Resolved in Task 4.

---

## Task 4: Create `completions.rs`

**Files:**
- Create: `apps/cli/src/commands/completions.rs`

The file has four parts: imports, the slug registry, `run_complete_slugs`, and `run_completions` with its per-shell generators.

### Step 4.1 — Write the full file

```rust
use anyhow::{bail, Result};
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use musicum_core::services::{clip_service, collection_service, file_service, preset_service};
use sea_orm::DatabaseConnection;

// ---------------------------------------------------------------------------
// Slug completion registry
//
// Add one entry here for each positional argument that should complete slugs.
// Subcommand names and flags appear in the completion automatically via
// clap_complete; only slug argument positions need an entry here.
// ---------------------------------------------------------------------------

struct SlugCompletion {
    /// Subcommand words, e.g. `&["clips", "show"]` or `&["play"]`
    path: &'static [&'static str],
    /// 0-based index of the positional argument within this subcommand
    arg_index: usize,
    /// Comma-separated slug types passed to `musicum _complete-slugs --type`
    types: &'static str,
}

const SLUG_COMPLETIONS: &[SlugCompletion] = &[
    SlugCompletion { path: &["play"],                            arg_index: 0, types: "file,clip,collection" },
    SlugCompletion { path: &["clips", "list"],                   arg_index: 0, types: "file" },
    SlugCompletion { path: &["clips", "show"],                   arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "create"],                 arg_index: 0, types: "file" },
    SlugCompletion { path: &["clips", "apply-preset"],           arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "apply-preset"],           arg_index: 1, types: "preset" },
    SlugCompletion { path: &["clips", "clear-processors"],       arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "edit"],                   arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "set-notes"],              arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "delete"],                 arg_index: 0, types: "clip" },
    SlugCompletion { path: &["collections", "show"],             arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "set-description"],  arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "delete"],           arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "add-clip"],         arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "add-clip"],         arg_index: 1, types: "clip" },
    SlugCompletion { path: &["collections", "remove-clip"],      arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "remove-clip"],      arg_index: 1, types: "clip" },
    SlugCompletion { path: &["presets", "show"],                 arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "delete"],               arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "edit"],                 arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "add-processor"],        arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "remove-processor"],     arg_index: 0, types: "preset" },
    SlugCompletion { path: &["files", "show"],                   arg_index: 0, types: "file" },
    SlugCompletion { path: &["export"],                          arg_index: 0, types: "file,clip" },
];

// ---------------------------------------------------------------------------
// Runtime slug listing — queries DB and prints one slug per line
// ---------------------------------------------------------------------------

pub async fn run_complete_slugs(db: &DatabaseConnection, types: &str) -> Result<()> {
    let mut slugs: Vec<String> = Vec::new();
    for kind in types.split(',') {
        match kind.trim() {
            "file" => {
                slugs.extend(file_service::list_files(db).await?.into_iter().map(|m| m.slug));
            }
            "clip" => {
                slugs.extend(clip_service::list_all_clips(db).await?.into_iter().map(|m| m.slug));
            }
            "collection" => {
                slugs.extend(
                    collection_service::list_collections(db)
                        .await?
                        .into_iter()
                        .map(|m| m.slug),
                );
            }
            "preset" => {
                slugs.extend(preset_service::list_presets(db).await?.into_iter().map(|m| m.slug));
            }
            other => eprintln!("unknown slug type: {other}"),
        }
    }
    slugs.sort();
    slugs.dedup();
    for s in slugs {
        println!("{s}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Completion script generation
// ---------------------------------------------------------------------------

pub fn run_completions<C: CommandFactory>(shell: &str) -> Result<()> {
    match shell {
        "zsh" => generate_zsh::<C>(),
        "bash" => generate_bash::<C>(),
        other => bail!("unsupported shell: {other}; choose zsh or bash"),
    }
}

// ---------------------------------------------------------------------------
// zsh
//
// clap_complete generates one function per leaf subcommand, named:
//   _musicum__<sub1>__<sub2>   (hyphens → underscores)
//
// We override those functions after the clap output to add slug completion
// for known positional argument positions. The override set is generated
// automatically from SLUG_COMPLETIONS.
// ---------------------------------------------------------------------------

fn zsh_fn_name(path: &[&str]) -> String {
    let parts: Vec<String> = path.iter().map(|s| s.replace('-', "_")).collect();
    format!("_musicum__{}", parts.join("__"))
}

fn generate_zsh<C: CommandFactory>() -> Result<()> {
    let mut cmd = C::command();
    let mut buf = Vec::<u8>::new();
    generate(Shell::Zsh, &mut cmd, "musicum", &mut buf);
    print!("{}", String::from_utf8(buf).unwrap());
    print!("{}", zsh_slug_augmentation());
    Ok(())
}

fn zsh_slug_augmentation() -> String {
    use std::collections::{BTreeMap, BTreeSet};

    let mut out = String::from(
        r#"
# Slug completion helper — queries musicum at tab-press time.
__musicum_slug() {
  local completions
  completions=(${(f)"$(musicum _complete-slugs --type "$1" 2>/dev/null)"})
  compadd -a completions
}
"#,
    );

    // One typed helper per unique type combo.
    let unique_types: BTreeSet<&str> = SLUG_COMPLETIONS.iter().map(|e| e.types).collect();
    for types in &unique_types {
        let safe = types.replace(',', "_");
        out.push_str(&format!(
            "__musicum_slug_{safe}() {{ __musicum_slug \"{types}\"; }}\n"
        ));
    }

    // One function override per unique subcommand path.
    let mut by_fn: BTreeMap<String, Vec<&SlugCompletion>> = BTreeMap::new();
    for entry in SLUG_COMPLETIONS {
        by_fn.entry(zsh_fn_name(entry.path)).or_default().push(entry);
    }

    for (fn_name, mut entries) in by_fn {
        entries.sort_by_key(|e| e.arg_index);
        let specs: Vec<String> = entries
            .iter()
            .map(|e| {
                let safe = e.types.replace(',', "_");
                // zsh _arguments positional spec: 'N: :helper_fn'  (1-based)
                format!("    '{}: :__musicum_slug_{safe}'", e.arg_index + 1)
            })
            .collect();
        out.push_str(&format!(
            "\n{fn_name}() {{\n    _arguments \\\n{} \\\n    && return 0\n}}\n",
            specs.join(" \\\n")
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// bash
//
// clap_complete generates `_musicum()` and registers it with `complete`.
// We rename it to `_musicum_static()`, then wrap it: if the current position
// is a known slug argument we call musicum _complete-slugs; otherwise we fall
// back to the clap-generated static completion.
//
// The rename relies on the clap_complete output containing the literal string
// `_musicum() {`. Verify this with Task 8 when clap_complete is upgraded.
// ---------------------------------------------------------------------------

fn generate_bash<C: CommandFactory>() -> Result<()> {
    let mut cmd = C::command();
    let mut buf = Vec::<u8>::new();
    generate(Shell::Bash, &mut cmd, "musicum", &mut buf);
    let clap_out = String::from_utf8(buf).unwrap();

    // Rename the clap function so our wrapper can delegate to it.
    // Strip the existing `complete` registration; we emit a new one below.
    let clap_static = clap_out
        .replace("_musicum() {", "_musicum_static() {")
        .replace("complete -F _musicum musicum", "# (re-registered below)");

    print!("{}", clap_static);
    print!("{}", bash_slug_augmentation());
    Ok(())
}

fn bash_slug_case_arm(entry: &SlugCompletion) -> String {
    // Key format: "subcmd1::subcmd2::arg_index" (2-level) or "subcmd1::arg_index" (1-level)
    let key = if entry.path.len() == 1 {
        format!("{}::{}", entry.path[0], entry.arg_index)
    } else {
        format!("{}::{}::{}", entry.path[0], entry.path[1], entry.arg_index)
    };
    format!("        \"{key}\") echo \"{}\" ;;", entry.types)
}

fn bash_slug_augmentation() -> String {
    let case_arms: Vec<String> = SLUG_COMPLETIONS.iter().map(bash_slug_case_arm).collect();

    format!(
        r#"
# Detects which slug type (if any) should complete at the current cursor position.
# Call with: __musicum_detect_slug_type "${COMP_WORDS[@]}" "$COMP_CWORD"
__musicum_detect_slug_type() {{
    local -a words=("${{@:1:$#-1}}")
    local cword="${{!#}}"

    # Walk words before the cursor; collect non-flag positionals (skip binary at [0]).
    # --library is the only global flag that consumes a value; skip its value.
    local subcmd1="" subcmd2=""
    local -i pos=0 i skip=0
    for (( i=1; i<cword; i++ )); do
        local w="${{words[$i]}}"
        if [[ $skip -eq 1 ]]; then skip=0; continue; fi
        if [[ "$w" == "--library" ]]; then skip=1; continue; fi
        if [[ "$w" == -* ]]; then continue; fi
        if   [[ -z "$subcmd1" ]]; then subcmd1="$w"
        elif [[ -z "$subcmd2" ]]; then subcmd2="$w"
        else (( pos++ )); fi
    done

    local key="${{subcmd1}}${{subcmd2:+::${{subcmd2}}}}::${{pos}}"
    case "$key" in
{arms}
    esac
}}

_musicum() {{
    local cur="${{COMP_WORDS[COMP_CWORD]}}"
    local slug_type
    slug_type=$(__musicum_detect_slug_type "${{COMP_WORDS[@]}}" "$COMP_CWORD")
    if [[ -n "$slug_type" ]]; then
        local IFS=$'\n'
        COMPREPLY=($(compgen -W "$(musicum _complete-slugs --type "$slug_type" 2>/dev/null)" -- "$cur"))
    else
        _musicum_static "$@"
    fi
}}
complete -F _musicum musicum
"#,
        arms = case_arms.join("\n")
    )
}
```

### Step 4.2 — Build

```
cargo build -p musicum-cli
```

Fix any type errors. The most likely issue: service functions may return `Result<Vec<Model>, ServiceError>` — use `?` to unwrap and rely on `anyhow`'s `From` conversion. If `ServiceError` doesn't implement `std::error::Error`, you may need `.map_err(|e| anyhow::anyhow!("{e}"))`.

### Step 4.3 — Lint

```
cargo clippy -p musicum-cli -- -D warnings
```

Fix all warnings before continuing.

---

## Task 5: Verify `_complete-slugs` (hidden DB command)

### Step 5.1 — Confirm it is hidden

```
cargo run -p musicum-cli -- --help
```

Expected: no `_complete-slugs` in the output.

### Step 5.2 — Confirm it lists slugs

Run with a library that has some content:

```
cargo run -p musicum-cli -- _complete-slugs --type file
cargo run -p musicum-cli -- _complete-slugs --type clip
cargo run -p musicum-cli -- _complete-slugs --type file,clip
```

Expected: one slug per line, sorted, deduplicated. Empty output is valid if the library is empty.

### Step 5.3 — Confirm unknown type is tolerated

```
cargo run -p musicum-cli -- _complete-slugs --type bogus
```

Expected: exits 0, prints nothing to stdout, prints a warning to stderr.

---

## Task 6: Verify zsh completion script

### Step 6.1 — Generate it

```
cargo run -p musicum-cli -- completions zsh > /tmp/musicum.zsh
```

### Step 6.2 — Inspect the output structure

```
head -5 /tmp/musicum.zsh          # should start with #compdef musicum
grep "__musicum_slug" /tmp/musicum.zsh | head -5   # helper functions
grep "^_musicum__clips__show" /tmp/musicum.zsh     # expected override
```

**Critical check:** Confirm that the function names generated by clap_complete match what `zsh_fn_name()` produces. Run:

```zsh
source /tmp/musicum.zsh
typeset -f | grep "^_musicum__clips__show"
```

If the function is not found, inspect what clap_complete actually generated:

```
grep "^_musicum__" /tmp/musicum.zsh | grep "clips" | head -10
```

Adjust `zsh_fn_name()` in `completions.rs` if the naming convention differs (e.g., clap may use single underscores or a different separator for subcommand names).

### Step 6.3 — Source and test interactively

In a zsh session:

```zsh
source /tmp/musicum.zsh
musicum clips show <TAB>          # → clip slugs
musicum play <TAB>                # → file, clip, collection slugs
musicum collections add-clip <TAB>  # → collection slugs
```

After completing the first arg of `add-clip`, pressing TAB again should offer clip slugs.

---

## Task 7: Verify bash completion script

### Step 7.1 — Generate it

```
cargo run -p musicum-cli -- completions bash > /tmp/musicum.bash
```

### Step 7.2 — Verify the rename worked

```
grep "_musicum_static" /tmp/musicum.bash | head -3
grep "^_musicum() {" /tmp/musicum.bash      # should NOT appear (replaced)
grep "complete -F _musicum musicum" /tmp/musicum.bash | tail -1  # our final line
```

If `_musicum_static` is missing, the `replace("_musicum() {", ...)` call didn't match. Inspect what clap_complete actually emits:

```
grep "_musicum()" /tmp/musicum.bash | head -5
```

Adjust the `replace` call in `generate_bash` to match the exact declaration syntax.

### Step 7.3 — Source and test interactively

In a bash session:

```bash
source /tmp/musicum.bash
musicum clips show <TAB>    # → clip slugs
musicum export <TAB>        # → file and clip slugs
musicum sync <TAB>          # → no slug completion (correct)
musicum --library /tmp clips show <TAB>  # → clip slugs (--library skipped)
```

---

## Task 8: End-to-end one-time setup

Test the installation commands:

```zsh
musicum completions zsh >> ~/.zshrc && source ~/.zshrc
```

```bash
musicum completions bash >> ~/.bashrc && source ~/.bashrc
```

Open a fresh shell and confirm completion works without sourcing the temp file.

---

## Adding slug completion for a future command

When you add a new subcommand with slug arguments, the only required step is:

1. Add a `SlugCompletion` entry to `SLUG_COMPLETIONS` in `apps/cli/src/commands/completions.rs`.
2. Users re-run `musicum completions zsh` (or bash) once to update their shell config.

No shell script strings need editing. The subcommand name and flags already appear in completion automatically via clap_complete.
