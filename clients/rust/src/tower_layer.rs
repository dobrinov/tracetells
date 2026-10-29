//! A tower layer for HTTP services (axum, tonic's HTTP, hyper with tower):
//! responses with a 5xx status are reported with their route, method and
//! status, and a panic in a handler knows the request it happened in.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::{Request as HttpRequest, Response};
use tower::{Layer, Service};

use crate::Request;

/// Wrap your router: `Router::new().route(...).layer(tracetells::TraceLayer)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraceLayer;

impl<S> Layer<S> for TraceLayer {
    type Service = TraceService<S>;
    fn layer(&self, inner: S) -> Self::Service {
        TraceService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct TraceService<S> {
    inner: S,
}

/// The route template when the framework knows it (axum's matched path), else
/// the path itself, which is scrubbed of ids before it's sent.
fn route_of<B>(req: &HttpRequest<B>) -> String {
    #[cfg(feature = "axum")]
    if let Some(m) = req.extensions().get::<axum::extract::MatchedPath>() {
        return m.as_str().to_string();
    }
    req.uri().path().to_string()
}

impl<S, B, R> Service<HttpRequest<B>> for TraceService<S>
where
    S: Service<HttpRequest<B>, Response = Response<R>>,
    S::Error: std::fmt::Display,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequest<B>) -> Self::Future {
        let request = Request { route: route_of(&req), method: req.method().as_str().to_string(), status: 0 };
        let inner = crate::with_request(&request, || self.inner.call(req));
        ResponseFuture { inner, request }
    }
}

pin_project_lite::pin_project! {
    pub struct ResponseFuture<F> {
        #[pin]
        inner: F,
        request: Request,
    }
}

impl<F, R, E> Future for ResponseFuture<F>
where
    F: Future<Output = Result<Response<R>, E>>,
    E: std::fmt::Display,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let request = this.request;
        let out = crate::with_request(request, || this.inner.poll(cx));
        if let Poll::Ready(result) = &out {
            match result {
                Ok(res) if res.status().is_server_error() => {
                    let status = res.status();
                    let req = Request { status: status.as_u16(), ..request.clone() };
                    crate::capture(format!("HTTP {}", status.as_u16()), status.canonical_reason().unwrap_or("Server error").to_string(), req);
                }
                Err(e) => crate::capture("ServiceError".into(), e.to_string(), Request { status: 500, ..request.clone() }),
                _ => {}
            }
        }
        out
    }
}
