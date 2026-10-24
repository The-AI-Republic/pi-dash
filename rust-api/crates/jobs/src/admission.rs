#![forbid(unsafe_code)]

//! Redis-backed [`AdmissionCache`] for worker-owned dispatch (PIDASHCONV-1487).
//!
//! Ports the cache half of `_take` / `_consume`
//! (`apps/api/pi_dash/cloud_agent/admission.py:22-58`): the fixed
//! one-minute buckets live in the shared Redis, so every fire path
//! (ticker, scheduler, seed, handoff, tool transitions) counts against
//! the same per-user / per-workspace buckets as the HTTP plane and the
//! Django worker. A per-call in-memory map cannot serve here: each call
//! would start from an empty bucket and over-admit, sequentially as
//! well as under concurrent load.
//!
//! Key form is Django's `cache_key` (`:1:{key}` —
//! [`cache_key`](pidash_services::ops::boot::cache_key), the default
//! `KEY_FUNCTION` rendering with no `KEY_PREFIX` / `VERSION` overrides),
//! so Rust and Django read and write the same bucket. Each call opens a
//! short sync connection (the trait is sync; only cloud admission paths
//! touch it). An unset, empty, or unparsable `REDIS_URL` — or any
//! connection or command failure — surfaces as `Err`, and `take_bucket`
//! maps that to `admission_unavailable`: admission fails closed exactly
//! like Python's backend failure (`admission.py:44-51`). The consume
//! step stays deferred to post-commit by the caller
//! (`cache.add(key, 1, timeout) or cache.incr(key)`, `:22-29`).

use pidash_services::dispatch::admission::AdmissionCache;
use pidash_services::ops::boot::cache_key;

/// `REDIS_URL` env name (config registry; `redis.py` reads
/// `settings.REDIS_URL` — the `redis_origin.rs` position).
const REDIS_URL_VAR: &str = "REDIS_URL";

/// Shared-Redis [`AdmissionCache`]: the worker-side twin of the api
/// crate's `RedisAdmissionCache` (a jobs-side type because jobs cannot
/// depend on api — crate graph).
#[derive(Clone, Debug)]
pub struct RedisAdmissionCache {
    client: Option<redis::Client>,
}

fn cache_unavailable<T>(what: &'static str) -> Result<T, redis::RedisError> {
    Err(redis::RedisError::from((
        redis::ErrorKind::Io,
        what,
        "redis unavailable".to_owned(),
    )))
}

impl RedisAdmissionCache {
    /// Build from `REDIS_URL`: `None` client when it is unset, empty,
    /// or unparsable, which fails every bucket read closed.
    pub fn from_env() -> Self {
        let client = std::env::var(REDIS_URL_VAR)
            .ok()
            .filter(|url| !url.is_empty())
            .and_then(|url| redis::Client::open(url.as_str()).ok());
        RedisAdmissionCache { client }
    }
}

impl AdmissionCache for RedisAdmissionCache {
    type Error = redis::RedisError;

    fn bucket_count(&self, key: &str) -> Result<Option<i64>, Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("bucket_count");
        };
        let mut connection = client.get_connection()?;
        let value: Option<i64> = redis::cmd("GET")
            .arg(cache_key(key))
            .query(&mut connection)?;
        Ok(value)
    }

    fn add_or_incr(&self, key: &str, timeout_secs: i64) -> Result<(), Self::Error> {
        let Some(client) = &self.client else {
            return cache_unavailable("add_or_incr");
        };
        let mut connection = client.get_connection()?;
        // `cache.add(key, 1, timeout) or cache.incr(key)`: set-if-absent
        // with TTL, else increment.
        let set: Option<String> = redis::cmd("SET")
            .arg(cache_key(key))
            .arg(1)
            .arg("EX")
            .arg(timeout_secs)
            .arg("NX")
            .query(&mut connection)?;
        if set.is_none() {
            let _: i64 = redis::cmd("INCR")
                .arg(cache_key(key))
                .query(&mut connection)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_client_fails_closed() {
        let cache = RedisAdmissionCache { client: None };
        assert!(cache.bucket_count("k").is_err());
        assert!(cache.add_or_incr("k", 60).is_err());
    }

    #[test]
    fn refused_connection_fails_closed() {
        // Discard port: nothing listens, so the sync connect refuses
        // immediately with no server needed.
        let client = redis::Client::open("redis://127.0.0.1:9/").expect("url parses");
        let cache = RedisAdmissionCache {
            client: Some(client),
        };
        assert!(cache.bucket_count("k").is_err());
        assert!(cache.add_or_incr("k", 60).is_err());
    }
}
