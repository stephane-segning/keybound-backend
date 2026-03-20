use backend_core::{Error, Result};
use diesel::Connection;
use diesel::pg::PgConnection;
use diesel::prelude::RunQueryDsl;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");
const MIGRATION_ADVISORY_LOCK_KEY: i64 = 9_247_331_105_042_019;

#[derive(Clone, Copy, Debug)]
enum DbKind {
    Postgres,
}

/// Builder/factory for constructing database pools with migrations.
#[derive(Clone, Debug)]
pub struct DbFactory {
    url: String,
    max_connections: u32,
    kind: DbKind,
}

impl DbFactory {
    pub fn postgres(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_connections: 5,
            kind: DbKind::Postgres,
        }
    }

    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    pub async fn build_postgres(self) -> Result<Pool<AsyncPgConnection>> {
        if !matches!(self.kind, DbKind::Postgres) {
            return Err(Error::Database(
                "DbFactory::build_postgres called on non-postgres factory".to_string(),
            ));
        }

        // Run migrations synchronously
        let url = self.url.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = PgConnection::establish(&url)
                .map_err(|e| Error::Database(format!("Failed to connect for migrations: {}", e)))?;

            diesel::sql_query(format!(
                "SELECT pg_advisory_lock({MIGRATION_ADVISORY_LOCK_KEY})"
            ))
            .execute(&mut conn)
            .map_err(|e| Error::Database(format!("Failed to acquire migration lock: {}", e)))?;

            let migration_result = conn
                .run_pending_migrations(MIGRATIONS)
                .map(|_| ())
                .map_err(|e| Error::Database(format!("Migration failed: {}", e)));

            let unlock_result = diesel::sql_query(format!(
                "SELECT pg_advisory_unlock({MIGRATION_ADVISORY_LOCK_KEY})"
            ))
            .execute(&mut conn)
            .map_err(|e| Error::Database(format!("Failed to release migration lock: {}", e)));

            migration_result?;
            unlock_result?;

            Ok::<_, Error>(())
        })
        .await
        .map_err(|e| Error::Database(format!("Migration task failed: {}", e)))??;

        // Create async pool
        let config = AsyncDieselConnectionManager::<diesel_async::AsyncPgConnection>::new(self.url);
        let pool = Pool::builder(config)
            .max_size(self.max_connections as usize)
            .build()
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(pool)
    }
}

/// Connect using PgPool and run the provided migrator.
pub async fn connect_postgres_and_migrate(database_url: &str) -> Result<Pool<AsyncPgConnection>> {
    DbFactory::postgres(database_url).build_postgres().await
}
