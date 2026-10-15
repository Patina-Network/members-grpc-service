use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;
use metrics_process::Collector;

use sqlx::PgPool;

use crate::{
    config::Config,
    db::{GreetingsRepository, members::MemberSqlRepo},
    members::MemberService,
};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Config,
    pub prometheus_handle: PrometheusHandle,
    pub sys_collector: Collector,
    pub greetings_repo: Arc<GreetingsRepository>,
    pub db: PgPool,
    pub members: MemberService<MemberSqlRepo>,
}

impl AppState {
    pub fn new(
        config: &Config,
        prometheus_handle: PrometheusHandle,
        sys_collector: Collector,
        greetings_repo: Arc<GreetingsRepository>,
        db: PgPool,
        members: MemberService<MemberSqlRepo>,
    ) -> Self {
        Self {
            config: config.clone(),
            prometheus_handle,
            sys_collector,
            greetings_repo,
            db,
            members,
        }
    }
}
