// This code is adapted from fasterthanli.me

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Instant,
};

use axum::{extract::Request, response::Response};
use tower::{Layer, Service};
use tracing::{field, info_span, instrument::Instrumented, Instrument, Span};

use crate::token::get_token_from_request;

/// Layer for [HttpTraceService]
#[derive(Copy, Clone, Default)]
pub struct HttpTrace;

impl<S> Layer<S> for HttpTrace {
    type Service = HttpTraceService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpTraceService { inner }
    }
}

#[derive(Clone)]
pub struct HttpTraceService<S> {
    inner: S,
}

impl<S> Service<Request> for HttpTraceService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = PostFuture<Instrumented<S::Future>, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let start = Instant::now();

        let user_agent = req
            .headers()
            .get("user-agent")
            .and_then(|s| s.to_str().ok())
            .unwrap_or("");

        let host = req.headers().get("host").and_then(|s| s.to_str().ok()).unwrap_or("");

        let referrer = req.headers().get("referer").and_then(|s| s.to_str().ok()).unwrap_or("");

        let span = info_span!(
            "http request",
            http.method = %req.method(),
            http.url = %req.uri(),
            http.status_code = field::Empty,
            http.user_agent = &user_agent,
            http.referrer = &referrer,
            http.host = &host,
            http.latency = field::Empty,
            user_id = field::Empty,
        );

        if let Some(token) = get_token_from_request(req.extensions()) {
            span.record("user_id", token.id);
        }

        let fut = {
            let _guard = span.enter();
            self.inner.call(req)
        };
        PostFuture {
            inner: fut.instrument(span.clone()),
            span,
            start,
        }
    }
}

pin_project_lite::pin_project! {
    /// Future that records http status code
    pub struct PostFuture<F, E>
    where
        F: Future<Output = Result<Response, E>>,
    {
        #[pin]
        inner: F,
        span: Span,
        start: Instant,
    }
}

impl<F, E> Future for PostFuture<F, E>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let res = match this.inner.poll(cx) {
            Poll::Ready(t) => t,
            Poll::Pending => return Poll::Pending,
        };
        let latency = this.start.elapsed();
        this.span.record("http.latency", latency.as_micros() as u64);
        if let Ok(res) = &res {
            this.span.record("http.status_code", res.status().as_u16());
        }
        res.into()
    }
}
