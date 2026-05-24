# CLAUDE.md

## Project overview
Musicum Tauri is a Tauri 2 desktop app for audio file management: browsing, clipping,
applying audio plugin chains, collecting, and caching processed audio. The repo is a
greenfield rewrite; only the plugin/processor Rust crates are carried over from the old repo.

## Repo layout
```
apps/desktop/src-tauri/   # Tauri shell (Rust): commands, state, optional Axum HTTP adapter
apps/frontend/            # SvelteKit 5 UI
libs/musicum-core/        # All business logic: DB, services, audio engine
libs/audio-plugin-sdk/    # Rust trait crate (copied from old repo)
libs/audio-plugins/       # gain, reverb, pan, normalize, oscilloscope, level-meter
libs/structural-processor-sdk/
libs/structural-processors/
```

## Tech stack
- Rust + Tauri 2 (desktop shell, IPC, events)
- SvelteKit 5 + TypeScript (frontend, `apps/frontend/`)
- SeaORM 1 + SQLite (no migration system — see Gotchas)
- cpal 0.17 (audio output), symphonia 0.5 (decoding), rtrb 0.3 (lock-free ring buffer)
- ffmpeg (system binary, MP3 encoding only)

## Dev commands
- `cargo tauri dev` — start desktop app (spawns frontend dev server automatically)
- `cd apps/frontend && npm run dev` — frontend dev server alone
- `cargo test -p musicum-core` — run core library tests
- `cargo tauri build` — production build → `target/release/bundle/`

## Gotchas & known issues
- **No DB migrations.** SeaORM uses `create_table_from_entity()` on every startup.
  Breaking schema changes require bumping `SCHEMA_VERSION` in `libs/musicum-core/src/db/schema.rs`,
  which drops and recreates all tables in dev. Never add a migration system without discussion.
- **Sidecars are source of truth.** The SQLite DB is a queryable index rebuilt from
  `.musicum.json` sidecars via `sync_library`. Don't treat the DB as canonical. Whenever adding changes to a sidecar, always propagate them to the library if possible. syncing should be only if new files are copied in or old ones are removed
- **ffmpeg is a system dependency.** Must be installed on the host for clip caching
  (`cache_clip` command). Pure-Rust audio I/O everywhere else.
- **Audio plugin crates need dual crate-type.** Each plugin/processor `Cargo.toml` must
  have `crate-type = ["cdylib", "rlib"]` — `cdylib` for WASM, `rlib` for native linkage
  in `musicum-core`. Adding a new plugin crate without this will break the native build.
- **Logic goes in musicum-core** try to keep all the logic and models in the core library. cli and frontend just display, the tauri interafce just wraps the musicum-core library

## Supplemental docs
Full architecture detail, DB schema, sidecar formats, audio engine design, and
implementation order:
- `docs/plans/specs/2026-05-22-tauri-greenfield-setup.md`

## Important Guidelines
- After any code changes always lint and test