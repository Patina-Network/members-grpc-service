use std::env;

use anyhow::Context;
use tracing::info;

#[derive(Clone)]
pub struct Config {
    pub profile: Option<String>,
    pub grpc_port: u16,
    pub http_port: u16,
    pub grpc_reflection: bool,
    pub version: Option<String>,
    pub database_host: String,
    pub database_port: u16,
    pub database_name: String,
    pub database_user: String,
    pub database_password: String,
}

impl Config {
    pub fn log(&self) {
        info!(
            profile = self.profile.as_deref().unwrap_or("N/A"),
            grpc_port = self.grpc_port,
            http_port = self.http_port,
            grpc_reflection = self.grpc_reflection,
            version = self.version.as_deref().unwrap_or("N/A"),
            database_host = self.database_host,
            database_port = self.database_port,
            database_name = self.database_name,
            database_user = self.database_user,
            "loaded config"
        );
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(|key| env::var(key).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        Ok(Self {
            profile: var("PROFILE").filter(|v| !v.is_empty()),
            grpc_reflection: var("GRPC_REFLECTION")
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            grpc_port: var("GRPC_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(50051),
            http_port: var("HTTP_PORT")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            version: var("VERSION").filter(|v| !v.is_empty()),
            database_host: required(&var, "DATABASE_HOST")?,
            database_port: required(&var, "DATABASE_PORT")?
                .parse()
                .context("DATABASE_PORT must be a number")?,
            database_name: required(&var, "DATABASE_NAME")?,
            database_user: required(&var, "DATABASE_USER")?,
            database_password: required(&var, "DATABASE_PASSWORD")?,
        })
    }
}

fn required(var: &impl Fn(&str) -> Option<String>, name: &str) -> anyhow::Result<String> {
    var(name).with_context(|| format!("missing {name}"))
}
