use backend_core::{Error, Result};
use diesel::Connection;
use diesel::pg::PgConnection;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

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
        let _ = tokio::task::spawn_blocking(move || {
            let mut conn = PgConnection::establish(&url)
                .map_err(|e| Error::Database(format!("Failed to connect for migrations: {}", e)))?;

            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| Error::Database(format!("Migration failed: {}", e)))?;

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
