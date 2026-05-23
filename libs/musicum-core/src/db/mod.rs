pub mod entities;
pub mod schema;

use sea_orm::{
    ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr, Schema, Statement,
};

use crate::ServiceError;
use schema::SCHEMA_VERSION;

pub async fn connect(library_dir: &str) -> Result<DatabaseConnection, ServiceError> {
    let db_path = format!("{library_dir}/.musicum/musicum.db");

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
