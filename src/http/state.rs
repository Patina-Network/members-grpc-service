use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;

use members_grpc_service::{config::Config, db::health_check::HealthCheckRepository};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub prometheus_handle: PrometheusHandle,
    pub sys_collector: Collector,
    pub health_check_repository: HealthCheckRepository,
}

impl AppState {
    pub fn new(
        config: &Config,
        prometheus_handle: PrometheusHandle,
        sys_collector: Collector,
        health_check_repository: HealthCheckRepository,
    ) -> Self {
        Self {
            config: config.clone(),
            prometheus_handle,
            sys_collector,
            health_check_repository,
        }
    }
}
