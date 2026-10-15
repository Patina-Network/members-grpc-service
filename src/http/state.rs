use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub prometheus_handle: PrometheusHandle,
    pub sys_collector: Collector,
}

impl AppState {
    pub fn new(
        config: &Config,
        prometheus_handle: PrometheusHandle,
        sys_collector: Collector,
    ) -> Self {
        Self {
            config: config.clone(),
            prometheus_handle,
            sys_collector,
        }
    }
}
