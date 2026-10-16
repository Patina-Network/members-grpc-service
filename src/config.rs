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
        Self::from_vars(|key| env::var(key).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        Ok(Self {
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

fn required(var: &impl Fn(&str) -> Option<String>, name: &str) -> anyhow::Result<String> {
    var(name).with_context(|| format!("missing {name}"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Config;

    fn config_from(vars: &[(&str, &str)]) -> anyhow::Result<Config> {
        let vars: HashMap<&str, &str> = vars.iter().copied().collect();
        Config::from_vars(|key| vars.get(key).map(|value| (*value).to_owned()))
    }

    fn database_vars() -> [(&'static str, &'static str); 5] {
        [
            ("DATABASE_HOST", "localhost"),
            ("DATABASE_PORT", "5432"),
            ("DATABASE_NAME", "members"),
            ("DATABASE_USER", "postgres"),
            ("DATABASE_PASSWORD", "postgres"),
        ]
    }

    #[test]
    fn database_settings_are_required() {
        let err = config_from(&[]).unwrap_err().to_string();
        assert!(err.contains("DATABASE_HOST"), "{err}");
    }

    #[test]
    fn reads_database_settings() {
        let config = config_from(&database_vars()).unwrap();

        assert_eq!(config.database_host, "localhost");
        assert_eq!(config.database_port, 5432);
        assert_eq!(config.database_name, "members");
        assert_eq!(config.database_user, "postgres");
        assert_eq!(config.database_password, "postgres");
        assert_eq!(config.grpc_port, 50051);
        assert_eq!(config.http_port, 3000);
    }

    #[test]
    fn rejects_a_non_numeric_database_port() {
        let mut vars = database_vars();
        vars[1].1 = "db";
        let err = config_from(&vars).unwrap_err().to_string();
        assert!(err.contains("DATABASE_PORT"), "{err}");
    }
}
