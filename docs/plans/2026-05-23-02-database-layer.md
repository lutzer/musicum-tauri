# Database Layer Implementation Plan

**Goal:** Implement the full SeaORM entity set, a `connect()` function with WAL mode and schema-version-aware table creation, and integration tests that verify every entity can be inserted and queried.

**Architecture:** Seven SeaORM entities map to seven SQLite tables. A `_musicum_meta` hidden table stores the current schema version. On connect, musicum-core reads the stored version; if it doesn't match `SCHEMA_VERSION`, all tables are dropped and recreated. All DB logic lives under `libs/musicum-core/src/db/`.

**Tech Stack:** SeaORM 1 with `sqlx-sqlite` + `runtime-tokio-rustls`, SQLite WAL mode, Tokio async runtime for tests.

**Prerequisite:** Plan 01 complete — workspace compiles and `musicum-core` skeleton is in place.

---

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `libs/musicum-core/src/db/entities/file.rs` | `file` table entity |
| Create | `libs/musicum-core/src/db/entities/file_metadata.rs` | `file_metadata` table entity |
| Create | `libs/musicum-core/src/db/entities/file_attachment.rs` | `file_attachment` table entity |
| Create | `libs/musicum-core/src/db/entities/clip.rs` | `clip` table entity |
| Create | `libs/musicum-core/src/db/entities/collection.rs` | `collection` table entity |
| Create | `libs/musicum-core/src/db/entities/collection_clip.rs` | `collection_clip` join table entity |
| Create | `libs/musicum-core/src/db/entities/preset.rs` | `preset` table entity |
| Modify | `libs/musicum-core/src/db/entities/mod.rs` | Declare all entity modules |
| Modify | `libs/musicum-core/src/db/mod.rs` | `connect()`, `run_create_all()` |
| Create | `libs/musicum-core/tests/db_schema.rs` | Integration tests |

---

### Task 1: Implement the `file` entity

**Step 1.1** — Create `libs/musicum-core/src/db/entities/file.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub name: String,
    pub path: String,
    pub duration: f64,
    pub sample_rate: i32,
    pub channels: i32,
    pub mime_type: String,
    pub hash: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_one = "super::file_metadata::Entity")]
    FileMetadata,
    #[sea_orm(has_many = "super::file_attachment::Entity")]
    FileAttachment,
    #[sea_orm(has_many = "super::clip::Entity")]
    Clip,
}

impl ActiveModelBehavior for ActiveModel {}
```

---

### Task 2: Implement the `file_metadata` entity

**Step 2.1** — Create `libs/musicum-core/src/db/entities/file_metadata.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_metadata")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub file_id: String,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub rating: Option<i32>,
    pub color: Option<String>,
    pub notes: String,
    pub tags: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileId",
        to = "super::file::Column::Id"
    )]
    File,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

---

### Task 3: Implement the `file_attachment` entity

**Step 3.1** — Create `libs/musicum-core/src/db/entities/file_attachment.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_attachment")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub file_id: String,
    #[sea_orm(column_name = "type")]
    pub attachment_type: String,
    pub text: Option<String>,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileId",
        to = "super::file::Column::Id"
    )]
    File,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

Note: SQLite column name `type` is a reserved word in some contexts, so the Rust field is named `attachment_type` but stored as `type` via `#[sea_orm(column_name = "type")]`.

---

### Task 4: Implement the `clip` entity

**Step 4.1** — Create `libs/musicum-core/src/db/entities/clip.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "clip")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub file_id: String,
    pub title: String,
    pub processors: String,
    pub cached: String,
    pub cached_path: Option<String>,
    pub duration: Option<f64>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::FileId",
        to = "super::file::Column::Id"
    )]
    File,
    #[sea_orm(has_many = "super::collection_clip::Entity")]
    CollectionClip,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

The `processors` column holds JSON (same format as in the sidecar). It is stored as `TEXT` and parsed at the service layer.

The `cached` column is a string enum: `"no_cache"`, `"caching"`, `"ready"`, `"error"`.

---

### Task 5: Implement `collection`, `collection_clip`, and `preset` entities

**Step 5.1** — Create `libs/musicum-core/src/db/entities/collection.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "collection")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub title: String,
    pub description: String,
    pub background_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::collection_clip::Entity")]
    CollectionClip,
}

impl ActiveModelBehavior for ActiveModel {}
```

**Step 5.2** — Create `libs/musicum-core/src/db/entities/collection_clip.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_clip")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub collection_id: String,
    pub clip_id: String,
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::collection::Entity",
        from = "Column::CollectionId",
        to = "super::collection::Column::Id"
    )]
    Collection,
    #[sea_orm(
        belongs_to = "super::clip::Entity",
        from = "Column::ClipId",
        to = "super::clip::Column::Id"
    )]
    Clip,
}

impl Related<super::collection::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Collection.def()
    }
}

impl Related<super::clip::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Clip.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

Note: The spec requires a unique constraint on `(collection_id, clip_id)`. SeaORM's `create_table_from_entity` does not handle compound unique constraints automatically — we'll enforce this at the service layer by checking for existence before insert, and add the constraint via raw SQL in `run_create_all` (see Task 7).

