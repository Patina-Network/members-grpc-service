use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;
use sqlx::PgPool;

use crate::{config::Config, db::health_check::HealthCheckRepository};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub prometheus_handle: PrometheusHandle,
    pub sys_collector: Collector,
    pub health_check_repository: HealthCheckRepository,
    pub db_pool: PgPool,
}
