#![allow(clippy::declare_interior_mutable_const)]

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue},
    response::{IntoResponse, Response},
};
use futures_util::future::Either;
use quick_cache::sync::Cache;

use std::{
    future::{ready, Future, Ready},
    hash::Hash,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower::{Layer, Service};

use crate::error::ApiError;

const X_RATELIMIT_LIMIT: HeaderName = HeaderName::from_static("x-ratelimit-limit");
const X_RATELIMIT_REMAINING: HeaderName = HeaderName::from_static("x-ratelimit-remaining");
const X_RATELIMIT_RESET: HeaderName = HeaderName::from_static("x-ratelimit-reset");
const X_RATELIMIT_RESET_AFTER: HeaderName = HeaderName::from_static("x-ratelimit-reset-after");

fn diff_seconds(a: SystemTime, b: SystemTime) -> f32 {
    let a = a.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let b = b.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    (a - b) as f32
}

/// Implements rate limiting using the GCRA algorithm.
///
/// Note that axum clones this *every* request.
#[derive(Clone)]
pub struct RateLimitLayer<T: KeyExtractor> {
    lookup: Arc<Cache<T::Key, SystemTime>>,
    rate: u16,
    per: f32,
    extractor: T,
}

/// A trait that describes the rate limit policy
pub trait KeyExtractor: Clone {
    /// The underlying type of the key
    type Key: Hash + Eq + Clone + Send + Sync + 'static;

    /// Extracts the lookup key from a request.
    ///
    /// If no key is found for this request then `None` should be returned.
    fn extract(&self, req: &Request) -> Option<Self::Key>;
}

#[derive(Debug, Copy, Clone)]
struct RateLimitInfo {
    limit: u16,
    remaining: u16,
    reset_time: SystemTime,
    retry_after: f32,
}

impl<T: KeyExtractor> RateLimitLayer<T> {
    fn emission_interval(&self) -> f32 {
        self.per / self.rate as f32
    }

    fn process(&self, request: &Request) -> RateLimitInfo {
        let emission_interval = self.emission_interval();
        let limit = self.rate;
        let delay_variation_tolerance = self.per;
        let now = SystemTime::now();
        let Some(key) = self.extractor.extract(request) else {
            return RateLimitInfo::banned();
        };

        let tat = self.lookup.get(&key).unwrap_or(now);
        let new_tat = tat.max(now) + Duration::from_secs_f32(emission_interval);

        let allow_at = new_tat - Duration::from_secs_f32(delay_variation_tolerance);
        let diff = diff_seconds(now, allow_at);
        let mut remaining = ((diff / emission_interval) + 0.5).floor() as u16;
        let retry_after = if remaining < 1 {
            remaining = 0;
            emission_interval - diff
        } else {
            0.0
        };

        self.lookup.insert(key, new_tat);
        RateLimitInfo {
            limit,
            remaining,
            reset_time: now + Duration::from_secs_f32(retry_after),
            retry_after,
        }
    }
}

impl RateLimitInfo {
    fn is_ratelimited(&self) -> bool {
        self.remaining == 0
    }

    fn banned() -> Self {
        Self {
            limit: 0,
            remaining: 0,
            reset_time: UNIX_EPOCH,
            retry_after: 0.0,
        }
    }

    fn modify_headers(&self, resp: &mut Response) {
        resp.headers_mut().insert(
            X_RATELIMIT_LIMIT,
            HeaderValue::from_str(&self.limit.to_string()).unwrap(),
        );
        resp.headers_mut().insert(
            X_RATELIMIT_REMAINING,
            HeaderValue::from_str(&self.remaining.to_string()).unwrap(),
        );
        if let Ok(epoch) = self.reset_time.duration_since(UNIX_EPOCH) {
            resp.headers_mut().insert(
                X_RATELIMIT_RESET,
                HeaderValue::from_str(&epoch.as_secs_f32().to_string()).unwrap(),
            );
        }

        if self.remaining == 0 {
            resp.headers_mut().insert(
                X_RATELIMIT_RESET_AFTER,
                HeaderValue::from_str(&self.retry_after.to_string()).unwrap(),
            );
        }
    }
}

impl IntoResponse for RateLimitInfo {
    fn into_response(self) -> Response {
        let mut resp = ApiError::rate_limited().into_response();
        self.modify_headers(&mut resp);
        resp
    }
}

#[derive(Clone)]
pub struct RateLimitService<S, T: KeyExtractor> {
    layer: RateLimitLayer<T>,
    inner: S,
}

impl<S, T: KeyExtractor> Layer<S> for RateLimitLayer<T> {
    type Service = RateLimitService<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            layer: self.clone(),
            inner,
        }
    }
}

pin_project_lite::pin_project! {
    pub struct ModifyHeaders<F, E>
    where
        F: Future<Output = Result<Response, E>>
    {
        #[pin]
        inner: F,
        info: RateLimitInfo,
    }
}

impl<F, E> Future for ModifyHeaders<F, E>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = F::Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = match this.inner.poll(cx) {
            Poll::Ready(t) => t,
            Poll::Pending => return Poll::Pending,
        };
        if let Ok(resp) = &mut res {
            this.info.modify_headers(resp);
        }
        res.into()
    }
}

impl<S, K> Service<Request> for RateLimitService<S, K>
where
    S: Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
    K: KeyExtractor,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Either<ModifyHeaders<S::Future, S::Error>, Ready<Result<Self::Response, Self::Error>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let info = self.layer.process(&req);
        if info.is_ratelimited() {
            Either::Right(ready(Ok(info.into_response())))
        } else {
            Either::Left(ModifyHeaders {
                inner: self.inner.call(req),
                info,
            })
        }
    }
}

/// A global key extractor for a global rate limit
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GlobalKeyExtractor;

impl KeyExtractor for GlobalKeyExtractor {
    type Key = ();

    fn extract(&self, _req: &Request) -> Option<Self::Key> {
        Some(())
    }
}

/// A key extractor based on IPs
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct IpKeyExtractor;

impl KeyExtractor for IpKeyExtractor {
    type Key = IpAddr;

    fn extract(&self, req: &Request) -> Option<Self::Key> {
        req.extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|addr| addr.ip())
    }
}

/// A builder for creating [`RateLimitLayer`].
pub struct RateLimit<T: KeyExtractor> {
    max_capacity: usize,
    rate: u16,
    per: f32,
    extractor: T,
}

impl Default for RateLimit<IpKeyExtractor> {
    /// Creates the default rate limit configuration with 5 requests per 5 seconds
    /// using [`IpKeyExtractor`] as the key.
    fn default() -> Self {
        Self {
            max_capacity: 10_000,
            rate: 5,
            per: 5.0,
            extractor: IpKeyExtractor,
        }
    }
}

impl<T: KeyExtractor> RateLimit<T> {
    pub fn extractor<U: KeyExtractor>(self, key: U) -> RateLimit<U> {
        RateLimit {
            max_capacity: self.max_capacity,
            rate: self.rate,
            per: self.per,
            extractor: key,
        }
    }

    pub fn max_capacity(mut self, capacity: usize) -> Self {
        self.max_capacity = capacity;
        self
    }

    pub fn quota(mut self, rate: u16, per: f32) -> Self {
        self.rate = rate;
        self.per = per;
        self
    }

    pub fn build(self) -> RateLimitLayer<T> {
        RateLimitLayer {
            lookup: Arc::new(Cache::new(self.max_capacity)),
            rate: self.rate,
            per: self.per,
            extractor: self.extractor,
        }
    }
}
