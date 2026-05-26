# Musicum Tauri — Greenfield Setup Spec

**Date:** 2026-05-22
**Status:** Reviewed
**Purpose:** Bootstrap guide for a new repo. Only the audio plugin crates and structural processor crates are carried over from the old repo. Everything else is written from scratch.

---

## What to Copy From the Old Repo

Copy these directories verbatim into the new repo:

```
libs/audio-plugin-sdk/        # Rust trait crate (AudioPlugin, AudioAnalyzer, implement_plugin!)
libs/audio-plugins/           # gain, reverb, pan, normalize, oscilloscope, level-meter
libs/structural-processor-sdk/ # Rust trait crate (structural processor chain)
libs/structural-processors/   # trim, cut, slice, crop
```

**Required change in each plugin/processor crate's `Cargo.toml`:** add a `lib` target so the crate can be linked natively (in addition to the existing WASM target):

```toml
[lib]
name = "plugin_gain"
crate-type = ["cdylib", "rlib"]   # cdylib = WASM, rlib = native linkage
```

Do the same for `structural-processors`.

Everything else — frontend, backend, database, config — is written from scratch.

---

## Repo Structure

```
musicum-tauri/
├── Cargo.toml                  # Cargo workspace root
├── Cargo.lock
├── package.json                # npm workspace root (frontend)
├── nx.json                     # Nx monorepo config (optional, for build orchestration)
│
├── apps/
│   ├── desktop/                # Tauri application
│   │   ├── src-tauri/
│   │   │   ├── Cargo.toml
│   │   │   ├── tauri.conf.json
│   │   │   ├── icons/
│   │   │   └── src/
│   │   │       ├── main.rs
│   │   │       ├── state.rs
│   │   │       ├── commands/
│   │   │       │   ├── mod.rs
│   │   │       │   ├── files.rs
│   │   │       │   ├── clips.rs
│   │   │       │   ├── collections.rs
│   │   │       │   ├── presets.rs
│   │   │       │   ├── sync.rs
│   │   │       │   ├── playback.rs
│   │   │       │   └── settings.rs
│   │   │       └── http/
│   │   │           ├── mod.rs
│   │   │           └── routes/
│   │   └── package.json        # frontend dev server integration for Tauri
│   │
│   ├── frontend/               # SvelteKit 5 app (written fresh)
│   │   ├── package.json
│   │   ├── svelte.config.js
│   │   ├── vite.config.ts
│   │   └── src/
│   │       ├── app.html
│   │       ├── routes/
│   │       ├── lib/
│   │       └── ...
│   │
│   └── cli/                    # Standalone Rust CLI (musicum binary)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           └── commands/
│               ├── mod.rs
│               ├── sync.rs
│               ├── files.rs
│               ├── clips.rs
│               ├── collections.rs
│               └── presets.rs
│
└── libs/
    ├── musicum-core/           # NEW: all business logic (written fresh)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── db/
    │       ├── services/
    │       ├── audio/
    │       └── error.rs
    │
    ├── audio-plugin-sdk/       # COPIED from old repo
    ├── audio-plugins/          # COPIED from old repo
    ├── structural-processor-sdk/ # COPIED from old repo
    └── structural-processors/  # COPIED from old repo
```

---

## Cargo Workspace

**`Cargo.toml` (workspace root):**

```toml
[workspace]
resolver = "2"
members = [
    "apps/desktop/src-tauri",
    "apps/cli",
    "libs/musicum-core",
    "libs/audio-plugin-sdk",
    "libs/audio-plugins/gain",
    "libs/audio-plugins/reverb",
    "libs/audio-plugins/pan",
    "libs/audio-plugins/normalize",
    "libs/audio-plugins/oscilloscope",
    "libs/audio-plugins/level-meter",
    "libs/structural-processor-sdk",
    "libs/structural-processors",   # single crate (unlike audio-plugins which are individual crates)
]

[workspace.dependencies]
serde       = { version = "1",    features = ["derive"] }
serde_json  = "1"
uuid        = { version = "1",    features = ["v4", "serde"] }
tokio       = { version = "1",    features = ["full"] }
sea-orm     = { version = "1",    features = ["sqlx-sqlite", "runtime-tokio-rustls", "macros"] }
thiserror   = "1"
anyhow      = "1"
tracing     = "1"
```

