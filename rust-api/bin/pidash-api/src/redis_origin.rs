//! Blocking [`RedisOrigin`] for the `issue_activity` worker wiring (PIDASHCONV-836).
//!
//! `register_activity_task` takes a synchronous [`RedisOrigin`] (`set_ex`
//! returns a `Result`, it does not await), while the foundation handle
//! (`pidash_db::redis::RedisHandle`) is async-only — so the worker binary
//! owns this thin sync adapter over the same locked `redis` crate. The
//! foundation crates stay untouched.
//!
//! Semantics mirror `redis_instance()`
//! (`apps/api/pi_dash/settings/redis.py`): Django raises at call time when
//! `REDIS_URL` is unset, and `issue_activity`'s broad-except turns that
//! raise into an acked abort before any writes. Construction here never
//! fails the worker boot; a missing, empty, or unparsable URL (or any
//! connection/command error) surfaces as `Err` from `set_ex`, which
//! `run_activity` maps to the same abort (`DispatchError::Log`, acked).
//!
//! Timeouts mirror `redis_instance` (`socket_connect_timeout` 2s,
//! `socket_timeout` 5s). One TCP connection per write (no pool):
//! behavior matches redis-py's pooled client; throughput is a
//! worker-sizing note, not a behavior difference.
//!
//! [`RedisOrigin`]: pidash_jobs::tasks_webhooks::RedisOrigin

use pidash_jobs::tasks_webhooks::RedisOrigin;
use redis::Commands;

/// `socket_connect_timeout` (`settings/redis.py` default 2.0s).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
/// `socket_timeout` (`settings/redis.py` default 5.0s).
const RW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// `REDIS_URL` env name (config registry; `redis.py` reads `settings.REDIS_URL`).
const REDIS_URL_VAR: &str = "REDIS_URL";

/// `ri.set(str(issue_id), origin, ex=600)` over a blocking connection.
/// `Clone` shares the client (each `set_ex` opens its own connection);
/// `Send + Sync` for the registry handler.
#[derive(Debug, Clone)]
pub struct OriginRedis {
    client: Option<redis::Client>,
}

impl OriginRedis {
    /// Build from `REDIS_URL`, never failing the worker boot: a missing,
    /// empty, or unparsable URL degrades to an adapter whose `set_ex`
    /// always errors (warn-logged, like `RedisHandle::from_settings`),
    /// which aborts origin-bearing calls exactly like Django's
    /// `redis_instance()` `RuntimeError` under the broad-except.
    pub fn from_env() -> Self {
        let url = std::env::var(REDIS_URL_VAR)
            .ok()
            .filter(|url| !url.is_empty());
        match url {
            None => {
                tracing::warn!(
                    "redis: REDIS_URL unset; issue_activity origin writes will abort like Django's redis_instance() RuntimeError"
                );
                Self { client: None }
            }
            Some(url) => match redis::Client::open(url.as_str()) {
                Ok(client) => Self {
                    client: Some(client),
                },
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "redis: REDIS_URL unparsable; issue_activity origin writes will abort"
                    );
                    Self { client: None }
                }
            },
        }
    }

    /// The degraded adapter, without touching the environment (tests).
    #[cfg(test)]
    fn unconfigured() -> Self {
        Self { client: None }
    }
}

impl RedisOrigin for OriginRedis {
    /// `SETEX key ex_secs value`. Every failure — unconfigured URL,
    /// refused connection, timeout, command error — is `Err`, and the
    /// caller aborts the task (the broad-except), never panicking.
    fn set_ex(&self, key: &str, value: &str, ex_secs: u64) -> Result<(), String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| "REDIS_URL is not configured".to_owned())?;
        let mut connection = client
            .get_connection_with_timeout(CONNECT_TIMEOUT)
            .map_err(|error| error.to_string())?;
        connection
            .set_read_timeout(Some(RW_TIMEOUT))
            .map_err(|error| error.to_string())?;
        connection
            .set_write_timeout(Some(RW_TIMEOUT))
            .map_err(|error| error.to_string())?;
        connection
            .set_ex(key, value, ex_secs)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_origin_errors_without_io() {
        // Mirrors `redis_instance()` raising when REDIS_URL is unset: the
        // caller (`run_activity`) aborts before any writes. No server is
        // involved — the error comes back synchronously.
        let redis = OriginRedis::unconfigured();
        let error = redis
            .set_ex("some-issue-id", "https://app.example", 600)
            .expect_err("unconfigured origin must error");
        assert!(
            error.contains("REDIS_URL"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn refused_server_surfaces_as_error() {
        // A refused connection is `Err` (the caller aborts), never a panic
        // or a hang: nothing listens on 6399 (same precedent as
        // pidash-db's redis test), so this fails fast without a server.
        let client =
            redis::Client::open("redis://127.0.0.1:6399/").expect("url parses");
        let redis = OriginRedis {
            client: Some(client),
        };
        assert!(redis.set_ex("k", "v", 600).is_err());
    }

    #[test]
    fn live_setex_roundtrip() {
        // Live SETEX + GET + DEL on 127.0.0.1:6379 — the `redis` service
        // in the rust-api workflows (same precedent as pidash-db's
        // `invalidate_matching` roundtrip).
        let client =
            redis::Client::open("redis://127.0.0.1:6379/").expect("url parses");
        let redis = OriginRedis {
            client: Some(client),
        };
        let key = "pidash-836-test:issue-origin";
        redis
            .set_ex(key, "https://app.example/issues/1", 600)
            .expect("setex");
        let mut connection = redis
            .client
            .as_ref()
            .expect("client")
            .get_connection()
            .expect("readback connects");
        let stored: Option<String> = connection.get(key).expect("get");
        assert_eq!(stored.as_deref(), Some("https://app.example/issues/1"));
        let deleted: usize = connection.del(key).expect("del");
        assert_eq!(deleted, 1);
    }
}
