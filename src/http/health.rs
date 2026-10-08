/// k8s will use this to determine non-recoverability.
/// If it returns a non-200, it will kill the pod & restart it.
///
/// Avoid making any external service calls here. Those should go in `/livez`.
pub async fn ready() -> &'static str {
    "ok"
}

/// k8s will use this to determine instability.
/// If it returns a non-200, it will stop sending requests
/// to the pod until it has recovered.
pub async fn live() -> &'static str {
    "ok"
}