---

## `musicum-core`

### `Cargo.toml`

```toml
[package]
name = "musicum-core"
version = "0.1.0"
edition = "2021"

[dependencies]
# workspace
serde.workspace      = true
serde_json.workspace = true
uuid.workspace       = true
tokio.workspace      = true
sea-orm.workspace    = true
thiserror.workspace  = true
anyhow.workspace     = true
tracing.workspace    = true

# audio
symphonia   = { version = "0.5", features = ["all"] }
cpal        = "0.15"
rtrb        = "0.3"

# utils
slug        = "0.1"
chrono      = { version = "0.4", features = ["serde"] }
walkdir     = "2"

# plugins (linked natively)
audio-plugin-sdk         = { path = "../audio-plugin-sdk" }
plugin-gain              = { path = "../audio-plugins/gain" }
plugin-reverb            = { path = "../audio-plugins/reverb" }
plugin-pan               = { path = "../audio-plugins/pan" }
plugin-normalize         = { path = "../audio-plugins/normalize" }
plugin-level-meter       = { path = "../audio-plugins/level-meter" }
plugin-oscilloscope      = { path = "../audio-plugins/oscilloscope" }
structural-processor-sdk = { path = "../structural-processor-sdk" }
structural-processors    = { path = "../structural-processors" }
```

### Module layout

```
musicum-core/src/
├── lib.rs              # pub mod declarations, re-exports
├── error.rs            # ServiceError enum (thiserror)
│
├── db/
│   ├── mod.rs          # connect() → DatabaseConnection, run_create_all()
│   ├── schema.rs       # SCHEMA_VERSION constant
│   └── entities/
│       ├── mod.rs
│       ├── file.rs
│       ├── file_metadata.rs
│       ├── file_attachment.rs
│       ├── clip.rs
│       ├── collection.rs
│       ├── collection_clip.rs
│       └── preset.rs
│
├── services/
│   ├── mod.rs
│   ├── file_service.rs
│   ├── file_metadata_service.rs
│   ├── file_attachment_service.rs
│   ├── clip_service.rs
│   ├── collection_service.rs
│   ├── preset_service.rs
│   └── sync_service.rs
│
└── audio/
    ├── mod.rs              # pub use PlaybackEngine
    ├── engine.rs           # PlaybackEngine, PlaybackState, PlaybackCommand
    ├── decoder.rs          # decode_file() → Vec<f32> + AudioInfo
    ├── plugin_chain.rs     # PluginChain::process_buffer()
    ├── structural_chain.rs # StructuralChain, virtual sample cursor
    ├── cache.rs            # cache_clip() background task
    └── waveform.rs         # generate_waveform() → WaveformData
```

---

## Database Schema

SeaORM entities. No migration system — `create_table_from_entity()` on every startup. Schema version bump in `schema.rs` signals a breaking change (drop + recreate all tables in dev).

### `file`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| slug | TEXT | unique |
| name | TEXT | display name (filename without extension) |
| path | TEXT | absolute path to source audio file |
| duration | REAL | seconds |
| sample_rate | INTEGER | |
| channels | INTEGER | |
| mime_type | TEXT | |
| hash | TEXT | SHA-256 of file contents (detect changes) |
| created_at | TEXT (ISO8601) | |
| updated_at | TEXT (ISO8601) | |

### `file_metadata`

| Column | Type | Notes |
|--------|------|-------|
| file_id | TEXT (UUID) | PK, FK → file |
| bpm | REAL | nullable |
| key | TEXT | nullable |
| rating | INTEGER | nullable, 1–5 |
| color | TEXT | nullable, hex |
| notes | TEXT | |
| tags | TEXT | comma-separated |

