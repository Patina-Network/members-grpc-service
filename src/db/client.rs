use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::config::Config;

pub async fn connect(config: &Config) -> anyhow::Result<sqlx::PgPool> {
    PgPoolOptions::new()
        .max_connections(10)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect_with(connect_options(config))
        .await
        .context("failed to connect to postgres")
}

fn connect_options(config: &Config) -> PgConnectOptions {
    PgConnectOptions::new()
        .host(&config.database_host)
        .port(config.database_port)
        .database(&config.database_name)
        .username(&config.database_user)
        .password(&config.database_password)
}
