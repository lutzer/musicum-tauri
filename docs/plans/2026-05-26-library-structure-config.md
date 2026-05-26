# Library Structure & Config Redesign Implementation Plan

**Goal:** Replace the flat `.musicum/` hidden-directory layout with explicit `files/`, `catalog/`, and `.generated/` subdirectories, introduce `~/.musicum/config.toml`, and move all path-resolution logic into `musicum-core`.

**Architecture:** A new `musicum_core::config` module owns `AppSettings` (TOML-deserialised), `LibraryPaths` (derived resolved paths), and the `load()` / template-write logic. All core services switch from `library_dir: &str` to `catalog_dir: &Path` or `&LibraryPaths`. The CLI `settings.rs` is deleted; `main.rs` calls `musicum_core::config::load()`.

**Tech Stack:** Rust, `toml 0.8` crate (new dep on musicum-core), `serde/serde_derive` (already present), `tempfile` (tests, already present).

---

## File map

| Status | Path | Responsibility |
|---|---|---|
| **New** | `libs/musicum-core/src/config.rs` | `AppSettings`, `LibraryConfig`, `LibraryPaths`, `load()`, `config_path()` |
| Modify | `libs/musicum-core/Cargo.toml` | add `toml = "0.8"` |
| Modify | `libs/musicum-core/src/lib.rs` | expose `pub mod config` |
| Modify | `libs/musicum-core/src/db/mod.rs` | `connect(catalog_dir: &Path)` |
| Modify | `libs/musicum-core/src/sidecar.rs` | collection/preset helpers take `catalog_dir: &Path` |
| Modify | `libs/musicum-core/src/services/sync_service.rs` | take `&LibraryPaths`; walk `files_dir`; pass `catalog_dir` to sidecars |
| Modify | `libs/musicum-core/src/services/preset_service.rs` | take `catalog_dir: &Path`; drop unused `library_dir` params |
| Modify | `libs/musicum-core/src/services/clip_service.rs` | drop unused `_library_dir` param from `update_clip_processors` |
| Modify | `libs/musicum-core/tests/common/mod.rs` | add `make_paths(base)` helper |
| Modify | `libs/musicum-core/tests/sync_service.rs` | use `make_paths`, `&LibraryPaths` throughout |
| Modify | `libs/musicum-core/tests/clip_service.rs` | use `make_paths` |
| Modify | `libs/musicum-core/tests/preset_service.rs` | use `make_paths`, `catalog_dir` |
| **Delete** | `apps/cli/src/settings.rs` | replaced by `musicum_core::config` |
| Modify | `apps/cli/src/main.rs` | use `musicum_core::config::load()`; pass `&paths` |
| Modify | `apps/cli/src/commands/sync.rs` | `run(db, paths: &LibraryPaths)` |
| Modify | `apps/cli/src/commands/presets.rs` | `run(db, catalog_dir: &Path, args)` |
| Modify | `apps/cli/src/commands/presets_editor.rs` | `run_editor(db, catalog_dir: &Path, slug)` |
| Modify | `apps/cli/src/commands/clips.rs` | drop `library_dir` param (no longer needed) |
| Modify | `apps/cli/Cargo.toml` | remove `serde_json` (may still be needed — check) |

---

## Task 1: Add `toml` dependency

**Files:**
- Modify: `libs/musicum-core/Cargo.toml`

Add to `[dependencies]`:
```toml
toml = "0.8"
```

Run:
```
cargo build -p musicum-core
```
Expected: compiles (no code changes yet).

---

## Task 2: Create `musicum-core/src/config.rs`

**Files:**
- Create: `libs/musicum-core/src/config.rs`