### `file_attachment`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| file_id | TEXT (UUID) | FK → file |
| type | TEXT | "text" \| "image" \| "video" |
| text | TEXT | nullable (text attachments) |
| path | TEXT | nullable (file attachments) |
| mime_type | TEXT | nullable |
| created_at | TEXT (ISO8601) | |
| updated_at | TEXT (ISO8601) | |

### `clip`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| slug | TEXT | unique |
| file_id | TEXT (UUID) | FK → file |
| title | TEXT | |
| processors | TEXT (JSON) | ordered list of processor states |
| cached | TEXT | "no_cache" \| "caching" \| "ready" \| "error" |
| cached_path | TEXT | nullable, path to cached MP3 |
| duration | REAL | nullable, duration of cached output |
| notes | TEXT | |
| created_at | TEXT (ISO8601) | |
| updated_at | TEXT (ISO8601) | |

### `collection`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| slug | TEXT | unique |
| title | TEXT | |
| description | TEXT | |
| background_path | TEXT | nullable |
| created_at | TEXT (ISO8601) | |
| updated_at | TEXT (ISO8601) | |

### `collection_clip`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| collection_id | TEXT (UUID) | FK → collection |
| clip_id | TEXT (UUID) | FK → clip |
| position | INTEGER | ordering index |

Unique constraint on `(collection_id, clip_id)`.

### `preset`

| Column | Type | Notes |
|--------|------|-------|
| id | TEXT (UUID) | PK |
| slug | TEXT | unique |
| title | TEXT | |
| description | TEXT | |
| processors | TEXT (JSON) | same format as clip.processors |
| created_at | TEXT (ISO8601) | |
| updated_at | TEXT (ISO8601) | |

---

## Sidecar File Formats

Sidecars are the source of truth. The DB is a queryable index rebuilt from sidecars.

### Audio file sidecar — `{filename}.musicum.json`

Lives next to the source audio file.

```json
{
  "version": 1,
  "metadata": {
    "bpm": null,
    "key": null,
    "rating": null,
    "color": null,
    "notes": "",
    "tags": ""
  },
  "attachments": [
    {
      "uuid": "550e8400-e29b-41d4-a716-446655440000",
      "type": "image",
      "mime_type": "image/jpeg"
    }
  ],
  "clips": [
    {
      "slug": "recording-clean",
      "title": "Clean",
      "notes": "",
      "processors": []
    },
    {
      "slug": "recording-reverb",
      "title": "With Reverb",
      "notes": "",
      "processors": [
        {
          "type": "plugin",
          "id": "reverb",
          "enabled": true,
          "params": { "room_size": 0.6, "wet": 0.3 }
        }
      ]
    }
  ]
}
```

### Collection sidecar — `collections/{slug}.musicum.json`

```json
{
  "version": 1,
  "slug": "my-album",
  "title": "My Album",
  "description": "",
  "clips": ["recording-clean", "beat-reverb"]
}
```

`clips` is an ordered array of clip slugs.

### Preset sidecar — `presets/{slug}.musicum-preset.json`

```json
{
  "version": 1,
  "slug": "reverb-master",
  "title": "Reverb Master",
  "description": "",
  "processors": [
    { "type": "plugin", "id": "reverb",    "enabled": true, "params": { "room_size": 0.8 } },
    { "type": "plugin", "id": "normalize", "enabled": true, "params": { "target_lufs": -14 } }
  ]
}
```

### Processor entry format (shared by `clip.processors` and `preset.processors`)

```json
{ "type": "plugin",     "id": "gain",  "enabled": true, "params": { "level": -3.0 } }
{ "type": "structural", "id": "trim",  "enabled": true, "params": { "start_ms": 200, "end_ms": 0 } }
```

---

## Filesystem Layout (Runtime)

```
<library_dir>/                        # set by user in settings, persisted in app config
  files/                              # can be manually overridem in the config
    drums.wav
    drums.musicum.json
    synths/
      pad.wav
      pad.musicum.json
  catalog/                            # location can be manually overridem in the config
    musicum.db
    collections/
      ep-01.musicum.json
    presets/
      lo-fi.musicum-preset.json
    attachments/
      550e8400-e29b-41d4-a716-446655440000.jpg
  .generated/                         # location can be manually overriden in the config
    waveforms/
      file_{slug}.waveform.json         # raw file waveform
      clip_{slug}.waveform.json         # processed clip waveform
    cache/
      clip_{slug}.mp3
```

