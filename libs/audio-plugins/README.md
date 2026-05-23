# audio-plugins

Collection of audio edit types for Musicum. Each subdirectory is one plugin. The build script compiles WASM plugins and copies all descriptors + binaries to the frontend's static directory.

## Plugin modes

| Mode | Description | Compiled to WASM |
|---|---|---|
| `realtime` | Runs in the browser AudioWorklet; processed sample-by-sample in real time | yes |
| `offline` | Requires the full audio buffer (e.g. normalize); processed by the backend | no |
| `structural` | Changes the shape of the audio (e.g. trim); handled by backend only | no |

## Available plugins

| Plugin | Mode | Parameters |
|---|---|---|
| `gain` | realtime | `gain` (0–2×) |
| `reverb` | realtime | `room_size`, `damping`, `wet` (all 0–1) |
| `trim` | structural | `start`, `end` (seconds) |
| `normalize` | offline | `target_dbfs` (−40–0 dBFS) |

## Directory layout

```
libs/audio-plugins/
  build.sh              # build + deploy script
  project.json          # Nx project config
  gain/
    Cargo.toml          # realtime plugins have a Cargo.toml
    gain.json           # descriptor (id, name, version, mode, parameters)
    src/lib.rs          # AudioPlugin impl + implement_plugin! call
  reverb/
    Cargo.toml
    reverb.json
    src/lib.rs
  trim/
    trim.json           # JSON-only (structural/offline plugins have no Cargo.toml)
  normalize/
    normalize.json
```

## Build

```bash
npx nx build audio-plugins
```

The build script (`build.sh`) does the following for every plugin directory:

1. Copies `<name>.json` to `apps/frontend/static/plugins/<name>.json`.
2. If a `Cargo.toml` is present, compiles to `wasm32-unknown-unknown` (release) and copies the `.wasm` file alongside the descriptor.
3. Writes `apps/frontend/static/plugins/index.json` listing all available plugin IDs.

## Testing & linting

```bash
npx nx test audio-plugins   # cargo test (currently targets the gain crate)
npx nx lint audio-plugins   # cargo clippy
```

## Adding a new plugin

### Realtime plugin (WASM)

1. Create `libs/audio-plugins/<name>/` with:
   - `Cargo.toml` — `crate-type = ["cdylib"]`, depends on `audio-plugin-sdk`
   - `<name>.json` — descriptor with `"mode": "realtime"`
   - `src/lib.rs` — `AudioPlugin` impl + `implement_plugin!(YourPlugin)`
2. Run `npx nx build audio-plugins` — the script auto-discovers the new directory.

### Offline / structural plugin (JSON-only)

1. Create `libs/audio-plugins/<name>/<name>.json` with `"mode": "offline"` or `"mode": "structural"`.
2. Implement the processing logic in the backend (`apps/backend`).
3. Run `npx nx build audio-plugins` to deploy the descriptor.

## Descriptor format

```json
{
  "id": "gain",
  "name": "Gain",
  "version": "0.1.0",
  "mode": "realtime",
  "parameters": [
    {
      "id": "gain",
      "name": "Gain",
      "type": "float",
      "min": 0.0,
      "max": 2.0,
      "default": 1.0,
      "step": 0.01,
      "unit": "x"
    }
  ]
}
```

The frontend loads `index.json` at startup, then fetches each plugin's descriptor and (for realtime plugins) its `.wasm` binary via `PluginLoader.ts`.
