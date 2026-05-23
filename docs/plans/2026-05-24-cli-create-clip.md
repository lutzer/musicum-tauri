# CLI Create Clip Implementation Plan

**Goal:** Add `musicum clips create <file-slug> <title>` to create a clip entry in a file's sidecar and DB in one step.

**Architecture:** `create_clip` lives in `clip_service` — it looks up the file path from the DB, mutates the `.musicum.json` sidecar on disk, then inserts the clip row into the DB. The CLI handler is thin. No `library_dir` threading needed — the file's absolute path is already stored in the DB and is sufficient to locate the sidecar.

**Tech Stack:** Rust, SeaORM + SQLite, `slug` crate (already in `musicum-core`), `sidecar::*` helpers already in `musicum-core`.

---

## File Map

| File | Change |
|---|---|
| `libs/musicum-core/src/services/clip_service.rs` | Add `create_clip` function |
| `libs/musicum-core/tests/clip_service.rs` | New file — integration tests for `create_clip` |
| `apps/cli/src/commands/clips.rs` | Add `Create` subcommand variant + handler |

`main.rs` — **no changes needed.** `create_clip` derives the sidecar path from the file path already stored in the DB.

---

### Task 1: Write failing tests for `create_clip`

**Files:**
- Create: `libs/musicum-core/tests/clip_service.rs`

Create the test file. It follows the same pattern as `tests/sync_service.rs` — use `tempdir()`, write a WAV via `common::write_sine_wav`, connect a DB, sync once to populate the DB, then call the function under test.

```rust
mod common;

use musicum_core::{db, sidecar, services::{clip_service, sync_service}};
use musicum_core::db::entities::clip;
use musicum_core::ServiceError;
use sea_orm::EntityTrait;
use tempfile::tempdir;

async fn setup_with_file(lib_path: &std::path::Path, filename: &str) -> sea_orm::DatabaseConnection {
    let wav = lib_path.join(filename);
    common::write_sine_wav(&wav, 0.5);
    let db = db::connect(lib_path.to_str().unwrap()).await.unwrap();
    sync_service::sync_library(&db, lib_path.to_str().unwrap()).await.unwrap();
    db
}

#[tokio::test]
async fn create_clip_adds_to_db_and_sidecar() {
    let dir = tempdir().unwrap();
    let db = setup_with_file(dir.path(), "kick.wav").await;
    let wav = dir.path().join("kick.wav");

    let clip = clip_service::create_clip(&db, "kick", "My Clip").await.unwrap();

    assert_eq!(clip.slug, "my-clip");
    assert_eq!(clip.title, "My Clip");
    assert_eq!(clip.cached, "no_cache");

    // Clip is in DB
    let all = clip::Entity::find().all(&db).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].slug, "my-clip");

    // Clip is in sidecar
    let sc = sidecar::read_file_sidecar(&wav).unwrap();
    assert_eq!(sc.clips.len(), 1);
    assert_eq!(sc.clips[0].slug, "my-clip");
    assert_eq!(sc.clips[0].title, "My Clip");
    assert!(sc.clips[0].processors.is_empty());
}

#[tokio::test]
async fn create_clip_file_not_found() {
    let dir = tempdir().unwrap();
    let db = db::connect(dir.path().to_str().unwrap()).await.unwrap();

    let err = clip_service::create_clip(&db, "nonexistent", "Foo").await.unwrap_err();
    assert!(matches!(err, ServiceError::NotFound(_)));
}

#[tokio::test]
async fn create_clip_slug_collision() {
    let dir = tempdir().unwrap();
    let wav = dir.path().join("pad.wav");
    common::write_sine_wav(&wav, 0.5);

    // Pre-write sidecar with a clip whose slug would collide
    let sc = sidecar::FileSidecar {
        version: 1,
        metadata: Default::default(),
        attachments: vec![],
        clips: vec![sidecar::ClipSidecar {
            slug: "my-clip".into(),
            title: "My Clip".into(),
            notes: String::new(),
            processors: vec![],
        }],
    };
    sidecar::write_file_sidecar(&wav, &sc).unwrap();

    let db = db::connect(dir.path().to_str().unwrap()).await.unwrap();
    sync_service::sync_library(&db, dir.path().to_str().unwrap()).await.unwrap();

    let err = clip_service::create_clip(&db, "pad", "My Clip").await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidInput(_)));
}
```

Run the tests — they should **fail to compile** because `create_clip` doesn't exist yet:

```
cargo test -p musicum-core --test clip_service
```

Expected: compilation error mentioning `create_clip` not found.

---

### Task 2: Implement `create_clip` in `clip_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

Replace the current import line and add the new function. The full file after the change:

