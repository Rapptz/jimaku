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

#[derive(Clone)]
struct CachedBody {
    decompressed: Bytes,
    expiry: Instant,
}

impl CachedBody {
    fn new(body: Bytes) -> Self {
        Self {
            decompressed: body,
            expiry: Instant::now(),
        }
    }
}

/// Implements a cache for a response
#[derive(Clone)]
pub struct BodyCache {
    templates: Arc<Cache<&'static str, Option<CachedBody>>>,
    ttl: Duration,
}

pub enum CachedTemplateResponse {
    Cached(Duration, Bytes),
    Bypass(Response),
    Error,
}

impl BodyCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            templates: Arc::new(Cache::new(10)),
            ttl,
        }
    }

    fn get_cached(&self, key: &'static str) -> Option<Bytes> {
        let item = self.templates.get(&key)?;
        if let Some(body) = item {
            if body.expiry.elapsed() >= self.ttl {
                None
            } else {
                Some(body.decompressed)
            }
        } else {
            None
        }
    }

    pub async fn cache_template<T: askama::Template + IntoResponse>(
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
            self.templates.insert(key, Some(CachedBody::new(bytes.clone())));
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
