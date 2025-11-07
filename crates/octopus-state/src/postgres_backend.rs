//! PostgreSQL state backend implementation

use crate::{Error, Result, StateBackend};
use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::time::Duration;
use tracing::{debug, trace};

/// PostgreSQL state backend
///
/// ACID guarantees, durable storage, perfect for compliance-heavy use cases.
/// Slower than Redis but provides transactional guarantees and persistent audit trails.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: PgPool,
    table_name: String,
}

impl std::fmt::Debug for PostgresBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresBackend")
            .field("table_name", &self.table_name)
            .finish()
    }
}

impl PostgresBackend {
    /// Create a new PostgreSQL backend
    pub async fn new(url: &str, pool_size: u32, table_name: String) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_size)
            .connect(url)
            .await
            .map_err(|e| Error::Connection(e.to_string()))?;

        let backend = Self { pool, table_name };

        // Create table if it doesn't exist
        backend.init_schema().await?;

        debug!(url, table = %backend.table_name, "PostgreSQL backend connected");

        Ok(backend)
    }

    /// Initialize database schema
    async fn init_schema(&self) -> Result<()> {
        let query = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                key TEXT PRIMARY KEY,
                value BYTEA NOT NULL,
                expires_at TIMESTAMPTZ,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_{}_expires_at 
            ON {} (expires_at) 
            WHERE expires_at IS NOT NULL;
            "#,
            self.table_name, self.table_name, self.table_name
        );

        sqlx::query(&query).execute(&self.pool).await?;

        debug!(table = %self.table_name, "Schema initialized");

        Ok(())
    }

    /// Clean up expired entries
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let query = format!(
            "DELETE FROM {} WHERE expires_at IS NOT NULL AND expires_at < NOW()",
            self.table_name
        );

        let result = sqlx::query(&query).execute(&self.pool).await?;

        let rows_affected = result.rows_affected();
        if rows_affected > 0 {
            debug!(rows = rows_affected, "Cleaned up expired entries");
        }

        Ok(rows_affected)
    }
}

#[async_trait]
impl StateBackend for PostgresBackend {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        trace!(key, "PostgreSQL GET");

        let query = format!(
            "SELECT value FROM {} WHERE key = $1 AND (expires_at IS NULL OR expires_at > NOW())",
            self.table_name
        );

        let result = sqlx::query(&query)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result.map(|row| row.get::<Vec<u8>, _>("value")))
    }

    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()> {
        trace!(key, ttl_secs = ?ttl.map(|d| d.as_secs()), "PostgreSQL SET");

        let expires_at = ttl.map(|d| chrono::Utc::now() + chrono::Duration::from_std(d).unwrap());

        let query = format!(
            r#"
            INSERT INTO {} (key, value, expires_at, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (key) DO UPDATE
            SET value = EXCLUDED.value,
                expires_at = EXCLUDED.expires_at,
                updated_at = NOW()
            "#,
            self.table_name
        );

        sqlx::query(&query)
            .bind(key)
            .bind(value)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64> {
        trace!(key, delta, "PostgreSQL INCREMENT");

        let expires_at = ttl.map(|d| chrono::Utc::now() + chrono::Duration::from_std(d).unwrap());

        // Use a transaction for atomic increment
        let mut tx = self.pool.begin().await?;

        // Try to get current value
        let query = format!(
            "SELECT value FROM {} WHERE key = $1 AND (expires_at IS NULL OR expires_at > NOW()) FOR UPDATE",
            self.table_name
        );

        let current: Option<i64> = sqlx::query(&query)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            .and_then(|row| {
                let bytes: Vec<u8> = row.get("value");
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|s| s.parse().ok())
            });

        let new_value = current.unwrap_or(0) + delta;

        // Upsert with new value
        let query = format!(
            r#"
            INSERT INTO {} (key, value, expires_at, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (key) DO UPDATE
            SET value = EXCLUDED.value,
                expires_at = EXCLUDED.expires_at,
                updated_at = NOW()
            "#,
            self.table_name
        );

        sqlx::query(&query)
            .bind(key)
            .bind(new_value.to_string().as_bytes())
            .bind(expires_at)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(new_value)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        trace!(key, "PostgreSQL DELETE");

        let query = format!("DELETE FROM {} WHERE key = $1", self.table_name);

        sqlx::query(&query).bind(key).execute(&self.pool).await?;

        Ok(())
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected: Vec<u8>,
        new_value: Vec<u8>,
    ) -> Result<bool> {
        trace!(key, "PostgreSQL CAS");

        let query = format!(
            r#"
            UPDATE {} 
            SET value = $2, updated_at = NOW()
            WHERE key = $1 
            AND value = $3 
            AND (expires_at IS NULL OR expires_at > NOW())
            "#,
            self.table_name
        );

        let result = sqlx::query(&query)
            .bind(key)
            .bind(new_value)
            .bind(expected)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool> {
        trace!(key, ttl_secs = ttl.as_secs(), "PostgreSQL EXPIRE");

        let expires_at = chrono::Utc::now() + chrono::Duration::from_std(ttl).unwrap();

        let query = format!(
            "UPDATE {} SET expires_at = $2, updated_at = NOW() WHERE key = $1",
            self.table_name
        );

        let result = sqlx::query(&query)
            .bind(key)
            .bind(expires_at)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn keys(&self, pattern: &str) -> Result<Vec<String>> {
        trace!(pattern, "PostgreSQL KEYS");

        // Convert glob pattern to SQL LIKE pattern
        let sql_pattern = pattern.replace("*", "%").replace("?", "_");

        let query = format!(
            "SELECT key FROM {} WHERE key LIKE $1 AND (expires_at IS NULL OR expires_at > NOW())",
            self.table_name
        );

        let rows = sqlx::query(&query)
            .bind(sql_pattern)
            .fetch_all(&self.pool)
            .await?;

        let keys = rows.into_iter().map(|row| row.get("key")).collect();

        Ok(keys)
    }

    async fn flush(&self) -> Result<()> {
        debug!(table = %self.table_name, "PostgreSQL TRUNCATE");

        let query = format!("TRUNCATE TABLE {}", self.table_name);

        sqlx::query(&query).execute(&self.pool).await?;

        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::Backend(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests require a running PostgreSQL instance
    // Run with: docker run -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16-alpine

    async fn setup() -> Option<PostgresBackend> {
        PostgresBackend::new(
            "postgresql://postgres:postgres@localhost:5432/postgres",
            5,
            "octopus_test_state".to_string(),
        )
        .await
        .ok()
    }

    #[tokio::test]
    async fn test_postgres_get_set() {
        let Some(backend) = setup().await else {
            eprintln!("Skipping PostgreSQL tests - PostgreSQL not available");
            return;
        };

        backend
            .set("test_key", b"test_value".to_vec(), None)
            .await
            .unwrap();
        let value = backend.get("test_key").await.unwrap();

        assert_eq!(value, Some(b"test_value".to_vec()));

        backend.delete("test_key").await.unwrap();
    }

    #[tokio::test]
    async fn test_postgres_increment() {
        let Some(backend) = setup().await else {
            return;
        };

        let val1 = backend.increment("pg_counter", 1, None).await.unwrap();
        assert_eq!(val1, 1);

        let val2 = backend.increment("pg_counter", 5, None).await.unwrap();
        assert_eq!(val2, 6);

        backend.delete("pg_counter").await.unwrap();
    }

    #[tokio::test]
    async fn test_postgres_health_check() {
        let Some(backend) = setup().await else {
            return;
        };

        assert!(backend.health_check().await.is_ok());
    }
}
