use std::{
    sync::Arc,
    task::{Context, Poll},
    time::Instant,
};

use bytes::Bytes;
use pin_project_lite::pin_project;
use tonic::{
    Code, Status,
    body::Body,
    codegen::http::{HeaderMap, Request, Response},
};
use tonic_middleware::{Middleware, ServiceBound};

use crate::metrics::{self, GrpcStatus};

// TODO: migrate this to a separate library

#[derive(Debug, Clone)]
pub struct GrpcRequestMetricsMiddleware;

#[tonic::async_trait]
impl<S> Middleware<S> for GrpcRequestMetricsMiddleware
where
    S: ServiceBound,
    S::Future: Send,
{
    async fn call(&self, req: Request<Body>, mut service: S) -> Result<Response<Body>, S::Error> {
        let start = Instant::now();
        let grpc_method = Arc::from(req.uri().path().trim_start_matches('/'));

        let result = service.call(req).await;

        match result {
            Ok(response) => {
                if let Some(status) = status_from(response.headers()) {
                    record(grpc_method, status, start);
                    return Ok(response);
                }

                let (parts, body) = response.into_parts();
                let body = ObservedGrpcBody {
                    body,
                    recorder: GrpcMetricsRecorder {
                        grpc_method,
                        start,
                        recorded: false,
                    },
                };

                Ok(Response::from_parts(parts, Body::new(body)))
            }
            Err(err) => {
                record(grpc_method, GrpcStatus::TransportError, start);
                Err(err)
            }
        }
    }
}

fn status_from(headers: &HeaderMap) -> Option<GrpcStatus> {
    headers
        .get("grpc-status")
        .map(|value| Code::from_bytes(value.as_bytes()).into())
}

fn record(grpc_method: Arc<str>, status: GrpcStatus, start: Instant) {
    metrics::record_grpc_request(grpc_method, status, start.elapsed());
}

pin_project! {
    struct ObservedGrpcBody {
        #[pin]
        body: Body,
        recorder: GrpcMetricsRecorder,
    }
}

struct GrpcMetricsRecorder {
    grpc_method: Arc<str>,
    start: Instant,
    recorded: bool,
}

impl GrpcMetricsRecorder {
    fn record(&mut self, status: GrpcStatus) {
        if self.recorded {
            return;
        }

        self.recorded = true;
        record(self.grpc_method.clone(), status, self.start);
    }
}

impl Drop for GrpcMetricsRecorder {
    fn drop(&mut self) {
        if !self.recorded {
            self.record(GrpcStatus::Incomplete);
        }
    }
}

impl http_body::Body for ObservedGrpcBody {
    type Data = Bytes;
    type Error = Status;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        match this.body.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(trailers) = frame.trailers_ref() {
                    let status = status_from(trailers).unwrap_or(GrpcStatus::Unknown);
                    this.recorder.record(status);
                }

                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(status))) => {
                this.recorder.record(status.code().into());

                Poll::Ready(Some(Err(status)))
            }
            Poll::Ready(None) => {
                this.recorder.record(GrpcStatus::Ok);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
}
