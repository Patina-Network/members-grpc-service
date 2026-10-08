use std::sync::Arc;

use anyhow::Context;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::net::TcpListener;
use tonic::transport::Server;
use tonic_middleware::MiddlewareLayer;
use tower_http::catch_panic;
use tracing::info;

use crate::{
    config::Config,
    db::GreetingsRepository,
    grpc::greeter::{GreeterService, GreeterServiceServer},
    grpc::metrics::GrpcRequestMetricsMiddleware,
    http::{router, state::AppState},
};

mod config;
mod db;
mod grpc;
mod http;
mod metrics;

fn init_tracing() {
    let use_json = std::env::var("ENVIRONMENT")
        .map(|v| v.eq_ignore_ascii_case("production") || v.eq_ignore_ascii_case("staging"))
        .unwrap_or(false);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if use_json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .pretty()
            .with_env_filter(env_filter)
            .init();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let prom_handle = PrometheusBuilder::new()
        .install_recorder()
        .context("failed to install Prometheus recorder")?;
    let sys_collector = metrics_process::Collector::default();
    sys_collector.describe();

    let config = Config::from_env()?;
    config.log();

    let grpc_addr = format!("0.0.0.0:{}", config.grpc_port).parse()?;
    let http_addr = format!("0.0.0.0:{}", config.http_port);

    // repos and internal svcs should be wrapped in Arc so that we can pass them into multiple grpc
    // svc impls without expensive cloning (especially when we have an actual database connection)
    let greeter_repo = Arc::new(GreetingsRepository::new());

    let grpc_greeter_svc = GreeterService::new(greeter_repo.clone());
    let (grpc_health_reporter, grpc_health_svc) = tonic_health::server::health_reporter();
    grpc_health_reporter
        .set_serving::<GreeterServiceServer<GreeterService>>()
        .await;
    grpc_health_reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let (grpc_reflection_svc, grpc_reflection_svc2) = if config.grpc_reflection {
        (
            Some(
                tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(grpc::greeter::FILE_DESCRIPTOR_SET)
                    .build_v1()
                    .context("failed to build v1 gRPC reflection service")?,
            ),
            Some(
                tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(grpc::greeter::FILE_DESCRIPTOR_SET)
                    .build_v1alpha()
                    .context("failed to build v1alpha gRPC reflection service")?,
            ),
        )
    } else {
        (None, None)
    };

    let grpc_panic_layer = catch_panic::CatchPanicLayer::custom(|_panic_info| {
        let status = tonic::Status::internal("An unhandled panic occurred");
        status.into_http::<tonic::body::Body>()
    });

    info!(addr = %grpc_addr, "starting grpc server");
    let grpc_server = tokio::spawn(async move {
        Server::builder()
            .layer(grpc_panic_layer)
            .layer(MiddlewareLayer::new(GrpcRequestMetricsMiddleware))
            .add_service(grpc_health_svc)
            .add_service(GreeterServiceServer::new(grpc_greeter_svc))
            .add_optional_service(grpc_reflection_svc)
            .add_optional_service(grpc_reflection_svc2)
            .serve(grpc_addr)
            .await
    });

    let http_app_state = AppState::new(&config, prom_handle, sys_collector, greeter_repo);

    let http_listener = TcpListener::bind(&http_addr)
        .await
        .context("failed to bind to http address")?;
    info!(addr = %http_addr, "starting http server");
    let http_server =
        tokio::spawn(async move { axum::serve(http_listener, router(http_app_state)).await });

    let (http_result, grpc_result) = tokio::join!(http_server, grpc_server);
    http_result.context("HTTP server task panicked")??;
    grpc_result.context("gRPC server task panicked")??;

    Ok(())
}
