# Remove Clip Caching Implementation Plan

**Goal:** Strip the `cached` / `cached_path` fields and all caching logic from clips across the DB entity, services, CLI, and tests.

**Architecture:** Pure deletion — no new code is introduced. Each task targets one file or one logical group of changes. The schema version bump causes SeaORM to drop and recreate all tables on next startup; the library rebuilds from sidecars automatically.

**Tech Stack:** Rust, SeaORM 1, SQLite, ratatui CLI

**Spec:** `docs/plans/specs/2026-05-27-remove-clip-caching-design.md`

---

### Task 1: Drop caching fields from the DB entity

**Files:**
- Modify: `libs/musicum-core/src/db/entities/clip.rs:14-15`

**Step 1.1 — Remove the two fields**

Open `libs/musicum-core/src/db/entities/clip.rs`. Delete lines 14–15:

```rust
    pub cached: String,
    pub cached_path: Option<String>,
```

The struct should now look like:

```rust
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "clip")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub file_id: String,
    pub title: String,
    pub processors: String,
    pub duration: Option<f64>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}
```

**Step 1.2 — Verify it compiles (it won't yet — that's fine)**

```
cargo check -p musicum-core 2>&1 | head -40
```

Expected: errors referencing `cached` and `cached_path` in service files — that's the list of places we'll fix in the next tasks.

---

### Task 2: Bump the schema version

**Files:**
- Modify: `libs/musicum-core/src/db/schema.rs:1`

**Step 2.1 — Increment SCHEMA_VERSION**

Change:
```rust
pub const SCHEMA_VERSION: u32 = 1;
```
To:
```rust
pub const SCHEMA_VERSION: u32 = 2;
```

This causes SeaORM's `create_table_from_entity()` startup logic to drop and recreate all tables, picking up the removed columns. No migration system is used — this is the project's intended approach.

---

### Task 3: Remove caching from `clip_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/clip_service.rs`

There are four locations in this file.

**Step 3.1 — `create_clip` ActiveModel (around line 67)**

Remove `cached` and `cached_path` from the `clip::ActiveModel` struct literal. Before:

```rust
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
```

After:

```rust
    let model = clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set(clip_slug),
        file_id: Set(file.id),
        title: Set(title.to_string()),
        processors: Set("[]".to_string()),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now.clone()),
        updated_at: Set(now),
    }
```

**Step 3.2 — `update_clip_processors` ActiveModel (around line 106)**

Remove `cached` and `cached_path` lines. Before:

```rust
    clip::ActiveModel {
        id:          Set(clip.id),
        slug:        Set(clip.slug),
        file_id:     Set(clip.file_id),
        title:       Set(clip.title),
        processors:  Set(processors_json),
        cached:      Set(clip.cached),
        cached_path: Set(clip.cached_path),
        duration:    Set(clip.duration),
        notes:       Set(clip.notes),
        created_at:  Set(clip.created_at),
        updated_at:  Set(now),
    }
```

After:

```rust
    clip::ActiveModel {
        id:         Set(clip.id),
        slug:       Set(clip.slug),
        file_id:    Set(clip.file_id),
        title:      Set(clip.title),
        processors: Set(processors_json),
        duration:   Set(clip.duration),
        notes:      Set(clip.notes),
        created_at: Set(clip.created_at),
        updated_at: Set(now),
    }
```

**Step 3.3 — `set_clip_notes` ActiveModel (around line 144)**

Same removal. Before:

```rust
    clip::ActiveModel {
        id:          Set(clip.id),
        slug:        Set(clip.slug),
        file_id:     Set(clip.file_id),
        title:       Set(clip.title),
        processors:  Set(clip.processors),
        cached:      Set(clip.cached),
        cached_path: Set(clip.cached_path),
        duration:    Set(clip.duration),
        notes:       Set(notes.to_string()),
        created_at:  Set(clip.created_at),
        updated_at:  Set(now),
    }
```

After:

```rust
    clip::ActiveModel {
        id:         Set(clip.id),
        slug:       Set(clip.slug),
        file_id:    Set(clip.file_id),
        title:      Set(clip.title),
        processors: Set(clip.processors),
        duration:   Set(clip.duration),
        notes:      Set(notes.to_string()),
        created_at: Set(clip.created_at),
        updated_at: Set(now),
    }
```