```rust
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use slug::slugify;
use uuid::Uuid;
use std::path::Path;

use crate::db::entities::clip;
use crate::sidecar::{self, ClipSidecar};
use crate::services::file_service;
use crate::ServiceError;

pub async fn list_clips_for_file(
    db: &DatabaseConnection,
    file_id: &str,
) -> Result<Vec<clip::Model>, ServiceError> {
    Ok(clip::Entity::find()
        .filter(clip::Column::FileId.eq(file_id))
        .order_by_asc(clip::Column::Title)
        .all(db)
        .await?)
}

pub async fn get_clip_by_slug(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<clip::Model, ServiceError> {
    clip::Entity::find()
        .filter(clip::Column::Slug.eq(slug))
        .one(db)
        .await?
        .ok_or_else(|| ServiceError::NotFound(format!("clip '{slug}'")))
}

pub async fn create_clip(
    db: &DatabaseConnection,
    file_slug: &str,
    title: &str,
) -> Result<clip::Model, ServiceError> {
    let file = file_service::get_file_by_slug(db, file_slug).await?;
    let audio_path = Path::new(&file.path);
    let mut sc = sidecar::read_file_sidecar(audio_path)?;

    let clip_slug = slugify(title);

    if sc.clips.iter().any(|c| c.slug == clip_slug) {
        return Err(ServiceError::InvalidInput(format!(
            "clip with slug '{clip_slug}' already exists for this file"
        )));
    }

    sc.clips.push(ClipSidecar {
        slug: clip_slug.clone(),
        title: title.to_string(),
        notes: String::new(),
        processors: vec![],
    });

    sidecar::write_file_sidecar(audio_path, &sc)?;

    let now = chrono::Utc::now().to_rfc3339();
    let model = clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set(clip_slug),
        file_id: Set(file.id),
        title: Set(title.to_string()),
        processors: Set("[]".to_string()),
        cached: Set("no_cache".to_string()),
        cached_path: Set(None),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    Ok(model)
}
```

---

### Task 3: Run tests — confirm they pass

```
cargo test -p musicum-core --test clip_service
```

Expected output (all three tests pass):

```
test create_clip_adds_to_db_and_sidecar ... ok
test create_clip_file_not_found ... ok
test create_clip_slug_collision ... ok
```

If any test fails, fix `create_clip` before continuing. Do not move on with failing tests.

---

### Task 4: Add `Create` subcommand to the CLI

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

Add `Create` to `ClipsCommand` and handle it in `run`. Replace the full file:

```rust
use anyhow::Result;
use clap::{Args, Subcommand};
use musicum_core::services::{clip_service, file_service};
use sea_orm::DatabaseConnection;

use crate::output::{print_detail, print_json, print_table};

#[derive(Debug, Args)]
pub struct ClipsArgs {
    #[command(subcommand)]
    pub command: ClipsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClipsCommand {
    /// List clips for a file
    List {
        file_slug: String,
        #[arg(long)]
        json: bool,
    },
    /// Show detail for one clip
    Show {
        slug: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a new clip for a file
    Create {
        file_slug: String,
        title: String,
    },
}

pub async fn run(db: &DatabaseConnection, args: ClipsArgs) -> Result<()> {
    match args.command {
        ClipsCommand::List { file_slug, json } => {
            let file = file_service::get_file_by_slug(db, &file_slug).await?;
            let clips = clip_service::list_clips_for_file(db, &file.id).await?;

            if json {
                print_json(&clips);
            } else if clips.is_empty() {
                println!("No clips for '{}'. Sync or create clips via sidecar.", file_slug);
            } else {
                print_table(
                    ("SLUG", "TITLE  [CACHED]"),
                    clips
                        .iter()
                        .map(|c| {
                            (c.slug.clone(), format!("{}  [{}]", c.title, c.cached))
                        })
                        .collect(),
                );
            }
        }
        ClipsCommand::Show { slug, json } => {
            let clip = clip_service::get_clip_by_slug(db, &slug).await?;

            if json {
                print_json(&clip);
            } else {
                let processors: serde_json::Value =
                    serde_json::from_str(&clip.processors).unwrap_or(serde_json::json!([]));
                print_detail(vec![
                    ("slug", clip.slug.clone()),
                    ("title", clip.title.clone()),
                    ("file_id", clip.file_id.clone()),
                    ("cached", clip.cached.clone()),
                    (
                        "cached_path",
                        clip.cached_path.clone().unwrap_or_else(|| "-".into()),
                    ),
                    (
                        "duration",
                        clip.duration
                            .map_or("-".into(), |d| format!("{d:.3}s")),
                    ),
                    ("processors", serde_json::to_string_pretty(&processors).unwrap()),
                    ("notes", if clip.notes.is_empty() { "-".into() } else { clip.notes.clone() }),
                ]);
            }
        }
        ClipsCommand::Create { file_slug, title } => {
            let clip = clip_service::create_clip(db, &file_slug, &title).await?;
            println!("Created clip '{}' for file '{}'", clip.slug, file_slug);
        }
    }
    Ok(())
}
```

---

### Task 5: Build and smoke-test the CLI

Build:

```
cargo build -p musicum-cli 2>&1 | head -30
```

Expected: no errors.

Then smoke-test with your actual library (adjust slug to a file you have synced):

```
# Confirm a file exists first
musicum files list

# Create a clip on it — replace <file-slug> with a real slug from the list
musicum clips create <file-slug> "Test Clip"

# Confirm it appears
musicum clips list <file-slug>

# Confirm sidecar was updated
cat /path/to/your/audio/file.wav.musicum.json | grep -A5 clips
```

Expected output of the create command:
```
Created clip 'test-clip' for <file-slug>
```

---

### Task 6: Run the full core test suite

Confirm no regressions:

```
cargo test -p musicum-core
```

Expected: all tests pass.
