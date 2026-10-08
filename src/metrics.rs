use std::future::Future;

pub trait MetricsCollector {
    fn collect(&self) -> impl Future<Output = ()> + Send;
}