App config (Tauri app data dir):

```json
{
  "library_dir": "/Users/lutz/Music/Musicum",
  "generated_dir": null,
  "http_server_enabled": false,
  "http_server_port": 8000
}
```

---

## Tauri Shell

### `src-tauri/Cargo.toml`

```toml
[package]
name = "musicum-desktop"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri        = { version = "2", features = ["shell-open"] }
musicum-core = { path = "../../../libs/musicum-core" }
axum         = { version = "0.7", optional = true }
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace  = true

[features]
http-server = ["axum"]
```

### `state.rs`

```rust
use musicum_core::audio::PlaybackEngine;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AppState {
    pub db: DatabaseConnection,
    pub engine: Arc<Mutex<PlaybackEngine>>,
    pub settings: Arc<Mutex<AppSettings>>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct AppSettings {
    pub library_dir: String,
    pub generated_dir: Option<String>,
    pub http_server_enabled: bool,
    pub http_server_port: u16,
}
```

`AppSettings` is persisted as JSON at `{tauri_app_config_dir}/settings.json` (resolved via `tauri::api::path::app_config_dir`). On startup, `main.rs` reads and deserializes this file (defaulting to `AppSettings::default()` if absent). Any `settings::set_*` command writes the full struct back to the same file after mutating the in-memory value.

### `main.rs` skeleton

```rust
fn main() {
    tauri::Builder::default()
        .manage(/* build AppState: open DB, init engine */)
        .invoke_handler(tauri::generate_handler![
            // files
            commands::files::get_files,
            commands::files::get_file,
            commands::files::update_file,
            commands::files::delete_file,
            // clips
            commands::clips::get_clips,
            commands::clips::get_clip,
            commands::clips::create_clip,
            commands::clips::update_clip,
            commands::clips::delete_clip,
            commands::clips::cache_clip,
            // collections
            commands::collections::get_collections,
            commands::collections::get_collection,
            commands::collections::create_collection,
            commands::collections::update_collection,
            commands::collections::delete_collection,
            commands::collections::reorder_clips,
            // presets
            commands::presets::get_presets,
            commands::presets::create_preset,
            commands::presets::update_preset,
            commands::presets::delete_preset,
            commands::presets::apply_preset,
            // sync
            commands::sync::sync_library,
            // playback
            commands::playback::play,
            commands::playback::pause,
            commands::playback::stop,
            commands::playback::seek,
            commands::playback::set_processor_param,
            commands::playback::get_playback_state,
            // settings
            commands::settings::get_settings,
            commands::settings::set_library_dir,
            commands::settings::set_generated_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error running Tauri app");
}
```

### Command pattern

```rust
// commands/clips.rs
#[tauri::command]
pub async fn get_clips(
    file_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ClipResponse>, String> {
    let id = Uuid::parse_str(&file_id).map_err(|e| e.to_string())?;
    musicum_core::services::clip_service::get_clips_for_file(&state.db, id)
        .await
        .map_err(|e| e.to_string())
}
```

### Tauri events (Rust → frontend)

Emitted via `app_handle.emit(event, payload)`:

| Event | Payload type | Description |
|-------|-------------|-------------|
| `playback:position` | `{ seconds: number }` | Current playhead position |
| `playback:state` | `"playing" \| "paused" \| "stopped"` | State changes |
| `clip:cache_progress` | `{ clip_id: string, percent: number }` | Caching progress |
| `clip:cache_done` | `{ clip_id: string, status: string }` | Cache complete |
| `sync:progress` | `{ message: string }` | Sync status message |
| `sync:done` | `{ added: number, updated: number, removed: number }` | Sync complete |

---

## Frontend (SvelteKit 5 — written fresh)

### Setup

