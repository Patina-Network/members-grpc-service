use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;
use sqlx::PgPool;

use members_grpc_service::config::Config;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub prometheus_handle: PrometheusHandle,
    pub sys_collector: Collector,
    pub db: PgPool,
}

impl AppState {
    pub fn new(
        config: &Config,
        prometheus_handle: PrometheusHandle,
        sys_collector: Collector,
        db: PgPool,
    ) -> Self {
        Self {
            config: config.clone(),
            prometheus_handle,
            sys_collector,
            db,
        }
    }
}
