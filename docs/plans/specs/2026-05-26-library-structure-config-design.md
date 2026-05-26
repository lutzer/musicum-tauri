# Library Structure & Config Redesign

## Overview

Reorganise the on-disk library layout into explicit subdirectories, introduce a
TOML config file at `~/.musicum/config.toml`, and move all config/path-resolution
logic into `musicum-core`.

---

## Config file

**Location:** `~/.musicum/config.toml`

Auto-generated on first run if the file does not exist. The initial template
includes commented-out override keys so users can see what is configurable:

```toml
# Musicum configuration

[library]
dir = "~/Musik/musicum"

# Override individual subdirectories (uncomment to customize)
# files_dir = "~/Musik/musicum/files"
# catalog_dir = "~/Musik/musicum/catalog"
# generated_dir = "~/Musik/musicum/.generated"
```

`~` is expanded to the user's home directory on load. Commented-out lines are
ignored by the TOML parser (they are only present for discoverability).

The `[library]` section may be extended in the future. Additional top-level
sections (e.g. `[audio]`, `[ui]`) may be added for future settings.

### `--library` override

When the CLI is invoked with `--library <path>`, the provided path is used as
`library_dir` and **all** individual subdirectory overrides from the config are
ignored — `LibraryPaths` is constructed purely from the given path.

---

## Directory structure

```
<library_dir>/
  files/                          # audio files + co-located sidecars
    drums.wav
    drums.musicum.json
    synths/
      pad.wav
      pad.musicum.json
  catalog/                        # queryable index and catalog data
    musicum.db
    collections/
      ep-01.musicum.json
    presets/
      lo-fi.musicum-preset.json
    attachments/
      <uuid>.<ext>
  .generated/                     # derived/cached data (can be deleted safely)
    waveforms/                    # defined here; waveform generation is future work
      file_{slug}.waveform.json
      clip_{slug}.waveform.json
    cache/
      clip_{slug}.mp3
```

The previous `<library_dir>/.musicum/` hidden directory is removed entirely.

### Default path derivation

| `LibraryPaths` field | Default | Config override key |
|---|---|---|
| `files_dir` | `library_dir/files` | `library.files_dir` |
| `catalog_dir` | `library_dir/catalog` | `library.catalog_dir` |
| `generated_dir` | `library_dir/.generated` | `library.generated_dir` |

Sub-paths derived from `LibraryPaths` (not stored in config):

| Purpose | Path |
|---|---|
| SQLite DB | `catalog_dir/musicum.db` |
| Collections | `catalog_dir/collections/` |
| Presets | `catalog_dir/presets/` |
| Attachments | `catalog_dir/attachments/` |
| Clip cache | `generated_dir/cache/` |
| Waveforms | `generated_dir/waveforms/` |

---

## Code structure

### New: `musicum-core/src/config.rs`

```rust
// Mirrors the TOML structure; future top-level sections become new fields here
pub struct AppSettings {
    pub library: LibraryConfig,
}

// Corresponds to the [library] TOML section
pub struct LibraryConfig {
    pub dir: String,
    pub files_dir: Option<String>,
    pub catalog_dir: Option<String>,
    pub generated_dir: Option<String>,
}

// Resolved absolute paths derived from AppSettings (or a --library override)
pub struct LibraryPaths {
    pub library_dir:   PathBuf,
    pub files_dir:     PathBuf,
    pub catalog_dir:   PathBuf,
    pub generated_dir: PathBuf,
}
```

Public API:

- `config_path() -> PathBuf` — returns `~/.musicum/config.toml`
- `load() -> Result<AppSettings>` — reads config, writes template if missing
- `AppSettings::library_paths(&self) -> LibraryPaths` — resolves all paths from `self.library` with `~` expansion and defaults applied
- `LibraryPaths::from_override(library_dir: &str) -> LibraryPaths` — constructs paths from a `--library` override, ignoring all config overrides

### Dependency

Add `toml` crate to `musicum-core`. Remove `serde_json` settings usage from CLI.

### Changes to existing files

| File | Change |
|---|---|
| `musicum-core/src/config.rs` | **new** — `AppSettings`, `LibraryPaths`, `load()`, `config_path()` |
| `musicum-core/src/lib.rs` | expose `pub mod config` |
| `musicum-core/src/db/mod.rs` | `connect()` takes `catalog_dir: &Path` instead of `library_dir: &str` |
| `musicum-core/src/sidecar.rs` | collection/preset helpers take `catalog_dir: &Path`; paths change from `<library_dir>/.musicum/{collections,presets}/` to `<catalog_dir>/{collections,presets}/` |
| `musicum-core/src/services/sync_service.rs` | `sync_library()` and `count_audio_files()` take `&LibraryPaths`; walk `files_dir`, read sidecars from `catalog_dir` |
| `apps/cli/src/settings.rs` | **deleted** — replaced by `musicum_core::config` |
| `apps/cli/src/main.rs` | call `musicum_core::config::load()`, apply `--library` override via `LibraryPaths::from_override()`, pass `&paths` everywhere |
| `apps/cli/src/commands/sync.rs` | use `paths.files_dir` / `paths.catalog_dir` |

### Sync service: directory skip logic

The sync walker currently skips the `.musicum` hidden directory. With audio files
isolated in `files/`, no skip is needed for the normal case. However, if a user
overrides `files_dir` to point at `library_dir` directly (edge case), `catalog/`
and `.generated/` could appear in the walk. The walker should skip any path whose
component matches `catalog` or starts with `.` (dot-prefixed directories).
