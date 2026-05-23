# audio-plugin-sdk

Shared Rust crate defining the `AudioPlugin` and `AudioAnalyzer` traits, their `implement_plugin!` / `implement_analyzer!` macros, and all descriptor/parameter types used by every Musicum audio plugin. The `js/` subdirectory is the companion TypeScript package (`@musicum/audio-plugin-sdk`) that wraps compiled WASM plugins for browser use.

---

## Rust SDK

### `AudioPlugin` trait

Every plugin implements this trait:

```rust
pub trait AudioPlugin: Sized {
    fn descriptor() -> &'static PluginDescriptor;
    fn new() -> Self;
    fn set_parameter(&mut self, id: &str, value: f32);
    fn get_parameter(&self, id: &str) -> f32;
    fn process(&mut self, samples: &mut [f32], channels: usize, sample_rate: f32, timestamp_secs: f64);
    fn render_snapshot(&mut self) -> &[u8] { &[] }  // optional; required for Canvas parameters
}
```

`process` receives an interleaved `f32` buffer (length is always a multiple of `channels`) and modifies it in-place. `render_snapshot` returns raw bytes for canvas rendering; leave it as the default no-op if the plugin has no `Canvas` parameters.

### `implement_plugin!(T)` macro

Expands to the full C ABI required by the Musicum plugin runtime:

| Export | Description |
|---|---|
| `__ap_alloc / __ap_free` | WASM linear-memory helpers for the AudioWorklet |
| `__ap_new / __ap_drop` | Plugin lifecycle |
| `__ap_descriptor_ptr / __ap_descriptor_len` | Pointer + length of the cached JSON descriptor |
| `__ap_set_parameter(id_ptr, id_len, value)` | Set a parameter by UTF-8 ID |
| `__ap_get_parameter(id_ptr, id_len) -> f32` | Get a parameter value |
| `__ap_process(ptr, len, channels, sample_rate, timestamp_secs)` | In-place audio processing |
| `__ap_render_snapshot()` | Returns packed `(ptr << 32 \| len)` u64; bytes owned by the plugin |

A single static instance is kept per WASM module — safe because WASM is single-threaded.

### Plugin descriptor

Descriptors are defined directly in Rust as a `&'static PluginDescriptor`. The macro calls `descriptor()` once and caches the resulting JSON string for the lifetime of the WASM module.

```rust
use audio_plugin_sdk::{PluginDescriptor, PluginMode, PluginParameter};

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "gain",
    name: "Gain",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &[
        PluginParameter::Float {
            id: "gain",
            name: "Gain",
            min: 0.0,
            max: 4.0,
            default: 1.0,
            step: 0.01,
            unit: "x",
            disabled: false,
            hidden: false,
        },
    ],
};
```

`PluginDescriptor::to_json()` serializes this to the JSON descriptor served to the frontend.

### `PluginParameter` variants

| Variant | Fields | Purpose |
|---|---|---|
| `Float` | `id, name, min, max, default, step, unit, disabled, hidden` | Knob / slider |
| `Bool` | `id, name, default, disabled, hidden` | Toggle |
| `Action` | `id, name, disabled` | Trigger button — no persistent value |
| `Canvas` | `id, name, aspect_ratio, disabled` | Plugin-driven canvas element |

`disabled: true` renders the control as inactive. `hidden: true` removes it from the rack UI entirely (used for internal computed parameters like `__computed_gain`).

### Plugin execution modes

| Mode | Where it runs | Description |
|---|---|---|
| `Realtime` | AudioWorklet | DSP effect applied sample-by-sample in real time |
| `Offline` | Backend only | Applied during caching; not streamed in the browser |
| `Analyzed` | AudioWorklet + WasmAnalyzer | Triggers whole-file analysis first, feeds computed params back |

### `AudioAnalyzer` trait

Plugins with `mode: PluginMode::Analyzed` also implement this trait. The analyzer runs on the main thread (not in the AudioWorklet) and performs a full-file pass before the plugin processes audio.

```rust
pub trait AudioAnalyzer: Sized {
    type Plugin: AudioPlugin;

    fn new() -> Self;

    /// Receive plugin parameters before analysis begins. Default is a no-op.
    fn init(&mut self, params: ParamMap) {}

    /// Called repeatedly with chunks of the audio file.
    fn analyze(&mut self, samples: &[f32], channels: usize, sample_rate: f32, timestamp_secs: f64);

    /// Called once after all chunks have been fed. Return computed values.
    fn finish_analysis(&self) -> ParamResult;
}
```

### `implement_analyzer!(T)` macro

Expands to the C ABI used by `WasmAnalyzer` on the main thread:

