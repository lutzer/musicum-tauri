## Musicum Tauri

Musicum is an app for organizing sound recordings, editing them non-destructively and publishing collections of them.

### Features

*  basic post processing on tracks through structural edits and audio plugins
*  create track collections and publish them as a self conained html file
*  save patch/creation notes to uploaded as images, videos, text
*  import/export tracks
*  automated data backup

## CLI

The `musicum` binary links directly to `musicum-core` and provides library management without the desktop app.

```bash
cargo build -p musicum-cli
./target/debug/musicum --help
```

All commands accept `--library <path>` to override the configured library directory.

| Command | Description |
|---------|-------------|
| `musicum config` | Print settings file path and current library directory |
| `musicum sync` | Walk the library directory and sync DB + sidecars |
| `musicum files list [--json]` | List all files in the library |
| `musicum files show <slug> [--json]` | Show file detail including clips |
| `musicum clips list <file-slug> [--json]` | List clips for a file |
| `musicum clips show <slug> [--json]` | Show clip detail including processor chain |
| `musicum collections list [--json]` | List all collections |
| `musicum collections show <slug> [--json]` | Show collection detail |
| `musicum presets list [--json]` | List all presets |
| `musicum presets show <slug> [--json]` | Show preset detail including processor chain |

Settings are read from `~/.config/com.musicum.app/settings.json` — the same file the desktop app writes.

## Development


Build by @lutzer and Claude Code in 2026
