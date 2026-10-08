use std::env;

use tracing::info;

#[derive(Debug, Clone)]
pub struct Config {
    pub grpc_port: u16,
    pub http_port: u16,
    pub grpc_reflection: bool,
    pub version: Option<String>,
}

impl Config {
    pub fn log(&self) {
        info!(
            grpc_port = self.grpc_port,
            http_port = self.http_port,
            grpc_reflection = self.grpc_reflection,
            version = self.version.as_deref().unwrap_or("N/A"),
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
        })
    }
}
