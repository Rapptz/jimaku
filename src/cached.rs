//! Middleware that implements a cache layer.
//!
//! This is opt-in per route and only for unauthenticated requests.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE},
        HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use quick_cache::sync::Cache;
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// A timed cache value that only lasts for a specified duration before expiring.
#[derive(Debug)]
pub struct TimedCachedValue<T> {
    value: RwLock<Option<(T, Instant)>>,
    ttl: Duration,
}

impl<T> TimedCachedValue<T> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            value: RwLock::new(None),
            ttl,
        }
    }

    /// Returns the cached value, or [`None`] if it cannot be found or is expired
    pub async fn get(&self) -> Option<RwLockReadGuard<'_, T>> {
        let guard = self.value.read().await;
        RwLockReadGuard::try_map(guard, |f| {
            if let Some((value, exp)) = f {
                if exp.elapsed() >= self.ttl {
                    None
                } else {
                    Some(value)
                }
            } else {
                None
            }
        })
        .ok()
    }

    /// Sets the value in the cache and returns a read guard to the value
    pub async fn set(&self, value: T) -> RwLockReadGuard<'_, T> {
        let mut guard = self.value.write().await;
        *guard = Some((value, Instant::now()));
        RwLockWriteGuard::downgrade_map(guard, |f| &f.as_ref().unwrap().0)
    }

    /// Invalidates the cache
    pub async fn invalidate(&self) {
        let mut guard = self.value.write().await;
        *guard = None;
    }
}

// /// Implements the caching layer for the given route.
// ///
// /// This only caches HTML routes from e.g. Askama.
// #[derive(Clone)]
// pub struct CachedRoute {
//     cached: Arc<TimedCachedValue<Bytes>>,
// }

// impl CachedRoute {
//     pub fn new(ttl: Duration) -> Self {
//         Self {
//             cached: Arc::new(TimedCachedValue::new(ttl)),
//         }
//     }
// }

// #[derive(Clone)]
// pub struct CacheRouteService<S> {
//     layer: CachedRoute,
//     inner: S,
// }

// impl<S> Layer<S> for CachedRoute {
//     type Service = CacheRouteService<S>;

//     fn layer(&self, inner: S) -> Self::Service {
//         CacheRouteService {
//             layer: self.clone(),
//             inner,
//         }
//     }
// }

// impl<S> Service<Request> for CacheRouteService<S>
// where
//     S: Service<Request, Response = Response> + Send + 'static,
//     S::Future: Send + 'static,
// {
//     type Response = S::Response;
//     type Error = S::Error;
//     type Future = Either<BoxFuture<'static, Result<Self::Response, Self::Error>>, S::Future>;

//     fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.inner.poll_ready(cx)
//     }

//     fn call(&mut self, mut req: Request) -> Self::Future {
//         let token = get_token_from_request(&req);
//         if token.is_some() {
//             req.headers_mut()
//                 .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
//             return Either::Right(self.inner.call(req));
//         }
//         // if
//         // if self.layer.is_ratelimited(&req) {
//         //     Either::Right(ready(Ok(self.layer.error_response())))
//         // } else {
//         //     Either::Left(self.inner.call(req))
//         // }
//         let future = self.inner.call(req);
//         let layer = self.layer.clone();
//         Either::Left(Box::pin(async move {
//             if let Some(body) = layer.cached.get().await {
//                 Ok(Response::builder()
//                     .status(StatusCode::OK)
//                     .header(
//                         CACHE_CONTROL,
//                         format!("private, max-age={}", layer.cached.ttl.as_secs()),
//                     )
//                     .header("content-type", "text/html")
//                     .body(body.clone()))
//             } else {
//                 let response = future.await;
//                 if let Ok(resp) = &response {
//                     if let Ok(bytes) = resp.body().collect().await {

//                     }
//                 }
//             }
//         }))
//         // async move {
//         //     if let Some(body) = layer.cached.get().await {
//         //         Response::builder()
//         //             .status(StatusCode::OK)
//         //     }
//         // }
//         // Box::pin()
//     }
// }

/// Implements a cache for Askama templates
#[derive(Clone)]
pub struct TemplateCache {
    templates: Arc<Cache<&'static str, Option<(Bytes, Instant)>>>,
    ttl: Duration,
}

pub enum CachedTemplateResponse {
    Cached(Duration, Bytes),
    Bypass(Response),
    Error,
}

impl TemplateCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            templates: Arc::new(Cache::new(10)),
            ttl,
        }
    }

    fn get_cached(&self, key: &'static str) -> Option<Bytes> {
        let item = self.templates.get(&key)?;
        if let Some((item, exp)) = item {
            if exp.elapsed() >= self.ttl {
                None
            } else {
                Some(item)
            }
        } else {
            None
        }
    }

    pub async fn cache<T: askama::Template + IntoResponse>(
        &self,
        key: &'static str,
        template: T,
        bypass_cache: bool,
    ) -> CachedTemplateResponse {
        if bypass_cache {
            return CachedTemplateResponse::Bypass(template.into_response());
        }

        if let Some(cached) = self.get_cached(key) {
            return CachedTemplateResponse::Cached(self.ttl, cached);
        }

        // Cache miss
        if let Ok(rendered) = template.render() {
            let bytes = Bytes::from(rendered);
            self.templates.insert(key, Some((bytes.clone(), Instant::now())));
            CachedTemplateResponse::Cached(self.ttl, bytes)
        } else {
            CachedTemplateResponse::Error
        }
    }
}

impl IntoResponse for CachedTemplateResponse {
    fn into_response(self) -> Response {
        match self {
            CachedTemplateResponse::Cached(ttl, bytes) => {
                let mut resp = Response::new(Body::from(bytes));
                resp.headers_mut().insert(
                    CACHE_CONTROL,
                    HeaderValue::from_str(&format!("private, max-age={}", ttl.as_secs())).unwrap(),
                );
                resp.headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("text/html"));
                resp
            }
            CachedTemplateResponse::Bypass(mut resp) => {
                resp.headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
                resp
            }
            CachedTemplateResponse::Error => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    }
}
