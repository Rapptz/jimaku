use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::future::Either;
use quick_cache::sync::Cache;

use std::{
    future::{ready, Ready},
    hash::Hash,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime},
};
use tower::{Layer, Service};

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

impl<T: KeyExtractor> RateLimitLayer<T> {
    fn ratio(&self) -> f32 {
        self.per / self.rate as f32
    }

    fn is_ratelimited(&self, request: &Request) -> bool {
        let now = SystemTime::now();
        match self.extractor.extract(request) {
            None => false,
            Some(key) => {
                let tat = self.lookup.get(&key).unwrap_or(now).max(now);
                let ratio = self.ratio();
                let max_interval = Duration::from_secs_f32(self.per - ratio);
                if let Ok(diff) = now.duration_since(tat) {
                    if diff > max_interval {
                        return true;
                    }
                }

                let new_tat = tat.max(now) + Duration::from_secs_f32(ratio);
                self.lookup.insert(key, new_tat);
                false
            }
        }
    }

    fn error_response(&self) -> Response {
        StatusCode::TOO_MANY_REQUESTS.into_response()
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

impl<S, K> Service<Request> for RateLimitService<S, K>
where
    S: Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
    K: KeyExtractor,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Either<S::Future, Ready<Result<Self::Response, Self::Error>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if self.layer.is_ratelimited(&req) {
            Either::Right(ready(Ok(self.layer.error_response())))
        } else {
            Either::Left(self.inner.call(req))
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
