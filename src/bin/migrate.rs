use members_grpc_service::{config::Config, db::client};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    client::migrate_database(&config).await?;
    println!("migrations complete");
    Ok(())
}
