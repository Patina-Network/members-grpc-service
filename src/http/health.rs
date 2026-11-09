use axum::{extract::State, http::StatusCode};

use crate::{
    http::state::AppState,
    metrics::{HealthCheckOutcome, record_db_health_check},
};

pub async fn ready(State(state): State<AppState>) -> Result<&'static str, StatusCode> {
    let result = state.health_check_repository.check().await;
    record_db_health_check(if result.is_ok() {
        HealthCheckOutcome::Success
    } else {
        HealthCheckOutcome::Failure
    });

    result.map_err(|err| {
        tracing::warn!(%err, "database health check failed");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    Ok("ok")
}

pub async fn live() -> &'static str {
    "ok"
}