**Step 3.4 — `delete_clip`: remove cached-file deletion block (around line 171)**

Remove these lines from `delete_clip`:

```rust
    // Delete cached audio file if present
    if let Some(ref cp) = clip.cached_path {
        let _ = std::fs::remove_file(cp); // best-effort
    }
```

The function should proceed directly from fetching the clip to removing the sidecar entry.

**Step 3.5 — Inline test `setup()` helper (around line 234)**

In the `#[cfg(test)]` module's `setup()` function, remove `cached` and `cached_path` from the `clip::ActiveModel`:

Before:
```rust
        clip::ActiveModel {
            id:          Set(uuid::Uuid::new_v4().to_string()),
            slug:        Set("my-clip".to_string()),
            file_id:     Set(file_id),
            title:       Set("My Clip".to_string()),
            processors:  Set("[]".to_string()),
            cached:      Set("no_cache".to_string()),
            cached_path: Set(None),
            duration:    Set(None),
            notes:       Set(String::new()),
            created_at:  Set(now.clone()),
            updated_at:  Set(now),
        }
```

After:
```rust
        clip::ActiveModel {
            id:         Set(uuid::Uuid::new_v4().to_string()),
            slug:       Set("my-clip".to_string()),
            file_id:    Set(file_id),
            title:      Set("My Clip".to_string()),
            processors: Set("[]".to_string()),
            duration:   Set(None),
            notes:      Set(String::new()),
            created_at: Set(now.clone()),
            updated_at: Set(now),
        }
```

---

### Task 4: Remove caching from `sync_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/sync_service.rs:329-354`

**Step 4.1 — Update-existing-clip branch (around line 329)**

Remove `cached` and `cached_path`. Before:

```rust
                clip::ActiveModel {
                    id: Set(ex.id.clone()),
                    slug: Set(cs.slug.clone()),
                    file_id: Set(file_id.to_string()),
                    title: Set(cs.title.clone()),
                    processors: Set(processors_json),
                    cached: Set(ex.cached.clone()),
                    cached_path: Set(ex.cached_path.clone()),
                    duration: Set(ex.duration),
                    notes: Set(cs.notes.clone()),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
```

After:

```rust
                clip::ActiveModel {
                    id:         Set(ex.id.clone()),
                    slug:       Set(cs.slug.clone()),
                    file_id:    Set(file_id.to_string()),
                    title:      Set(cs.title.clone()),
                    processors: Set(processors_json),
                    duration:   Set(ex.duration),
                    notes:      Set(cs.notes.clone()),
                    created_at: Set(ex.created_at.clone()),
                    updated_at: Set(now),
                }
```

**Step 4.2 — Insert-new-clip branch (around line 347)**

Remove `cached` and `cached_path`. Before:

```rust
            clip::ActiveModel {
                id: Set(Uuid::new_v4().to_string()),
                slug: Set(cs.slug.clone()),
                file_id: Set(file_id.to_string()),
                title: Set(cs.title.clone()),
                processors: Set(processors_json),
                cached: Set("no_cache".into()),
                cached_path: Set(None),
                duration: Set(None),
                notes: Set(cs.notes.clone()),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
```

After:

```rust
            clip::ActiveModel {
                id:         Set(Uuid::new_v4().to_string()),
                slug:       Set(cs.slug.clone()),
                file_id:    Set(file_id.to_string()),
                title:      Set(cs.title.clone()),
                processors: Set(processors_json),
                duration:   Set(None),
                notes:      Set(cs.notes.clone()),
                created_at: Set(now.clone()),
                updated_at: Set(now),
            }
```

---

### Task 5: Remove cached-file deletion from `file_service.rs`

**Files:**
- Modify: `libs/musicum-core/src/services/file_service.rs:115-127`

**Step 5.1 — Remove cache-cleanup block**

The `delete_file` function currently:
1. Queries all clips for the file (needed for `clip_count`)
2. Loops over clips to delete `cached_path` files from disk ← **remove this**
3. Deletes the sidecar, then cascades DB rows

Remove the cache-cleanup comment and loop. Before:

```rust
    // Collect clip cached paths before cascade deletes them
    let clips = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file.id))
        .all(db)
        .await?;
    let clip_count = clips.len();

    // Delete cached clip audio files from disk
    for c in &clips {
        if let Some(ref cp) = c.cached_path {
            let _ = std::fs::remove_file(cp); // best-effort
        }
    }
```

After:

```rust
    let clip_count = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file.id))
        .count(db)
        .await? as usize;
```

> **Note:** We switch from `.all()` to `.count()` since we no longer need the full clip rows — we only need the count. Check if `clip::Entity::find().filter(...).count(db)` is the SeaORM 1 API; it is — `PaginatorTrait::count` is available via `use sea_orm::PaginatorTrait`. Add that import if it's not already present at the top of `file_service.rs`.

**Step 5.2 — Check the import block**

At the top of `file_service.rs`, verify that `PaginatorTrait` is imported (or add it):

```rust
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
```

---

### Task 6: Remove cache display from `clips.rs` CLI

**Files:**
- Modify: `apps/cli/src/commands/clips.rs:75-96,114-115`

**Step 6.1 — List-by-file header and row format (line 75–76)**

Before:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "TITLE  [CACHED]"],
                        clips.iter().map(|c| vec![c.slug.clone(), format!("{}  [{}]", c.title, c.cached)]).collect(),
                    );
```

After:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "TITLE"],
                        clips.iter().map(|c| vec![c.slug.clone(), c.title.clone()]).collect(),
                    );
```

**Step 6.2 — List-all header and row format (lines 91–96)**

Before:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "FILE  TITLE  [CACHED]"],
                        clips.iter().map(|c| {
                            let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
                            vec![c.slug.clone(), format!("{}  {}  [{}]", file_slug, c.title, c.cached)]
                        }).collect(),
                    );
```

After:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "FILE  TITLE"],
                        clips.iter().map(|c| {
                            let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
                            vec![c.slug.clone(), format!("{}  {}", file_slug, c.title)]
                        }).collect(),
                    );
```

**Step 6.3 — Detail view fields (lines 114–115)**

Remove these two `Field` lines from the `print_detail` call:
```rust
                    Field("cached", clip.cached.clone()),
                    Field("cached_path", clip.cached_path.clone().unwrap_or_else(|| "-".into())),
```

---

### Task 7: Remove cache display from `files.rs` CLI

**Files:**
- Modify: `apps/cli/src/commands/files.rs` (around line 104–111)

**Step 7.1 — Clips-under-file table header and row format**

Before:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "TITLE  [CACHED]"],
                        clips
                            .iter()
                            .map(|c| vec![c.slug.clone(), format!("{}  [{}]", c.title, c.cached)])
                            .collect(),
                    );
```

After:
```rust
                    print_table(
                        "clips",
                        &["SLUG", "TITLE"],
                        clips
                            .iter()
                            .map(|c| vec![c.slug.clone(), c.title.clone()])
                            .collect(),
                    );
```

---

### Task 8: Remove stale assertion from external test

**Files:**
- Modify: `libs/musicum-core/tests/clip_service.rs:28`

**Step 8.1 — Remove `cached` assertion**

In `create_clip_adds_to_db_and_sidecar`, remove:

```rust
    assert_eq!(clip.cached, "no_cache");
```

The surrounding assertions (`slug`, `title`) remain unchanged.

---

### Task 9: Build and test

**Step 9.1 — Check compilation**

```
cargo check --all
```

Expected: zero errors.

**Step 9.2 — Run clippy**

```
cargo clippy --all
```

Expected: zero warnings. Fix any that appear (unused imports, etc.).

**Step 9.3 — Run core tests**

```
cargo test -p musicum-core
```

Expected output (all pass):
```
test create_clip_adds_to_db_and_sidecar ... ok
test create_clip_file_not_found ... ok
test create_clip_slug_collision ... ok
test set_clip_notes_updates_db_and_sidecar ... ok
test delete_clip_removes_db_and_sidecar_entry ... ok
... (other tests)
```

**Step 9.4 — Full build**

```
cargo build
```

Expected: success, no warnings.

**Step 9.5 — Smoke test via CLI**

```
cargo run -p musicum-cli -- clips list
cargo run -p musicum-cli -- clips show <some-slug>
```

Verify no `[cached]` badge in list output, and no `cached` / `cached_path` fields in the detail view.