| Export | Description |
|---|---|
| `__aa_alloc / __aa_free` | Memory helpers |
| `__aa_create / __aa_reset` | Create / reset analyzer instance |
| `__aa_init(ptr, len)` | Deserialize JSON params → build `ParamMap` → call `init()` |
| `__aa_analyze(ptr, len, channels, sample_rate, timestamp_secs)` | Feed one audio chunk |
| `__aa_result_ptr / __aa_result_len` | Lazily evaluated cached JSON result from `finish_analysis()` |

### `ParamMap`

`ParamMap` is passed to `AudioAnalyzer::init`. It is constructed inside the `__aa_init` ABI function by calling `PluginDescriptor::parse_params(json)` on the JSON payload sent from the JS SDK.

It holds two things:
- The parsed values from the JSON payload
- A reference back to the static `&[PluginParameter]` descriptor slice

`get_float` / `get_bool` resolve in this order:

1. **JSON payload value** — what the user set in the UI
2. **Descriptor `default`** — from the static `PluginParameter` definition in Rust
3. **Zero / false** — for completely unknown IDs

This means `init` implementations never need to handle missing parameters explicitly:

```rust
fn init(&mut self, params: ParamMap) {
    // Returns the user's value, or -3.0 from the descriptor default if not present
    self.target_dbfs = params.get_float("target_dbfs");
}
```

### `ParamResult`

Returned by `finish_analysis()`. Built with a chained `.with()` builder:

```rust
fn finish_analysis(&self) -> ParamResult {
    ParamResult::new()
        .with("__computed_gain", self.computed_gain)
}
```

`to_json()` serializes the result to `{"key": value, ...}` JSON, which is sent back to the AudioWorklet via the `set_data` message and delivered to the plugin via `receive_data()`.

---

## Writing a new plugin

### Realtime plugin

```rust
use audio_plugin_sdk::{AudioPlugin, PluginDescriptor, PluginMode, PluginParameter, implement_plugin};

struct MyPlugin { volume: f32 }

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "my_plugin",
    name: "My Plugin",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &[
        PluginParameter::Float {
            id: "volume", name: "Volume",
            min: 0.0, max: 2.0, default: 1.0, step: 0.01, unit: "x",
            disabled: false, hidden: false,
        },
    ],
};

impl AudioPlugin for MyPlugin {
    fn descriptor() -> &'static PluginDescriptor { &DESCRIPTOR }
    fn new() -> Self { MyPlugin { volume: 1.0 } }
    fn set_parameter(&mut self, id: &str, value: f32) {
        if id == "volume" { self.volume = value; }
    }
    fn get_parameter(&self, id: &str) -> f32 {
        if id == "volume" { self.volume } else { 0.0 }
    }
    fn process(&mut self, samples: &mut [f32], _channels: usize, _sample_rate: f32, _timestamp_secs: f64) {
        for s in samples.iter_mut() { *s *= self.volume; }
    }
}

implement_plugin!(MyPlugin);
```

### Analyzed-mode plugin

Analyzed-mode plugins also ship an analyzer WASM (same crate, separate `wasm-pack` target). The analyzer runs a full-file pass and returns computed parameters which are fed back into the AudioWorklet plugin.

```rust
use audio_plugin_sdk::{
    AudioPlugin, AudioAnalyzer, ParamMap, ParamResult,
    PluginDescriptor, PluginMode, PluginParameter,
    implement_plugin, implement_analyzer,
};

// --- Descriptor ---

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "my_analyzer_plugin",
    name: "My Analyzer Plugin",
    version: "0.1.0",
    mode: PluginMode::Analyzed,
    parameters: &[
        PluginParameter::Float {
            id: "target_db", name: "Target dB",
            min: -40.0, max: 0.0, default: -3.0, step: 0.1, unit: "dB",
            disabled: false, hidden: false,
        },
        PluginParameter::Float {
            id: "__computed_gain", name: "Computed Gain",
            min: 0.0, max: 100.0, default: 1.0, step: 0.001, unit: "x",
            disabled: true, hidden: true,
        },
    ],
};

// --- Plugin (runs in AudioWorklet) ---

struct MyPlugin { computed_gain: f32 }

impl AudioPlugin for MyPlugin {
    fn descriptor() -> &'static PluginDescriptor { &DESCRIPTOR }
    fn new() -> Self { MyPlugin { computed_gain: 1.0 } }
    fn set_parameter(&mut self, id: &str, value: f32) {
        if id == "__computed_gain" { self.computed_gain = value; }
    }
    fn get_parameter(&self, id: &str) -> f32 {
        if id == "__computed_gain" { self.computed_gain } else { 0.0 }
    }
    fn process(&mut self, samples: &mut [f32], _channels: usize, _sample_rate: f32, _timestamp_secs: f64) {
        for s in samples.iter_mut() { *s *= self.computed_gain; }
    }
}

implement_plugin!(MyPlugin);

// --- Analyzer (runs on main thread) ---

struct MyAnalyzer { peak: f32, target_db: f32 }

impl AudioAnalyzer for MyAnalyzer {
    type Plugin = MyPlugin;

    fn new() -> Self { MyAnalyzer { peak: 0.0, target_db: -3.0 } }

    fn init(&mut self, params: ParamMap) {
        // Falls back to -3.0 from the descriptor default if not in the JSON payload
        self.target_db = params.get_float("target_db");
    }

    fn analyze(&mut self, samples: &[f32], _channels: usize, _sample_rate: f32, _timestamp_secs: f64) {
        for &s in samples {
            if s.abs() > self.peak { self.peak = s.abs(); }
        }
    }

    fn finish_analysis(&self) -> ParamResult {
        let target_linear = 10_f32.powf(self.target_db / 20.0);
        let gain = target_linear / self.peak.max(f32::EPSILON);
        ParamResult::new().with("__computed_gain", gain)
    }
}

implement_analyzer!(MyAnalyzer);
```

