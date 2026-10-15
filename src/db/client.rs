use std::borrow::Cow;
use std::time::Duration;

use anyhow::Context;
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::config::Config;

pub async fn connect(config: &Config) -> anyhow::Result<sqlx::PgPool> {
    let options = PgConnectOptions::new()
        .host(&config.database_host)
        .port(config.database_port)
        .database(&config.database_name)
        .username(&config.database_user)
        .password(&config.database_password);

    // Same pool sizes as Codebloom's Hikari config.
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(options)
        .await
        .context("failed to connect to postgres")?;

    migrate(&pool).await?;

    Ok(pool)
}

pub async fn migrate(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    migrator()
        .run(pool)
        .await
        .context("failed to run database migrations")
}

fn migrator() -> Migrator {
    let sql = include_str!("../../db/migration/V0001__Create_members_table.SQL");
    Migrator {
        migrations: Cow::Owned(vec![Migration::new(
            1,
            Cow::Borrowed("create members table"),
            MigrationType::Simple,
            Cow::Borrowed(sql),
            false,
        )]),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    }
}
