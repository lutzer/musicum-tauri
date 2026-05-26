# CLAUDE.md

## Project overview
Musicum Tauri is a desktop app for audio file management: browsing, clipping,
applying audio plugin chains, collecting, and caching processed audio. Currently
CLI-only (ratatui TUI); Tauri 2 desktop shell and SvelteKit frontend are planned.

## Repo layout
```
apps/cli/             # CLI/TUI entry point (ratatui)
libs/musicum-core/    # All business logic: DB, services, audio engine
libs/audio-plugin-sdk/
libs/audio-plugins/           # gain, reverb, pan, normalize, oscilloscope, level-meter
libs/structural-processor-sdk/
libs/structural-processors/
```

## Tech stack
- Rust (CLI, all business logic)
- ratatui 0.29 + crossterm (TUI)
- SeaORM 1 + SQLite (no migration system — see Gotchas)
- cpal 0.17 (audio output), symphonia 0.5 (decoding), rtrb 0.3 (lock-free ring buffer)
- ffmpeg (system binary, MP3 encoding only)

## Dev commands
- `cargo run -p musicum-cli -- <args>` — run the CLI
- `cargo test -p musicum-core` — run core library tests
- `cargo clippy --all` — lint (run after every change)
- `cargo build` — build all crates

## Gotchas & known issues
- **No DB migrations.** SeaORM uses `create_table_from_entity()` on startup.
  Breaking schema changes require bumping `SCHEMA_VERSION` in `libs/musicum-core/src/db/schema.rs`,
  which drops and recreates all tables. Never add a migration system without discussion.
- **Sidecars are source of truth.** The SQLite DB is a queryable index rebuilt from
  `.musicum.json` sidecars. Don't treat the DB as canonical. Propagate sidecar changes
  to the in-memory library state immediately; full sync only for new/removed files.
- **ffmpeg is a system dependency.** Required only for clip caching (`cache_clip`).
- **Audio plugin crates need dual crate-type.** Each plugin/processor `Cargo.toml` must
  have `crate-type = ["cdylib", "rlib"]` — `cdylib` for WASM, `rlib` for native linkage.
- **Audio Engine applys processors and plugins** to the source audio files. the processors and plugins (Edits) are defined in the clips. Plugin parameters can be adjusted while the clip is playing, so make sure to design the audio engine taking this into account. A second audio pipeline is present for caching clips in a background operation.
- **Logic goes in musicum-core.** CLI is display-only; all business logic lives in the core lib.
- **CLI output style.** Reuse output functions so all commands share consistent formatting.

## Supplemental docs
- `docs/plans/specs/2026-05-22-tauri-greenfield-setup.md` — full architecture & DB schema
- `docs/plans/` — per-feature design docs (CLI, player, processor, output)