**Step 5.3** — Create `libs/musicum-core/src/db/entities/preset.rs`:
```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "preset")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    #[sea_orm(unique)]
    pub slug: String,
    pub title: String,
    pub description: String,
    pub processors: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

---

### Task 6: Update entities/mod.rs

**Step 6.1** — Replace `libs/musicum-core/src/db/entities/mod.rs` with:
```rust
pub mod clip;
pub mod collection;
pub mod collection_clip;
pub mod file;
pub mod file_attachment;
pub mod file_metadata;
pub mod preset;
```

---

### Task 7: Implement db/mod.rs with connect() and run_create_all()

**Step 7.1** — Replace `libs/musicum-core/src/db/mod.rs` with:
```rust
pub mod entities;
pub mod schema;

use sea_orm::{
    ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr, Schema, Statement,
};

use crate::ServiceError;
use schema::SCHEMA_VERSION;

pub async fn connect(library_dir: &str) -> Result<DatabaseConnection, ServiceError> {
    let db_path = format!("{library_dir}/.musicum/musicum.db");

    // Ensure the .musicum directory exists
    let dir = std::path::Path::new(&db_path).parent().unwrap();
    std::fs::create_dir_all(dir)?;

    let url = format!("sqlite://{db_path}?mode=rwc");
    let db = Database::connect(&url).await?;

    // WAL mode for concurrent access (CLI + desktop app can coexist)
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA journal_mode=WAL".to_owned(),
    ))
    .await?;

    ensure_schema(&db).await?;
    Ok(db)
}

async fn ensure_schema(db: &DatabaseConnection) -> Result<(), DbErr> {
    // Meta table to persist schema version
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS _musicum_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)"
            .to_owned(),
    ))
    .await?;

    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT value FROM _musicum_meta WHERE key = 'schema_version'".to_owned(),
        ))
        .await?;

    let stored: u32 = row
        .and_then(|r| r.try_get::<String>("", "value").ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if stored != SCHEMA_VERSION {
        drop_all_tables(db).await?;
    }

    create_all_tables(db).await?;

    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!(
            "INSERT OR REPLACE INTO _musicum_meta VALUES ('schema_version', '{SCHEMA_VERSION}')"
        ),
    ))
    .await?;

    Ok(())
}

async fn drop_all_tables(db: &DatabaseConnection) -> Result<(), DbErr> {
    let tables = [
        "collection_clip",
        "clip",
        "collection",
        "preset",
        "file_attachment",
        "file_metadata",
        "file",
    ];
    for table in &tables {
        db.execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("DROP TABLE IF EXISTS \"{table}\""),
        ))
        .await?;
    }
    Ok(())
}

async fn create_all_tables(db: &DatabaseConnection) -> Result<(), DbErr> {
    use entities::*;

    let builder = db.get_database_backend();
    let schema = Schema::new(builder);

    macro_rules! create {
        ($entity:expr) => {
            db.execute(
                builder.build(schema.create_table_from_entity($entity).if_not_exists()),
            )
            .await?;
        };
    }

    create!(file::Entity);
    create!(file_metadata::Entity);
    create!(file_attachment::Entity);
    create!(clip::Entity);
    create!(collection::Entity);
    create!(collection_clip::Entity);
    create!(preset::Entity);

    // compound unique constraint not expressible via SeaORM derive
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_collection_clip \
         ON collection_clip (collection_id, clip_id)"
            .to_owned(),
    ))
    .await?;

    Ok(())
}
```

---

### Task 8: Write integration tests

**Step 8.1** — Create `libs/musicum-core/tests/db_schema.rs`:
```rust
use musicum_core::db;
use musicum_core::db::entities::{clip, collection, collection_clip, file, file_metadata, preset};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
use tempfile::tempdir;
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

async fn open_db() -> (sea_orm::DatabaseConnection, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = db::connect(dir.path().to_str().unwrap()).await.unwrap();
    (db, dir)
}

#[tokio::test]
async fn connect_creates_db_file() {
    let (_, dir) = open_db().await;
    let db_path = dir.path().join(".musicum").join("musicum.db");
    assert!(db_path.exists(), "musicum.db was not created");
}

#[tokio::test]
async fn schema_version_is_stored() {
    let (db, _dir) = open_db().await;
    let row = db
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT value FROM _musicum_meta WHERE key = 'schema_version'".to_owned(),
        ))
        .await
        .unwrap()
        .unwrap();
    let version: String = row.try_get("", "value").unwrap();
    assert_eq!(version, musicum_core::db::schema::SCHEMA_VERSION.to_string());
}