```bash
npm create svelte@latest apps/frontend
# choose: SvelteKit, TypeScript, no additional tooling
```

### Key dependencies

```json
{
  "@tauri-apps/api": "^2",
  "@tauri-apps/plugin-shell": "^2"
}
```

### Structure

```
apps/frontend/src/
├── app.html
├── routes/
│   ├── +layout.svelte          # top nav, global state init
│   ├── +page.svelte            # home / library overview
│   ├── files/
│   │   ├── +page.svelte        # file browser
│   │   └── [f_slug]/
│   │       └── +page.svelte    # file detail + clips list
│   ├── clips/
│   │   └── [c_slug]/
│   │       └── +page.svelte    # clip editor
│   ├── collections/
│   │   ├── +page.svelte        # collection browser
│   │   └── [col_slug]/
│   │       └── +page.svelte    # collection detail + playback
│   ├── presets/
│   │   └── +page.svelte        # preset browser + batch apply
│   └── settings/
│       └── +page.svelte        # library dir, generated dir
│
└── lib/
    ├── api/
    │   ├── client.ts           # invoke() wrapper
    │   ├── files.ts
    │   ├── clips.ts
    │   ├── collections.ts
    │   ├── presets.ts
    │   ├── sync.ts
    │   ├── playback.ts
    │   └── settings.ts
    ├── stores/
    │   ├── playback.svelte.ts      # listens to playback:* events
    │   ├── clip-processors.svelte.ts  # undo/redo + debounced persist
    │   └── plugin-registry.svelte.ts  # plugin descriptors (id, params, ranges)
    ├── components/
    │   ├── audio/
    │   │   ├── ProcessorRack.svelte    # ordered list of active processors
    │   │   ├── ProcessorItem.svelte    # single processor (params UI)
    │   │   ├── ProcessorPicker.svelte  # add processor dialog
    │   │   ├── Waveform.svelte         # waveform visualization
    │   │   └── PlaybackBar.svelte      # play/pause/seek controls + position
    │   ├── FileRow.svelte
    │   ├── ClipRow.svelte
    │   ├── CollectionRow.svelte
    │   ├── PresetRow.svelte
    │   └── forms/
    ├── types/
    │   ├── file.ts
    │   ├── clip.ts
    │   ├── collection.ts
    │   ├── preset.ts
    │   └── playback.ts
    └── utils.ts
```

### API client (`lib/api/client.ts`)

```typescript
import { invoke } from '@tauri-apps/api/core'

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args)
}
```

All API modules use `call()`:

```typescript
// lib/api/clips.ts
import { call } from './client'
import type { ClipResponse, CreateClipRequest, UpdateClipRequest } from '$lib/types/clip'

export const getClips      = (fileId: string) => call<ClipResponse[]>('get_clips', { fileId })
export const getClip       = (slug: string)   => call<ClipResponse>('get_clip', { slug })
export const createClip    = (req: CreateClipRequest) => call<ClipResponse>('create_clip', req)
export const updateClip    = (slug: string, req: UpdateClipRequest) => call<ClipResponse>('update_clip', { slug, ...req })
export const deleteClip    = (slug: string)   => call<void>('delete_clip', { slug })
export const cacheClip     = (slug: string)   => call<void>('cache_clip', { slug })
```

### Playback store (`lib/stores/playback.svelte.ts`)

```typescript
import { listen } from '@tauri-apps/api/event'
import { play, pause, stop, seek } from '$lib/api/playback'

let position = $state(0)
let state    = $state<'playing' | 'paused' | 'stopped'>('stopped')

listen<{ seconds: number }>('playback:position', e => { position = e.payload.seconds })
listen<string>('playback:state', e => { state = e.payload as typeof state })

export const playback = { get position() { return position }, get state() { return state }, play, pause, stop, seek }
```

### Plugin descriptor loading (`lib/stores/plugin-registry.svelte.ts`)

Plugin WASM is not used for audio processing, but descriptor JSON (parameter names, ranges, defaults) is still needed to render the processor UI. Descriptors are bundled as static JSON files under `apps/frontend/src/lib/plugin-descriptors/` (one file per plugin, e.g. `reverb.json`). These are hand-authored alongside the plugin crates and imported statically at build time — no Tauri command or WASM loading required at runtime.

