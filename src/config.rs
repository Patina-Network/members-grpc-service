use std::env;

use anyhow::Context;
use tracing::info;

#[derive(Clone)]
pub struct Config {
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
        Ok(Self {
            grpc_reflection: env::var("GRPC_REFLECTION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(false),
            grpc_port: env::var("GRPC_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50051),
            http_port: env::var("HTTP_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            version: env::var("VERSION").ok().filter(|v| !v.is_empty()),
            database_host: required_env("DATABASE_HOST")?,
            database_port: required_env("DATABASE_PORT")?
                .parse()
                .context("DATABASE_PORT must be a number")?,
            database_name: required_env("DATABASE_NAME")?,
            database_user: required_env("DATABASE_USER")?,
            database_password: required_env("DATABASE_PASSWORD")?,
        })
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("grpc_port", &self.grpc_port)
            .field("http_port", &self.http_port)
            .field("grpc_reflection", &self.grpc_reflection)
            .field("version", &self.version)
            .field("database_host", &self.database_host)
            .field("database_port", &self.database_port)
            .field("database_name", &self.database_name)
            .field("database_user", &self.database_user)
            .field("database_password", &"***")
            .finish()
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("missing {name}"))
}