```rust
use std::path::{Path, PathBuf};
use serde::Deserialize;
use anyhow::{Context, Result};

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub library: LibraryConfig,
}

#[derive(Debug, Deserialize)]
pub struct LibraryConfig {
    pub dir: String,
    pub files_dir: Option<String>,
    pub catalog_dir: Option<String>,
    pub generated_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LibraryPaths {
    pub library_dir:   PathBuf,
    pub files_dir:     PathBuf,
    pub catalog_dir:   PathBuf,
    pub generated_dir: PathBuf,
}

pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".musicum").join("config.toml")
}

pub fn load() -> Result<AppSettings> {
    let path = config_path();
    if !path.exists() {
        write_default_config(&path)?;
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading config from {}", path.display()))?;
    toml::from_str(&text).context("parsing config.toml")
}

fn write_default_config(path: &Path) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let default_dir = format!("{home}/Musik/musicum");
    let template = format!(
        r#"# Musicum configuration

[library]
dir = "{default_dir}"

# Override individual subdirectories (uncomment to customize)
# files_dir = "{default_dir}/files"
# catalog_dir = "{default_dir}/catalog"
# generated_dir = "{default_dir}/.generated"
"#
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, template)
        .with_context(|| format!("writing default config to {}", path.display()))
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(s)
    }
}

impl AppSettings {
    pub fn library_paths(&self) -> LibraryPaths {
        let library_dir = expand_tilde(&self.library.dir);
        let files_dir = self.library.files_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("files"));
        let catalog_dir = self.library.catalog_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join("catalog"));
        let generated_dir = self.library.generated_dir.as_deref()
            .map(expand_tilde)
            .unwrap_or_else(|| library_dir.join(".generated"));
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}

impl LibraryPaths {
    /// Construct from a --library override; ignores all config-file overrides.
    pub fn from_override(library_dir: &str) -> Self {
        let library_dir = expand_tilde(library_dir);
        let files_dir     = library_dir.join("files");
        let catalog_dir   = library_dir.join("catalog");
        let generated_dir = library_dir.join(".generated");
        LibraryPaths { library_dir, files_dir, catalog_dir, generated_dir }
    }
}
```

---

## Task 3: Expose config module in lib.rs

**Files:**
- Modify: `libs/musicum-core/src/lib.rs`

Add:
```rust
pub mod config;
```

Run:
```
cargo build -p musicum-core
```
Expected: compiles cleanly.

---

## Task 4: Write config unit tests

