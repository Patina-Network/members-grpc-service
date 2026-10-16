use std::time::Duration;

use anyhow::Context;
use sqlx::PgPool;

#[derive(Clone)]
pub struct HealthCheckRepository {
    pool: PgPool,
}

impl HealthCheckRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn check(&self) -> anyhow::Result<()> {
        tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query("SELECT 1").execute(&self.pool),
        )
        .await
        .context("database health check timed out")?
        .context("database health check failed")?;
        Ok(())
    }
}