#[tokio::test]
async fn schema_reset_on_version_bump() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();

    // First connection — creates tables at current SCHEMA_VERSION
    let db = db::connect(path).await.unwrap();

    // Insert a file row
    file::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        slug: Set("test-file".into()),
        name: Set("test".into()),
        path: Set("/tmp/test.wav".into()),
        duration: Set(1.0),
        sample_rate: Set(44100),
        channels: Set(2),
        mime_type: Set("audio/wav".into()),
        hash: Set("abc123".into()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    assert_eq!(file::Entity::find().count(&db).await.unwrap(), 1);

    // Manually corrupt the stored version so the next connect() triggers a reset
    db.execute(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Sqlite,
        "UPDATE _musicum_meta SET value = '999' WHERE key = 'schema_version'".to_owned(),
    ))
    .await
    .unwrap();
    drop(db);

    // Second connection — sees version mismatch → drops + recreates tables
    let db2 = db::connect(path).await.unwrap();
    assert_eq!(
        file::Entity::find().count(&db2).await.unwrap(),
        0,
        "tables should be empty after schema reset"
    );
}

#[tokio::test]
async fn insert_and_query_file_with_metadata() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("drums-kick".into()),
        name: Set("Kick Drum".into()),
        path: Set("/music/kick.wav".into()),
        duration: Set(0.5),
        sample_rate: Set(44100),
        channels: Set(1),
        mime_type: Set("audio/wav".into()),
        hash: Set("deadbeef".into()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    file_metadata::ActiveModel {
        file_id: Set(file_id.clone()),
        bpm: Set(Some(128.0)),
        key: Set(Some("Am".into())),
        rating: Set(Some(4)),
        color: Set(None),
        notes: Set("punchy kick".into()),
        tags: Set("drums,percussion".into()),
    }
    .insert(&db)
    .await
    .unwrap();

    let f = file::Entity::find_by_id(&file_id)
        .one(&db)
        .await
        .unwrap()
        .expect("file should exist");
    assert_eq!(f.slug, "drums-kick");
    assert_eq!(f.sample_rate, 44100);

    let m = file_metadata::Entity::find_by_id(&file_id)
        .one(&db)
        .await
        .unwrap()
        .expect("metadata should exist");
    assert_eq!(m.bpm, Some(128.0));
    assert_eq!(m.tags, "drums,percussion");
}

#[tokio::test]
async fn insert_clip_and_query_by_file() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();
    let clip_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("synth-pad".into()),
        name: Set("Pad".into()),
        path: Set("/music/pad.wav".into()),
        duration: Set(4.0),
        sample_rate: Set(48000),
        channels: Set(2),
        mime_type: Set("audio/wav".into()),
        hash: Set("cafebabe".into()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    clip::ActiveModel {
        id: Set(clip_id.clone()),
        slug: Set("pad-reverb".into()),
        file_id: Set(file_id.clone()),
        title: Set("With Reverb".into()),
        processors: Set(r#"[{"type":"plugin","id":"reverb","enabled":true,"params":{}}]"#.into()),
        cached: Set("no_cache".into()),
        cached_path: Set(None),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    let clips = clip::Entity::find()
        .filter(clip::Column::FileId.eq(&file_id))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].slug, "pad-reverb");
}

#[tokio::test]
async fn collection_clip_unique_constraint() {
    let (db, _dir) = open_db().await;
    let file_id = Uuid::new_v4().to_string();
    let clip_id = Uuid::new_v4().to_string();
    let col_id = Uuid::new_v4().to_string();

    file::ActiveModel {
        id: Set(file_id.clone()),
        slug: Set("my-file".into()),
        name: Set("File".into()),
        path: Set("/tmp/f.wav".into()),
        duration: Set(1.0),
        sample_rate: Set(44100),
        channels: Set(1),
        mime_type: Set("audio/wav".into()),
        hash: Set("11".into()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    clip::ActiveModel {
        id: Set(clip_id.clone()),
        slug: Set("my-clip".into()),
        file_id: Set(file_id.clone()),
        title: Set("Clip".into()),
        processors: Set("[]".into()),
        cached: Set("no_cache".into()),
        cached_path: Set(None),
        duration: Set(None),
        notes: Set(String::new()),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    collection::ActiveModel {
        id: Set(col_id.clone()),
        slug: Set("my-col".into()),
        title: Set("Col".into()),
        description: Set(String::new()),
        background_path: Set(None),
        created_at: Set(now()),
        updated_at: Set(now()),
    }
    .insert(&db)
    .await
    .unwrap();

    collection_clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        collection_id: Set(col_id.clone()),
        clip_id: Set(clip_id.clone()),
        position: Set(0),
    }
    .insert(&db)
    .await
    .unwrap();

    // Inserting the same (collection_id, clip_id) again must fail
    let result = collection_clip::ActiveModel {
        id: Set(Uuid::new_v4().to_string()),
        collection_id: Set(col_id.clone()),
        clip_id: Set(clip_id.clone()),
        position: Set(1),
    }
    .insert(&db)
    .await;

    assert!(result.is_err(), "duplicate collection_clip should be rejected");
}
```

**Step 8.2** — Run the tests:
```bash
cargo test -p musicum-core --test db_schema
# Expected: 5 tests pass, 0 fail
```

If `connect_creates_db_file` fails with "file not found", check that `ensure_schema` runs `create_dir_all` before the SQLite URL is opened.

If `schema_reset_on_version_bump` fails, check the `drop_all_tables` order: child tables with FK references must be dropped before parent tables.

---

## What's next

Plan 03 builds the sidecar types and sync service on top of the DB layer.
