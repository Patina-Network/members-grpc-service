use std::{sync::Arc, time::Duration};

use anyhow::Context;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use sqlx::PgPool;
use strum::AsRefStr;
use tonic::Code;

const GRPC_REQUESTS: &str = "grpc_requests_total";
const GRPC_REQUEST_DURATION: &str = "grpc_request_duration_seconds";

const DB_POOL_CONNECTIONS: &str = "db_pool_connections";
const DB_POOL_MAX_CONNECTIONS: &str = "db_pool_max_connections";
const DB_HEALTH_CHECKS: &str = "db_health_checks_total";
const BULK_MEMBER_OPERATIONS: &str = "bulk_member_operations_total";

const LATENCY_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// gRPC status of a served request, used as the `status` label.
#[derive(Clone, Copy, AsRefStr)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum GrpcStatus {
    Ok,
    Cancelled,
    Unknown,
    InvalidArgument,
    DeadlineExceeded,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    OutOfRange,
    Unimplemented,
    Internal,
    Unavailable,
    DataLoss,
    Unauthenticated,
    /// The service errored before producing a response.
    TransportError,
    /// The response body was dropped before its status arrived.
    Incomplete,
}

impl From<Code> for GrpcStatus {
    fn from(code: Code) -> Self {
        match code {
            Code::Ok => Self::Ok,
            Code::Cancelled => Self::Cancelled,
            Code::Unknown => Self::Unknown,
            Code::InvalidArgument => Self::InvalidArgument,
            Code::DeadlineExceeded => Self::DeadlineExceeded,
            Code::NotFound => Self::NotFound,
            Code::AlreadyExists => Self::AlreadyExists,
            Code::PermissionDenied => Self::PermissionDenied,
            Code::ResourceExhausted => Self::ResourceExhausted,
            Code::FailedPrecondition => Self::FailedPrecondition,
            Code::Aborted => Self::Aborted,
            Code::OutOfRange => Self::OutOfRange,
            Code::Unimplemented => Self::Unimplemented,
            Code::Internal => Self::Internal,
            Code::Unavailable => Self::Unavailable,
            Code::DataLoss => Self::DataLoss,
            Code::Unauthenticated => Self::Unauthenticated,
        }
    }
}

#[derive(Clone, Copy, AsRefStr)]
#[strum(serialize_all = "snake_case")]
enum PoolState {
    Idle,
    Active,
}

/// Result of a `/readyz` database check, used as the `outcome` label.
#[derive(Clone, Copy, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum HealthCheckOutcome {
    Success,
    Failure,
}

/// Operation type inside `bulkMemberOperations`, used as the `kind` label.
#[derive(Clone, Copy, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum BulkOperationKind {
    Create,
    Update,
    Archive,
    Upsert,
    Missing,
}

/// Per-operation result inside `bulkMemberOperations`, used as the `outcome` label.
#[derive(Clone, Copy, AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub enum BulkOperationOutcome {
    Created,
    Updated,
    Archived,
    Unchanged,
    Failed,
}

pub fn install_recorder() -> anyhow::Result<PrometheusHandle> {
    PrometheusBuilder::new()
        .set_buckets(LATENCY_BUCKETS)
        .context("invalid histogram buckets")?
        .install_recorder()
        .context("failed to install Prometheus recorder")
}

/// `method` is the request path without the leading `/`, e.g. `members.MembersService/listMembers`.
pub fn record_grpc_request(method: Arc<str>, status: GrpcStatus, elapsed: Duration) {
    let status = status.as_ref().to_owned();

    ::metrics::counter!(GRPC_REQUESTS, "method" => method.clone(), "status" => status.clone())
        .increment(1);
    ::metrics::histogram!(GRPC_REQUEST_DURATION, "method" => method, "status" => status)
        .record(elapsed);
}

/// Snapshot of the connection pool; call on every scrape.
pub fn record_db_pool(pool: &PgPool) {
    let size = pool.size();
    let idle = u32::try_from(pool.num_idle()).unwrap_or(size);

    ::metrics::gauge!(DB_POOL_CONNECTIONS, "state" => PoolState::Idle.as_ref().to_owned())
        .set(idle);
    ::metrics::gauge!(DB_POOL_CONNECTIONS, "state" => PoolState::Active.as_ref().to_owned())
        .set(size.saturating_sub(idle));
    ::metrics::gauge!(DB_POOL_MAX_CONNECTIONS).set(pool.options().get_max_connections());
}

pub fn record_db_health_check(outcome: HealthCheckOutcome) {
    ::metrics::counter!(DB_HEALTH_CHECKS, "outcome" => outcome.as_ref().to_owned()).increment(1);
}

pub fn record_bulk_member_operation(kind: BulkOperationKind, outcome: BulkOperationOutcome) {
    ::metrics::counter!(
        BULK_MEMBER_OPERATIONS,
        "kind" => kind.as_ref().to_owned(),
        "outcome" => outcome.as_ref().to_owned(),
    )
    .increment(1);
}