**Files:**
- Modify: `libs/musicum-core/src/config.rs` (add `#[cfg(test)]` block at the bottom)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn library_paths_defaults_from_dir() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: None,
                catalog_dir: None,
                generated_dir: None,
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/mylib/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/mylib/.generated"));
    }

    #[test]
    fn library_paths_respects_overrides() {
        let settings = AppSettings {
            library: LibraryConfig {
                dir: "/tmp/mylib".into(),
                files_dir: Some("/mnt/audio".into()),
                catalog_dir: None,
                generated_dir: Some("/mnt/gen".into()),
            },
        };
        let paths = settings.library_paths();
        assert_eq!(paths.files_dir,     PathBuf::from("/mnt/audio"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/mylib/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/mnt/gen"));
    }

    #[test]
    fn from_override_ignores_subdirs() {
        let paths = LibraryPaths::from_override("/tmp/override");
        assert_eq!(paths.files_dir,     PathBuf::from("/tmp/override/files"));
        assert_eq!(paths.catalog_dir,   PathBuf::from("/tmp/override/catalog"));
        assert_eq!(paths.generated_dir, PathBuf::from("/tmp/override/.generated"));
    }

    #[test]
    fn load_writes_default_config_if_missing() {
        let dir = tempdir().unwrap();
        // Point HOME to a temp dir so config_path() resolves inside it
        std::env::set_var("HOME", dir.path().to_str().unwrap());
        let settings = load().unwrap();
        let path = config_path();
        assert!(path.exists(), "default config should be written");
        // dir contains "Musik/musicum"
        assert!(settings.library.dir.contains("Musik"));
    }
}
```

Run:
```
cargo test -p musicum-core config
```
Expected: all 4 tests pass.

---

## Task 5: Update `db::connect()` signature

**Files:**
- Modify: `libs/musicum-core/src/db/mod.rs`

Change:
```rust
pub async fn connect(library_dir: &str) -> Result<DatabaseConnection, ServiceError> {
    let db_path = format!("{library_dir}/.musicum/musicum.db");
    let dir = std::path::Path::new(&db_path).parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let url = format!("sqlite://{db_path}?mode=rwc");
```

To:
```rust
pub async fn connect(catalog_dir: &std::path::Path) -> Result<DatabaseConnection, ServiceError> {
    std::fs::create_dir_all(catalog_dir)?;
    let db_path = catalog_dir.join("musicum.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
```

Also update `test_db()` — it uses `sqlite::memory:` so no signature change needed there.

The compiler will now flag every call site. Fix them in subsequent tasks.

---

## Task 6: Update sidecar collection/preset helpers

**Files:**
- Modify: `libs/musicum-core/src/sidecar.rs`

Change all five functions that previously used `library_dir.join(".musicum")` to use `catalog_dir` directly:

**`read_collection_sidecars`** — change parameter and path:
```rust
pub fn read_collection_sidecars(catalog_dir: &Path) -> Result<Vec<CollectionSidecar>, ServiceError> {
    let dir = catalog_dir.join("collections");
    // rest unchanged
```

**`write_collection_sidecar`** — change parameter and path:
```rust
pub fn write_collection_sidecar(catalog_dir: &Path, sc: &CollectionSidecar) -> Result<(), ServiceError> {
    let dir = catalog_dir.join("collections");
    // rest unchanged
```

**`read_preset_sidecars`** — change parameter and path:
```rust
pub fn read_preset_sidecars(catalog_dir: &Path) -> Result<Vec<PresetSidecar>, ServiceError> {
    let dir = catalog_dir.join("presets");
    // rest unchanged
```

**`read_preset_sidecar`** — change parameter and path:
```rust
pub fn read_preset_sidecar(catalog_dir: &Path, slug: &str) -> Result<PresetSidecar, ServiceError> {
    let path = catalog_dir.join("presets").join(format!("{slug}.musicum-preset.json"));
    // rest unchanged
```

**`write_preset_sidecar`** — change parameter and path:
```rust
pub fn write_preset_sidecar(catalog_dir: &Path, sc: &PresetSidecar) -> Result<(), ServiceError> {
    let dir = catalog_dir.join("presets");
    // rest unchanged
```

---

## Task 7: Update `sync_service`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs`

**`count_audio_files`** — take `files_dir` instead of `library_dir`:
```rust
pub fn count_audio_files(files_dir: &Path) -> Result<usize, ServiceError> {
    let count = WalkDir::new(files_dir)
        // remove the .musicum skip — replace with dot-dir skip:
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let p = e.path();
            // skip dot-prefixed directories and catalog/
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "catalog" { return false; }
            }
            if !p.is_file() { return false; }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            AUDIO_EXTENSIONS.contains(&ext.as_str())
        })
        .count();
    Ok(count)
}
```

**`sync_library`** — take `paths: &LibraryPaths`:
```rust
pub async fn sync_library(
    db: &DatabaseConnection,
    paths: &crate::config::LibraryPaths,
    on_progress: impl Fn(),
) -> Result<SyncReport, ServiceError> {
    let lib_path = &paths.files_dir;
    // ... walk lib_path ...
    // Replace .musicum skip with dot-dir + catalog skip (same filter as count_audio_files)
    // ...
    // sync_collections and sync_presets now take catalog_dir:
    sync_collections(db, &paths.catalog_dir).await?;
    sync_presets(db, &paths.catalog_dir, &mut report).await?;
    Ok(report)
}
```

Also update `sync_collections` and `sync_presets` signatures from `library_dir: &Path` to `catalog_dir: &Path`:
```rust
async fn sync_collections(db: &DatabaseConnection, catalog_dir: &Path) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_collection_sidecars(catalog_dir)?;
    // rest unchanged
}

async fn sync_presets(db: &DatabaseConnection, catalog_dir: &Path, report: &mut SyncReport) -> Result<(), ServiceError> {
    let sidecars = sidecar::read_preset_sidecars(catalog_dir)?;
    // rest unchanged
}
```

---

## Task 8: Update `preset_service`

**Files:**
- Modify: `libs/musicum-core/src/services/preset_service.rs`

Changes:
- `create_preset(db, catalog_dir: &Path, ...)` — remove `lib.join(".musicum").join("presets")` path construction; just call `sidecar::write_preset_sidecar(catalog_dir, &sc)` directly (sidecar now handles the subdir internally)
- Also remove the manual `sidecar_path` construction for the exists-check: use `catalog_dir.join("presets").join(format!("{slug}.musicum-preset.json"))` directly
- `delete_preset(db, catalog_dir: &Path, ...)` — same pattern
- `set_processor_param(db, catalog_dir: &Path, ...)` — replace `Path::new(library_dir)` with `catalog_dir`
- `update_preset_processors_full(db, catalog_dir: &Path, ...)` — replace `Path::new(library_dir)` with `catalog_dir`
- `update_preset_processors(db, slug, processors)` — **remove** the `library_dir` parameter entirely (it was already `_library_dir` — unused)

Full updated signatures:
```rust
pub async fn create_preset(db, catalog_dir: &Path, slug, title, description) -> Result<preset::Model, ServiceError>
pub async fn delete_preset(db, catalog_dir: &Path, slug) -> Result<(), ServiceError>
pub async fn set_processor_param(db, catalog_dir: &Path, preset_slug, instance_uuid, key, value) -> Result<(), ServiceError>
pub async fn update_preset_processors_full(db, catalog_dir: &Path, slug, processors) -> Result<(), ServiceError>
pub async fn update_preset_processors(db, slug, processors) -> Result<(), ServiceError>
```

Inside `set_processor_param` and `update_preset_processors_full`, the internal call to `update_preset_processors` loses the `library_dir` argument:
```rust
// old:
update_preset_processors(db, library_dir, preset_slug, sc.processors).await
// new:
update_preset_processors(db, preset_slug, sc.processors).await
```

---

## Task 9: Drop unused `library_dir` from `clip_service::update_clip_processors`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

The parameter `_library_dir: &str` in `update_clip_processors` is already unused (the function uses `file.path` directly). Remove it:

```rust
pub async fn update_clip_processors(
    db: &DatabaseConnection,
    clip_slug: &str,
    processors: Vec<ProcessorEntry>,
) -> Result<(), ServiceError> {
```

---

## Task 10: Update test helper `common/mod.rs`

**Files:**
- Modify: `libs/musicum-core/tests/common/mod.rs`

Add at the bottom:
```rust
/// Create the standard library subdirectory layout under `base` and return
/// a `LibraryPaths` pointing at them. Suitable for use in tests as a drop-in
/// for the old bare `dir.path()` pattern.
pub fn make_paths(base: &std::path::Path) -> musicum_core::config::LibraryPaths {
    let files_dir     = base.join("files");
    let catalog_dir   = base.join("catalog");
    let generated_dir = base.join(".generated");
    std::fs::create_dir_all(&files_dir).unwrap();
    std::fs::create_dir_all(&catalog_dir).unwrap();
    std::fs::create_dir_all(&generated_dir).unwrap();
    musicum_core::config::LibraryPaths {
        library_dir:   base.to_path_buf(),
        files_dir,
        catalog_dir,
        generated_dir,
    }
}
```

---

## Task 11: Update `tests/sync_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/sync_service.rs`

Key changes throughout the file:

1. `setup(lib_path)` becomes:
```rust
async fn setup(paths: &musicum_core::config::LibraryPaths) -> sea_orm::DatabaseConnection {
    db::connect(&paths.catalog_dir).await.unwrap()
}
```

2. Every test that previously did:
```rust
let dir = tempdir().unwrap();
let wav = dir.path().join("kick.wav");
// ...
let db = setup(dir.path()).await;
sync_service::sync_library(&db, dir.path().to_str().unwrap(), || ()).await.unwrap();
```
becomes:
```rust
let dir = tempdir().unwrap();
let paths = common::make_paths(dir.path());
let wav = paths.files_dir.join("kick.wav");
// ...
let db = setup(&paths).await;
sync_service::sync_library(&db, &paths, || ()).await.unwrap();
```

3. Tests that call `sidecar::write_preset_sidecar(dir.path(), ...)` change to `sidecar::write_preset_sidecar(&paths.catalog_dir, ...)`.

4. `count_audio_files` calls change to `sync_service::count_audio_files(&paths.files_dir)`.

5. Subdirectory test (`sync_walks_subdirectories`) — create dirs inside `paths.files_dir`:
```rust
std::fs::create_dir(paths.files_dir.join("drums")).unwrap();
common::write_sine_wav(&paths.files_dir.join("drums").join("kick.wav"), 0.1);
common::write_sine_wav(&paths.files_dir.join("drums").join("snare.wav"), 0.1);
common::write_sine_wav(&paths.files_dir.join("pad.wav"), 1.0);
```

Run after changes:
```
cargo test -p musicum-core sync_service
```
Expected: all tests pass.

---

## Task 12: Update `tests/clip_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/clip_service.rs`

`setup_with_file` changes:
```rust
async fn setup_with_file(paths: &musicum_core::config::LibraryPaths, filename: &str) -> sea_orm::DatabaseConnection {
    let wav = paths.files_dir.join(filename);
    common::write_sine_wav(&wav, 0.5);
    let db = db::connect(&paths.catalog_dir).await.unwrap();
    sync_service::sync_library(&db, paths, || ()).await.unwrap();
    db
}
```

Every test that previously did:
```rust
let dir = tempdir().unwrap();
let db = setup_with_file(dir.path(), "kick.wav").await;
let wav = dir.path().join("kick.wav");
```
becomes:
```rust
let dir = tempdir().unwrap();
let paths = common::make_paths(dir.path());
let db = setup_with_file(&paths, "kick.wav").await;
let wav = paths.files_dir.join("kick.wav");
```

Also fix the `create_clip_slug_collision` test — the wav and sidecar setup moves to `paths.files_dir`, and the `db::connect` call uses `paths.catalog_dir`.

Run:
```
cargo test -p musicum-core clip_service
```
Expected: all tests pass.

---

## Task 13: Update `tests/preset_service.rs`

**Files:**
- Modify: `libs/musicum-core/tests/preset_service.rs`

`setup()` changes:
```rust
async fn setup() -> (sea_orm::DatabaseConnection, tempfile::TempDir, musicum_core::config::LibraryPaths) {
    let dir = tempdir().unwrap();
    let paths = common::make_paths(dir.path());
    let db = db::connect(&paths.catalog_dir).await.unwrap();
    (db, dir, paths)
}
```

Every test updates accordingly — e.g.:
```rust
// old:
let (db, dir) = setup().await;
let lib = dir.path().to_str().unwrap();
preset_service::create_preset(&db, lib, "my-preset", "My Preset", "").await.unwrap();
let sc = sidecar::read_preset_sidecar(dir.path(), "my-preset").unwrap();

// new:
let (db, _dir, paths) = setup().await;
preset_service::create_preset(&db, &paths.catalog_dir, "my-preset", "My Preset", "").await.unwrap();
let sc = sidecar::read_preset_sidecar(&paths.catalog_dir, "my-preset").unwrap();
```

Also update `update_preset_processors` call — remove the `lib` argument:
```rust
// old:
preset_service::update_preset_processors(&db, lib, "p1", processors).await.unwrap();
// new:
preset_service::update_preset_processors(&db, "p1", processors).await.unwrap();
```

Run:
```
cargo test -p musicum-core preset_service
```
Expected: all tests pass.

---

## Task 14: Run full core test suite

```
cargo test -p musicum-core
```
Expected: all tests pass, zero warnings from changed code.

---

## Task 15: Delete `apps/cli/src/settings.rs`

Simply delete the file. The compiler will flag all usages in the next step.

---

## Task 16: Update `apps/cli/src/main.rs`

**Files:**
- Modify: `apps/cli/src/main.rs`

Remove:
```rust
mod settings;
```

Replace settings usage:
```rust
use musicum_core::config::{self, LibraryPaths};

// In main():
let paths = if let Some(lib) = cli.library {
    LibraryPaths::from_override(&lib)
} else {
    let settings = config::load()?;
    settings.library_paths()
};

// Config command:
if let Commands::Config = cli.command {
    println!("Config file:   {}", config::config_path().display());
    println!("Library dir:   {}", paths.library_dir.display());
    println!("Files dir:     {}", paths.files_dir.display());
    println!("Catalog dir:   {}", paths.catalog_dir.display());
    println!("Generated dir: {}", paths.generated_dir.display());
    return Ok(());
}

// DB connect:
let db = musicum_core::db::connect(&paths.catalog_dir).await?;

// Command dispatch — pass &paths where library_dir was passed:
Commands::Sync              => commands::sync::run(&db, &paths).await?,
Commands::Clips(args)       => commands::clips::run(&db, args).await?,
Commands::Collections(args) => commands::collections::run(&db, args).await?,
Commands::Presets(args)     => commands::presets::run(&db, &paths.catalog_dir, args).await?,
// ... others unchanged
```

---

## Task 17: Update `apps/cli/src/commands/sync.rs`

**Files:**
- Modify: `apps/cli/src/commands/sync.rs`

Change signature and usage:
```rust
use musicum_core::config::LibraryPaths;

pub async fn run(db: &DatabaseConnection, paths: &LibraryPaths) -> Result<()> {
    println!("Syncing library: {}", paths.library_dir.display());

    let total = sync_service::count_audio_files(&paths.files_dir).unwrap_or(0);
    // ...
    let report = sync_service::sync_library(db, paths, move || pb_tick.inc(1)).await?;
    // rest unchanged
```

---

## Task 18: Update `apps/cli/src/commands/presets.rs` and `presets_editor.rs`

**Files:**
- Modify: `apps/cli/src/commands/presets.rs`
- Modify: `apps/cli/src/commands/presets_editor.rs`

**`presets.rs`** — change signature:
```rust
pub async fn run(db: &DatabaseConnection, catalog_dir: &std::path::Path, args: PresetsArgs) -> Result<()> {
```

Replace every `library_dir` usage with `catalog_dir`. Two patterns:
- Direct `Path::new(library_dir)` → just use `catalog_dir` directly
- `preset_service::create_preset(db, library_dir, ...)` → `preset_service::create_preset(db, catalog_dir, ...)`
- `preset_service::delete_preset(db, library_dir, ...)` → `preset_service::delete_preset(db, catalog_dir, ...)`
- `sidecar::read_preset_sidecar(lib, ...)` → `sidecar::read_preset_sidecar(catalog_dir, ...)`
- `sidecar::write_preset_sidecar(lib, ...)` → `sidecar::write_preset_sidecar(catalog_dir, ...)`
- `preset_service::update_preset_processors(db, library_dir, ...)` → `preset_service::update_preset_processors(db, ...)` (no dir arg)
- `super::presets_editor::run_editor(db, library_dir, &slug)` → `super::presets_editor::run_editor(db, catalog_dir, &slug)`

Also update the empty-preset message (remove `.musicum/presets/` reference):
```rust
println!("No presets. Add a sidecar under catalog/presets/ and run sync.");
```

**`presets_editor.rs`** — change signature:
```rust
pub async fn run_editor(
    db: &DatabaseConnection,
    catalog_dir: &std::path::Path,
    preset_slug: &str,
) -> Result<()> {
```

Replace `library_dir` with `catalog_dir` in the `update_preset_processors_full` call:
```rust
preset_service::update_preset_processors_full(db, catalog_dir, preset_slug, procs)
```

---

## Task 19: Update `apps/cli/src/commands/clips.rs`

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

`clip_service::update_clip_processors` no longer takes a `library_dir` argument. Update all three call sites:
```rust
// old:
clip_service::update_clip_processors(db, library_dir, &clip_slug, new_processors).await?;
// new:
clip_service::update_clip_processors(db, &clip_slug, new_processors).await?;
```

Since `library_dir` is no longer used in `clips.rs`, remove it from the function signature:
```rust
pub async fn run(db: &DatabaseConnection, args: ClipsArgs) -> Result<()> {
```

---

## Task 20: Final build and lint

```
cargo clippy --all 2>&1 | head -60
```
Expected: zero errors, zero warnings in changed files.

```
cargo test -p musicum-core
```
Expected: all tests pass.

```
cargo build
```
Expected: clean build.

Manual smoke test:
```
cargo run -p musicum-cli -- config
```
Expected output (paths will vary by HOME):
```
Config file:   /Users/<you>/.musicum/config.toml
Library dir:   /Users/<you>/Musik/musicum
Files dir:     /Users/<you>/Musik/musicum/files
Catalog dir:   /Users/<you>/Musik/musicum/catalog
Generated dir: /Users/<you>/Musik/musicum/.generated
```
