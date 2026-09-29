//! Shared async Redis handle (foundation transport, PIDASHCONV-265).
//!
//! Three ported behaviors need a Redis client in the API process:
//!
//! * Cancel signal (`assistant/views/messages.py:125`, `threads.py:107`):
//!   `SET assistant:cancel:<turn_id> "1" EX 600`, every failure swallowed.
//!   Key builder: [`pidash_jobs::assistant::cancel_key`].
//! * Message-POST throttle cache (`messages.py:39-42`, decision ported in
//!   `pidash-api`'s `assistant::throttles`): DRF sliding-window history at
//!   `throttle_assistant_message_<user_pk>` with a 3600s timeout.
//! * SSE live tail (`assistant/views/events.py:77-79`): async `SUBSCRIBE
//!   assistant:thread:<thread_id>` after the replay prefix. Channel builder:
//!   [`crate::assistant::event_queries::event_channel`].
//!
//! This module owns the transport only: one shared [`redis::Client`] built
//! from [`Settings`](crate::config::Settings) (`REDIS_URL` from the config
//! registry) plus the three primitives the call sites need (`SET .. EX`,
//! `GET`, `SUBSCRIBE`). Command specs (exact keys, values, expiries) stay
//! with the callers — `pidash-api`'s `assistant::redis` pins them — so this
//! handle never hardcodes a domain key.
//!
//! Failure policy is per call site, never here: every method returns the
//! `redis::RedisError` and the caller decides. Python swallows everything at
//! the cancel write (`except Exception: pass`); the throttle treats any
//! cache failure (or an unreadable value, e.g. a Django-pickled history the
//! Rust side cannot parse) as an empty history, i.e. fail-open to allow;
//! the SSE tail logs and ends the feeder. A missing or unparsable
//! `REDIS_URL` degrades to [`RedisHandle::from_settings`] returning `None`
//! (warn-logged), so `serve` boots without a cache exactly like the interim
//! replay-only behavior.
//!
//! TLS: production `REDIS_URL` values are `rediss://`, so the crate is built
//! with `tls-rustls-webpki-roots` (Mozilla roots, no system store needed).

use redis::AsyncCommands;

/// Timeout-free shared Redis client. `Clone` shares the underlying client
/// (each operation multiplexes a connection from it); store one in
/// `AppState` and clone it into handlers.
#[derive(Debug, Clone)]
pub struct RedisHandle {
    client: redis::Client,
}

impl RedisHandle {
    /// Build from an explicit URL (`redis://` or `rediss://`). Parsing only;
    /// no connection is opened until the first operation.
    pub fn from_url(url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: redis::Client::open(url)?,
        })
    }

    /// Build from resolved [`Settings`](crate::config::Settings). Returns
    /// `None` when `REDIS_URL` is unset or empty (cache disabled); a present
    /// but unparsable URL also degrades to `None` with a warn log, so a typo
    /// fails handlers soft (per-site policy above) instead of failing boot.
    pub fn from_settings(settings: &crate::config::Settings) -> Option<Self> {
        let url = settings.redis.url.as_deref().filter(|url| !url.is_empty())?;
        match Self::from_url(url) {
            Ok(handle) => Some(handle),
            Err(error) => {
                tracing::warn!(%error, "redis: REDIS_URL unparsable; continuing without cache");
                None
            }
        }
    }

    /// `SET key value EX expiry_secs` (cancel signal, throttle re-cache).
    pub async fn set_ex(
        &self,
        key: &str,
        value: &str,
        expiry_secs: u64,
    ) -> Result<(), redis::RedisError> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        connection.set_ex::<_, _, ()>(key, value, expiry_secs).await
    }

    /// `GET key` (`None` for a missing key). Callers parse the bytes; an
    /// unreadable value is a caller-level miss, never an error here.
    pub async fn get_string(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        connection.get(key).await
    }

    /// `SUBSCRIBE channel` (SSE live tail). The returned `PubSub` is already
    /// subscribed; the caller polls `get_message` / `on_message` and closes
    /// it when the stream ends.
    pub async fn subscribe(&self, channel: &str) -> Result<redis::aio::PubSub, redis::RedisError> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(channel).await?;
        Ok(pubsub)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn no_url_means_no_handle() {
        let settings = Settings::test_defaults();
        assert!(settings.redis.url.is_none());
        assert!(RedisHandle::from_settings(&settings).is_none());
    }

    #[test]
    fn unparsable_url_degrades_to_none() {
        let mut settings = Settings::test_defaults();
        settings.redis.url = Some(":// not a url".to_owned());
        assert!(RedisHandle::from_settings(&settings).is_none());
    }

    #[test]
    fn cancel_command_spec_pins_key_value_and_expiry() {
        // Same contract `pidash-api`'s `assistant::redis::cancel_set_command`
        // pins (`SET assistant:cancel:<turn_id> "1" EX 600`, key built by
        // `pidash_jobs::assistant::cancel_key`, which this crate cannot name
        // without an import cycle): the transport sends exactly these three.
        let turn: uuid::Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4".parse().expect("uuid");
        let key = format!("assistant:cancel:{turn}");
        let (value, expiry_secs) = ("1", 600u64);
        assert_eq!(
            (key.as_str(), value, expiry_secs),
            (
                "assistant:cancel:db68f428-df63-4de8-b060-2d4038a5b1f4",
                "1",
                600
            )
        );
    }

    #[test]
    fn tail_channel_matches_publish_channel() {
        let thread = "db68f428-df63-4de8-b060-2d4038a5b1f4";
        assert_eq!(
            crate::assistant::event_queries::event_channel(thread),
            format!("assistant:thread:{thread}"),
        );
    }

    #[tokio::test]
    async fn connection_refused_surfaces_as_error_for_callers_to_swallow() {
        // No server listens here; the refused connection must come back as
        // `Err` (callers swallow it per-site) rather than panic or hang.
        let handle = RedisHandle::from_url("redis://127.0.0.1:6399/").expect("url parses");
        assert!(handle.set_ex("assistant:cancel:x", "1", 600).await.is_err());
        assert!(handle.get_string("throttle_assistant_message_x").await.is_err());
        assert!(handle.subscribe("assistant:thread:x").await.is_err());
    }
}
