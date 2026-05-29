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
    SlugCompletion { path: &["play"],                           arg_index: 0, types: "file,clip,collection" },
    SlugCompletion { path: &["clips", "list"],                  arg_index: 0, types: "file" },
    SlugCompletion { path: &["clips", "show"],                  arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "create"],                arg_index: 0, types: "file" },
    SlugCompletion { path: &["clips", "apply-preset"],          arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "apply-preset"],          arg_index: 1, types: "preset" },
    SlugCompletion { path: &["clips", "clear-processors"],      arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "edit"],                  arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "set-notes"],             arg_index: 0, types: "clip" },
    SlugCompletion { path: &["clips", "delete"],                arg_index: 0, types: "clip" },
    SlugCompletion { path: &["collections", "show"],            arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "set-description"], arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "delete"],          arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "add-clip"],        arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "add-clip"],        arg_index: 1, types: "clip" },
    SlugCompletion { path: &["collections", "remove-clip"],     arg_index: 0, types: "collection" },
    SlugCompletion { path: &["collections", "remove-clip"],     arg_index: 1, types: "clip" },
    SlugCompletion { path: &["presets", "show"],                arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "delete"],              arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "edit"],                arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "add-processor"],       arg_index: 0, types: "preset" },
    SlugCompletion { path: &["presets", "remove-processor"],    arg_index: 0, types: "preset" },
    SlugCompletion { path: &["files", "show"],                  arg_index: 0, types: "file" },
    SlugCompletion { path: &["export"],                         arg_index: 0, types: "file,clip" },
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
// clap_complete generates one monolithic `_musicum()` function with everything
// inlined via nested case statements. Sub-functions are never called externally.
//
// Strategy (same as bash): rename `_musicum()` to `_musicum_clap()`, then
// write a thin `_musicum()` wrapper. The wrapper inspects `$words` to detect
// slug argument positions (using the same SLUG_COMPLETIONS registry) and either
// invokes `__musicum_slug` or falls through to `_musicum_clap`.
// ---------------------------------------------------------------------------

fn generate_zsh<C: CommandFactory>() -> Result<()> {
    let mut cmd = C::command();
    let mut buf = Vec::<u8>::new();
    generate(Shell::Zsh, &mut cmd, "musicum", &mut buf);
    let clap_out = String::from_utf8(buf).unwrap();

    // Rename so our wrapper can delegate to it.
    let clap_renamed = clap_out.replace("_musicum() {", "_musicum_clap() {");

    print!("{}", clap_renamed);
    print!("{}", zsh_slug_augmentation());
    Ok(())
}

fn zsh_slug_augmentation() -> String {
    let case_arms: Vec<String> = SLUG_COMPLETIONS
        .iter()
        .map(|e| {
            let key = if e.path.len() == 1 {
                format!("{}::{}", e.path[0], e.arg_index)
            } else {
                format!("{}::{}::{}", e.path[0], e.path[1], e.arg_index)
            };
            format!("        \"{key}\") slug_type=\"{}\" ;;", e.types)
        })
        .collect();

    format!(
        r#"
# Slug completion helper — queries musicum at tab-press time.
__musicum_slug() {{
  local completions
  completions=(${{(f)"$(musicum _complete-slugs --type "$1" 2>/dev/null)"}})
  compadd -a completions
}}

_musicum() {{
    local cur="${{words[CURRENT]}}"

    # Collect non-flag words to find the subcommand path and positional index.
    # Skip --library <value> (the only global flag that consumes a value).
    local -a _subcmds=()
    local -i _pos=0 _i _skip=0
    for (( _i=2; _i<CURRENT; _i++ )); do
        local _w="${{words[${{_i}}]}}"
        if (( _skip )); then _skip=0; continue; fi
        if [[ "$_w" == "--library" ]]; then _skip=1; continue; fi
        if [[ "$_w" == -* ]]; then continue; fi
        if (( ${{#_subcmds}} < 2 )); then _subcmds+=("$_w")
        else (( _pos++ )); fi
    done

    local _key="${{_subcmds[1]:-}}${{_subcmds[2]:+::${{_subcmds[2]}}}}::${{_pos}}"
    local slug_type=""
    case "$_key" in
{arms}
    esac

    if [[ -n "$slug_type" ]]; then
        __musicum_slug "$slug_type"
    else
        _musicum_clap "$@"
    fi
}}
compdef _musicum musicum
"#,
        arms = case_arms.join("\n")
    )
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
# Call with: __musicum_detect_slug_type "${{COMP_WORDS[@]}}" "$COMP_CWORD"
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
