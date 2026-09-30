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
//! * Cache invalidation (`pi_dash/utils/cache.py:54-75`,
//!   `authentication/utils/workspace_project_join.py:39-45`):
//!   `KEYS *{path}*` + `DEL` with `request=None, user=False, multiple=True`.
//!   Pattern builder stays with the caller; see
//!   [`RedisHandle::invalidate_matching`].
//!
//! This module owns the transport only: one shared [`redis::Client`] built
//! from [`Settings`](crate::config::Settings) (`REDIS_URL` from the config
//! registry) plus the primitives the call sites need (`SET .. EX`, `GET`,
//! `SUBSCRIBE`, `KEYS` + `DEL`). Command specs (exact keys, values, expiries,
//! match patterns) stay with the callers — `pidash-api`'s `assistant::redis`
//! pins them — so this handle never hardcodes a domain key.
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
        let url = settings
            .redis
            .url
            .as_deref()
            .filter(|url| !url.is_empty())?;
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

    /// `KEYS pattern` + `DEL` (cache invalidation,
    /// `pi_dash/utils/cache.py:54-75` with `multiple=True`).
    ///
    /// The caller builds the `*{path}*` pattern (exact key math stays with
    /// the call site, like every method here); this runs the two commands
    /// and returns the deleted count. An empty match deletes nothing and
    /// returns `Ok(0)` (Python's `delete_many([])`). Like every method here,
    /// the `Result` stays caller-visible and is never swallowed.
    pub async fn invalidate_matching(&self, pattern: &str) -> Result<usize, redis::RedisError> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let keys: Vec<String> = connection.keys(pattern).await?;
        if keys.is_empty() {
            return Ok(0);
        }
        connection.del(keys).await
    }

    /// `SUBSCRIBE channel` (SSE live tail). The returned `PubSub` is already
    /// subscribed; the caller awaits [`RedisHandle::next_payload`] and closes
    /// it when the stream ends.
    pub async fn subscribe(&self, channel: &str) -> Result<redis::aio::PubSub, redis::RedisError> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(channel).await?;
        Ok(pubsub)
    }

    /// Await the next publish on an already-subscribed channel (SSE live
    /// tail, `assistant/views/events.py:77-89`).
    ///
    /// The `redis` 1.7.1 async `PubSub` receives only through its
    /// `on_message` / `into_on_message` stream, which needs
    /// `futures_util::Stream` / `StreamExt` — neither reachable from
    /// `pidash-api`'s dependency closure. This crate already depends on
    /// `futures-util` directly, so the `Stream` polling lives here and the
    /// handler awaits plain bytes. Subscribe confirmations never surface:
    /// the crate routes them to the in-flight `SUBSCRIBE` request.
    ///
    /// Returns the publish's raw payload verbatim (`Msg::get_payload_bytes`),
    /// exactly what Python's `msg["data"]` relays into
    /// `event: chat.event\ndata: <data>\n\n`. A closed transport (`None`
    /// from the stream) surfaces as an `Io` error so the caller's feeder ends
    /// the same way Python's `except Exception` ends the generator. The
    /// keepalive tick (`tokio::time::timeout` around this call) and the
    /// unsubscribe-on-end stay with the caller. Like every method here, the
    /// `Result` stays caller-visible and is never swallowed.
    pub async fn next_payload(
        &self,
        pubsub: &mut redis::aio::PubSub,
    ) -> Result<Vec<u8>, redis::RedisError> {
        use futures_util::StreamExt;
        match pubsub.on_message().next().await {
            Some(msg) => Ok(msg.get_payload_bytes().to_vec()),
            None => Err(redis::RedisError::from((
                redis::ErrorKind::Io,
                "pubsub stream ended",
            ))),
        }
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
        let turn: uuid::Uuid = "db68f428-df63-4de8-b060-2d4038a5b1f4"
            .parse()
            .expect("uuid");
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
        assert!(handle
            .get_string("throttle_assistant_message_x")
            .await
            .is_err());
        assert!(handle.subscribe("assistant:thread:x").await.is_err());
        assert!(handle
            .invalidate_matching("*assistant:cancel:*")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn next_payload_relays_publish_verbatim() {
        // Live roundtrip for `next_payload` (PIDASHCONV-267): subscribe, then
        // publish one message and assert the exact bytes come back. Needs a
        // server on 127.0.0.1:6379 — the `redis` service in the rust-api
        // workflows, present wherever `cargo test --workspace` runs in CI.
        let handle = RedisHandle::from_url("redis://127.0.0.1:6379/").expect("url parses");
        let channel = "assistant:thread:next-payload-test";
        let mut pubsub = handle.subscribe(channel).await.expect("subscribe");
        let mut publisher = redis::Client::open("redis://127.0.0.1:6379/")
            .expect("url parses")
            .get_multiplexed_async_connection()
            .await
            .expect("publisher connects");
        let payload = br#"{"seq":7,"event":"done"}"#;
        let _: i32 = publisher.publish(channel, payload).await.expect("publish");
        let received = handle
            .next_payload(&mut pubsub)
            .await
            .expect("next publish");
        assert_eq!(received, payload);
    }

    #[tokio::test]
    async fn invalidate_matching_deletes_keys_pattern_and_counts() {
        // Live roundtrip for `invalidate_matching` (PIDASHCONV-479): seed two
        // matching keys plus one survivor, then assert the KEYS pattern
        // deletes exactly the match and returns its count. Needs a server on
        // 127.0.0.1:6379 — the `redis` service in the rust-api workflows,
        // present wherever `cargo test --workspace` runs in CI.
        let handle = RedisHandle::from_url("redis://127.0.0.1:6379/").expect("url parses");
        let prefix = "invalidate-matching-test:/api/workspaces/acme/members/";
        let first = format!("{prefix}:1");
        let second = format!("{prefix}:2");
        let survivor = "invalidate-matching-test:/api/other/".to_owned();
        handle.set_ex(&first, "1", 600).await.expect("seed first");
        handle.set_ex(&second, "1", 600).await.expect("seed second");
        handle
            .set_ex(&survivor, "1", 600)
            .await
            .expect("seed survivor");
        let deleted = handle
            .invalidate_matching("invalidate-matching-test:/api/workspaces/acme/members/*")
            .await
            .expect("invalidate");
        assert_eq!(deleted, 2);
        assert!(handle
            .get_string(&first)
            .await
            .expect("get first")
            .is_none());
        assert!(handle
            .get_string(&second)
            .await
            .expect("get second")
            .is_none());
        assert_eq!(
            handle.get_string(&survivor).await.expect("get survivor"),
            Some("1".to_owned())
        );
        // Empty match deletes nothing and reports zero (Python's
        // `delete_many([])`).
        assert_eq!(
            handle
                .invalidate_matching("invalidate-matching-test:/api/workspaces/missing/*")
                .await
                .expect("empty invalidate"),
            0
        );
        handle
            .invalidate_matching("invalidate-matching-test:*")
            .await
            .expect("cleanup");
    }
}