---

## JavaScript SDK (`js/`)

`js/` is a standalone TypeScript package (`@musicum/audio-plugin-sdk`) that wraps WASM plugins for browser use.

### File structure

```
js/src/
  types.ts              # All public TypeScript types and interfaces
  worklet-processor.js  # AudioWorkletProcessor source (plain JS, imported via ?raw)
  plugin-loader.ts      # createPlugin, loadPluginDescriptor, AudioPluginImpl
  analyzer.ts           # WasmAnalyzer class
  audio-parser.ts       # parseAudio — decodes MP3/OGG/WAV via OfflineAudioContext
  plugin-manager.ts     # createPluginManager, AudioPluginChain
  index.ts              # Barrel re-export — the public entry point
  vite-env.d.ts         # Enables ?raw import typing for Vite/Vitest
```

### Public API

**`createPlugin(pluginUrl, audioUrl, initialParams?)`** — loads a plugin and returns an `AudioPlugin`:

```typescript
const plugin = await createPlugin('/plugins/gain', '/audio/file.wav', { gain: 1.2 });
```

The returned `AudioPlugin` exposes:

| Member | Description |
|---|---|
| `descriptor` | Static descriptor object |
| `handle` | `AudioPluginHandle` — `getParameter(id)`, `setParameter(id, value)` |
| `analyzer` | `AudioPluginAnalyzer \| undefined` — only present for `analyzed` mode plugins |
| `enabled` | Getter/setter — bypasses processing when `false` |
| `createNode(ctx)` | Creates and returns an `AudioWorkletNode` (call once per `AudioContext`) |
| `initRenderer(canvases)` | Attach canvas elements for `Canvas` parameters; starts RAF render loop |
| `readParams()` | Async — reads all float/bool param values from the worklet |
| `writeParams()` | Sync — writes current `params` map to the worklet |

**`loadPluginDescriptor(url)`** — fetches `${url}.json` only; use for building a plugin registry without loading WASM.

**`createPluginManager()`** — returns an `AudioPluginManager`:

```typescript
const manager = createPluginManager();
manager.sync(editStates);                          // add / remove / update plugins
const chain = manager.createWorkletChain(ctx, sourceNode);  // connect to audio graph
```

`AudioPluginChain.sync(editStates)` reconnects the chain whenever the plugin list or order changes.

**`WasmAnalyzer`** — runs an `AudioAnalyzer` WASM binary on the main thread:

```typescript
const result = await new WasmAnalyzer('/plugins/normalize.wasm').run('/audio/file.wav', paramsJson);
```

Internally it fetches WASM + audio in parallel, decodes the audio, and feeds it in chunks through the `__aa_*` ABI.

### How `analyzed` mode works

1. Plugin is created with `mode: 'analyzed'`; the `__analyzed` bool parameter starts as `false`
2. `createPlugin` instantiates both the AudioWorklet plugin and an `AudioPluginAnalyzerImpl`
3. On init, if `__analyzed !== true`, `runAnalysis(params)` is triggered automatically
4. `WasmAnalyzer.run()` fetches the analyzer WASM + audio file, feeds audio in chunks through `__aa_create → __aa_init → __aa_analyze* → __aa_result_ptr/len`, and returns a JSON string
5. The result is posted to the worklet via a `set_data` message → the plugin's `receive_data()` is called
6. The plugin stores computed values (e.g. `__computed_gain`) and marks `__analyzed = true` so re-analysis is skipped on the next load

---

## Commands

```bash
npx nx test audio-plugin-sdk        # cargo test (Rust)
npx nx lint audio-plugin-sdk        # cargo clippy (Rust)
cd libs/audio-plugin-sdk/js && npm test         # vitest (JS SDK), run once
cd libs/audio-plugin-sdk/js && npm run test:watch  # vitest watch mode
```