---

## CLI (`apps/cli`)

A standalone `musicum` binary that links `musicum-core` directly. Works without the desktop app running. SQLite WAL mode (enabled by `musicum-core`'s `connect()`) allows safe concurrent access if the desktop app is open at the same time.

### `Cargo.toml`

```toml
[package]
name = "musicum-cli"
version = "0.1.0"
edition = "2021"
default-run = "musicum"

[[bin]]
name = "musicum"
path = "src/main.rs"

[dependencies]
musicum-core = { path = "../../libs/musicum-core" }
clap         = { version = "4", features = ["derive"] }
tokio.workspace = true
serde_json.workspace = true
anyhow.workspace = true
```

### Command surface

```
musicum sync                          # walk library dir, update DB + sidecars
musicum files list                    # list all files (table output)
musicum files show <slug>             # show file detail + clips
musicum clips list <file-slug>        # list clips for a file
musicum clips create <file-slug> --title "Name"
musicum clips cache <clip-slug>       # run caching pipeline (requires ffmpeg)
musicum collections list
musicum collections show <slug>
musicum presets list
musicum presets apply <preset-slug> <clip-slug>
```

### `main.rs` skeleton

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "musicum", about = "Musicum audio library CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Sync,
    Files(commands::files::FilesArgs),
    Clips(commands::clips::ClipsArgs),
    Collections(commands::collections::CollectionsArgs),
    Presets(commands::presets::PresetsArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let settings = load_settings()?;   // reads same settings.json as desktop app
    let db = musicum_core::db::connect(&settings.library_dir).await?;

    match cli.command {
        Commands::Sync => commands::sync::run(&db, &settings).await?,
        Commands::Files(args) => commands::files::run(&db, args).await?,
        Commands::Clips(args) => commands::clips::run(&db, args).await?,
        Commands::Collections(args) => commands::collections::run(&db, args).await?,
        Commands::Presets(args) => commands::presets::run(&db, args).await?,
    }
    Ok(())
}
```

`load_settings()` reads the same `settings.json` from the Tauri app config dir (`{home}/.config/com.musicum.app/settings.json` on Linux/Mac) so the CLI and desktop app share one config.

### Output format

- Default: human-readable table (via simple `println!` / `format!`)
- `--json` flag on all list/show commands: pretty-printed JSON (useful for scripting)

---

## Audio Engine Design

### `engine.rs` — `PlaybackEngine`

```rust
pub struct PlaybackEngine {
    stream: Option<cpal::Stream>,
    command_tx: rtrb::Producer<PlaybackCommand>,
    /// Shared with the audio callback; callback writes, main thread reads.
    position_secs: Arc<AtomicU64>,  // bits reinterpreted as f64 via f64::from_bits
}

pub enum PlaybackCommand {
    Play { clip_id: Uuid },
    Pause,
    Stop,
    Seek { seconds: f64 },
    SetParam { processor_index: usize, param_id: String, value: f64 },
}

pub enum PlaybackState { Playing, Paused, Stopped }
```

Audio callback (cpal thread — must be lock-free, no allocation):
1. Drain any pending `PlaybackCommand` from `command_rx` (the `rtrb::Consumer` end, moved into the closure at stream creation)
2. Read next buffer from `StructuralChain` (handles trim/cut/slice as virtual cursor)
3. Pass buffer through `PluginChain::process_buffer()` (calls each `AudioPlugin::process()`)
4. Write to cpal output buffer
5. Store updated position via `position_secs.store(f64::to_bits(pos), Ordering::Relaxed)`

The main thread polls `position_secs` on a timer (e.g., every 50 ms via `tokio::time::interval`) and emits `playback:position` events to the frontend. Using `AtomicU64` avoids any lock or channel on the hot path.

### `cache.rs` — caching pipeline

```rust
pub async fn cache_clip(
    db: &DatabaseConnection,
    clip: &ClipModel,
    generated_dir: &Path,
    app: tauri::AppHandle,
) -> Result<(), ServiceError>
```

Steps:
1. Set `clip.cached = "caching"` in DB
2. Decode source file with symphonia → `Vec<f32>` + `AudioInfo`
3. Apply `StructuralChain` → modified sample buffer
4. Apply `PluginChain` offline (same trait methods, non-realtime) → processed buffer
5. Encode to MP3 via `ffmpeg` subprocess
6. Generate waveform JSON (downsample to ~1000 points per channel)
7. Write both files to `generated_dir`
8. Set `clip.cached = "ready"`, update `cached_path`, `duration` in DB
9. Emit `clip:cache_done` event

---

## Build Tooling

### Development

```bash
# Install Tauri CLI
cargo install tauri-cli

# Run desktop app (starts frontend dev server + Tauri window)
cargo tauri dev

# Run frontend dev server alone
cd apps/frontend && npm run dev

# Build WASM plugins (for descriptor JSON generation)
npx nx build audio-plugins

# Run musicum-core tests
cargo test -p musicum-core
```

### Production build

```bash
cargo tauri build   # produces platform-specific installer in target/release/bundle/
```

### `tauri.conf.json` (key settings)

```json
{
  "build": {
    "beforeDevCommand": "cd apps/frontend && npm run dev",
    "beforeBuildCommand": "cd apps/frontend && npm run build",
    "devUrl": "http://localhost:5173",
    "frontendDist": "../../frontend/build"
  },
  "app": {
    "windows": [{ "title": "Musicum", "width": 1280, "height": 800 }]
  },
  "bundle": {
    "identifier": "com.musicum.app",
    "targets": "all"
  }
}
```

---

## Key Rust Dependencies Summary

| Crate | Version | Purpose |
|-------|---------|---------|
| `tauri` | 2 | Desktop shell, IPC, events |
| `sea-orm` | 1 | ORM + SQLite |
| `symphonia` | 0.5 | Audio decoding (WAV, MP3, FLAC, OGG, AIFF) |
| `cpal` | 0.15 | Cross-platform audio output |
| `rtrb` | 0.3 | Lock-free ring buffer (audio thread params) |
| `axum` | 0.7 | Optional HTTP adapter |
| `tokio` | 1 | Async runtime |
| `serde` / `serde_json` | 1 | JSON (sidecars, processors, IPC) |
| `uuid` | 1 | ID generation |
| `slug` | 0.1 | Slug generation |
| `walkdir` | 2 | Library directory traversal |
| `chrono` | 0.4 | Timestamps |
| `thiserror` | 1 | Error types |
| `tracing` | 1 | Structured logging |

`ffmpeg` is a system dependency (subprocess) used only for MP3 encoding in the caching pipeline. All other audio I/O uses pure-Rust crates.

---

## Implementation Order

Suggested order to get to a working app incrementally:

1. **Cargo workspace** — set up workspace, copy plugin/processor libs, fix `crate-type`
2. **`musicum-core` skeleton** — `lib.rs`, `error.rs`, empty module stubs
3. **DB layer** — SeaORM entities, `connect()`, `create_all()`, schema version
4. **Services** — `file_service`, `clip_service`, `sync_service` (file walk + sidecar read/write)
5. **Tauri shell** — `main.rs`, `state.rs`, wire up `sync_library` command, settings commands
6. **SvelteKit skeleton** — fresh app, `client.ts`, file browser page talking to `get_files`
7. **Audio engine** — `decoder.rs`, `plugin_chain.rs`, `structural_chain.rs`, `engine.rs` (cpal)
8. **Clip editor UI** — `ProcessorRack`, `ProcessorItem`, `PlaybackBar`, playback store
9. **Caching pipeline** — `cache.rs`, waveform generation, `Waveform.svelte`
10. **Collections + Presets** — service + commands + UI
11. **CLI** — `apps/cli`, clap commands wrapping the same services, `--json` flag
12. **HTTP adapter** — Axum routes (thin wrappers over same services)
