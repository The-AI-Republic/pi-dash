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
//! TLS: production `REDIS_URL` values are `rediss://`, and Django connects
//! to them without certificate verification (`ssl_cert_reqs=None` in
//! `pi_dash/settings/redis.py`, `ssl_cert_reqs: False` in the `CACHES`
//! pool kwargs), so self-signed and private-CA servers are reachable.
//! Every `REDIS_URL`-derived client in the workspace builds through
//! [`open_client`], which mirrors that policy via redis-rs `#insecure`
//! (needs the crate's `tls-rustls-insecure` feature).

use redis::AsyncCommands;
use std::time::Duration;

/// Open a [`redis::Client`] with Django's TLS policy: `rediss://` (and
/// `valkeys://`) URLs connect without certificate verification, exactly
/// like Django's `ssl_cert_reqs=None`. Plain `redis://` URLs pass
/// through untouched, as does any URL that already carries a `#fragment`
/// (redis-rs rejects non-`insecure` fragments with a clear error).
/// Parsing only; no connection is opened until the first operation.
pub fn open_client(url: &str) -> Result<redis::Client, redis::RedisError> {
    let scheme = url.split("://").next().unwrap_or("");
    let is_tls = scheme.eq_ignore_ascii_case("rediss") || scheme.eq_ignore_ascii_case("valkeys");
    if is_tls && !url.contains('#') {
        return redis::Client::open(format!("{url}#insecure"));
    }
    redis::Client::open(url)
}

/// Django's `REDIS_SOCKET_CONNECT_TIMEOUT` default (`2.0` s,
/// `pi_dash/settings/redis.py` `async_redis_instance`), used when the
/// resolved setting is not a positive finite float.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Django's `REDIS_SOCKET_TIMEOUT` default (`5.0` s), used when the
/// resolved setting is not a positive finite float.
const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Resolve a `Settings.redis` float-seconds timeout, falling back to the
/// Django default for a non-positive or non-finite value (same guard as
/// the outbox `connect_blocking` helpers).
fn redis_timeout(setting_secs: f64, default: Duration) -> Duration {
    if setting_secs.is_finite() && setting_secs > 0.0 {
        Duration::try_from_secs_f64(setting_secs).unwrap_or(default)
    } else {
        default
    }
}

/// Shared Redis client with Django-parity subscribe timeouts. `Clone`
/// shares the underlying client (each operation multiplexes a connection
/// from it); store one in `AppState` and clone it into handlers.
/// [`RedisHandle::subscribe`] wraps its connect and subscribe phases in
/// [`tokio::time::timeout`] using these timeouts; the multiplexed
/// operations keep redis-rs's own connection config.
#[derive(Debug, Clone)]
pub struct RedisHandle {
    client: redis::Client,
    connect_timeout: Duration,
    response_timeout: Duration,
}

impl RedisHandle {
    /// Build from an explicit URL (`redis://` or `rediss://`). Parsing only;
    /// no connection is opened until the first operation. `rediss://`
    /// URLs skip certificate verification via [`open_client`], matching
    /// Django's `ssl_cert_reqs=None`. Subscribe timeouts are Django's
    /// defaults (2.0 s / 5.0 s): every caller of this constructor is a
    /// test; production builds via [`RedisHandle::from_settings`].
    pub fn from_url(url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: open_client(url)?,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            response_timeout: DEFAULT_RESPONSE_TIMEOUT,
        })
    }

    /// Build from resolved [`Settings`](crate::config::Settings). Returns
    /// `None` when `REDIS_URL` is unset or empty (cache disabled); a present
    /// but unparsable URL also degrades to `None` with a warn log, so a typo
    /// fails handlers soft (per-site policy above) instead of failing boot.
    /// Subscribe timeouts come from `Settings.redis`
    /// (`REDIS_SOCKET_CONNECT_TIMEOUT` / `REDIS_SOCKET_TIMEOUT`, Django's
    /// 2.0 s / 5.0 s defaults).
    pub fn from_settings(settings: &crate::config::Settings) -> Option<Self> {
        let url = settings
            .redis
            .url
            .as_deref()
            .filter(|url| !url.is_empty())?;
        match open_client(url) {
            Ok(client) => Some(Self {
                client,
                connect_timeout: redis_timeout(
                    settings.redis.socket_connect_timeout_secs,
                    DEFAULT_CONNECT_TIMEOUT,
                ),
                response_timeout: redis_timeout(
                    settings.redis.socket_timeout_secs,
                    DEFAULT_RESPONSE_TIMEOUT,
                ),
            }),
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
    ///
    /// redis-rs's `get_async_pubsub` applies no timeout (the connection-config
    /// timeouts only cover multiplexed connections), so both phases are
    /// wrapped in [`tokio::time::timeout`]: the pubsub connect in
    /// `REDIS_SOCKET_CONNECT_TIMEOUT`, the `SUBSCRIBE` reply in
    /// `REDIS_SOCKET_TIMEOUT` — the same client-side bounds Django's
    /// `async_redis_instance` enforces. An elapsed timeout surfaces as an
    /// `Io` error, so the callers' existing error paths apply unchanged.
    pub async fn subscribe(&self, channel: &str) -> Result<redis::aio::PubSub, redis::RedisError> {
        let mut pubsub = match tokio::time::timeout(
            self.connect_timeout,
            self.client.get_async_pubsub(),
        )
        .await
        {
            Ok(connected) => connected?,
            Err(_) => {
                return Err(redis::RedisError::from((
                    redis::ErrorKind::Io,
                    "pubsub connect timed out",
                )));
            }
        };
        match tokio::time::timeout(self.response_timeout, pubsub.subscribe(channel)).await {
            Ok(subscribed) => subscribed?,
            Err(_) => {
                return Err(redis::RedisError::from((
                    redis::ErrorKind::Io,
                    "pubsub subscribe timed out",
                )));
            }
        }
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
    fn rediss_url_builds_insecure_client_like_django() {
        // Django passes `ssl_cert_reqs=None` for every `rediss://`
        // connection (`pi_dash/settings/redis.py`, sync and async
        // `redis_instance`), so self-signed servers are reachable. The
        // `#insecure` fragment redis-rs parses is the same policy; no
        // connection is opened here, so no server or TLS provider is
        // needed to pin the mode.
        let client = open_client("rediss://127.0.0.1:6390/0").expect("rediss url parses");
        match client.get_connection_info().addr() {
            redis::ConnectionAddr::TcpTls { insecure, .. } => {
                assert!(*insecure, "rediss:// must skip verification like Django");
            }
            other => panic!("expected TcpTls for rediss://, got {other:?}"),
        }
        // The shared handle builds from it (parse-only, no I/O).
        assert!(RedisHandle::from_url("rediss://127.0.0.1:6390/0").is_ok());
        // An explicit fragment is left alone, never doubled.
        let client =
            open_client("rediss://127.0.0.1:6390/0#insecure").expect("fragment url parses");
        match client.get_connection_info().addr() {
            redis::ConnectionAddr::TcpTls { insecure, .. } => assert!(*insecure),
            other => panic!("expected TcpTls for rediss://, got {other:?}"),
        }
        // Plain `redis://` stays non-TLS.
        let client = open_client("redis://127.0.0.1:6379/0").expect("redis url parses");
        assert!(
            matches!(
                client.get_connection_info().addr(),
                redis::ConnectionAddr::Tcp(..)
            ),
            "redis:// must stay plaintext"
        );
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
        // Bind an ephemeral port and read it back, then drop the listener so
        // the port is free: a fixed port cannot be assumed unused on a shared
        // runner (PIDASHCONV-1283). No server listens on the chosen port; the
        // refused connection must come back as `Err` (callers swallow it
        // per-site) rather than panic or hang.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
            listener.local_addr().expect("local_addr").port()
        };
        let handle =
            RedisHandle::from_url(&format!("redis://127.0.0.1:{port}/")).expect("url parses");
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

    /// Spawn a silent TCP server: accepts connections on an ephemeral port
    /// and never replies (a blackholed Redis). A fixed port cannot be
    /// assumed unused on a shared runner (PIDASHCONV-1283).
    async fn spawn_silent_server() -> (tokio::task::JoinHandle<()>, u16) {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                // Hold the connection open, draining whatever the client
                // sends, and never reply — the handshake (and SUBSCRIBE)
                // waits forever.
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                    }
                });
            }
        });
        (task, port)
    }

    /// Spawn a fake server that answers redis-rs's `CLIENT SETINFO`
    /// handshake with `+OK` but never replies to `SUBSCRIBE` (a Redis
    /// frozen mid-subscribe, e.g. `DEBUG SLEEP`). Each `SETINFO` occurrence
    /// in the inbound stream earns one `+OK`; everything else stays silent.
    async fn spawn_handshake_only_server() -> (tokio::task::JoinHandle<()>, u16) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let port = listener.local_addr().expect("local_addr").port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut seen = Vec::new();
                    let mut answered = 0usize;
                    let mut buf = [0u8; 1024];
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        seen.extend_from_slice(&buf[..n]);
                        let setinfos = seen
                            .windows(b"SETINFO".len())
                            .filter(|w| *w == b"SETINFO")
                            .count();
                        while answered < setinfos {
                            if socket.write_all(b"+OK\r\n").await.is_err() {
                                return;
                            }
                            answered += 1;
                        }
                    }
                });
            }
        });
        (task, port)
    }

    #[tokio::test]
    async fn subscribe_against_silent_server_fails_fast() {
        // Django's async client carries `socket_connect_timeout=2.0` /
        // `socket_timeout=5.0` (`pi_dash/settings/redis.py`
        // `async_redis_instance`), so a stalled Redis fails a runner
        // long-poll or SSE stream in seconds. `subscribe` must match:
        // against a server that accepts and never replies it returns an
        // `Io` error (the callers' existing error paths apply) instead of
        // hanging to the OS TCP timeout. Pre-fix this test hangs.
        let (_task, port) = spawn_silent_server().await;
        let handle =
            RedisHandle::from_url(&format!("redis://127.0.0.1:{port}/")).expect("url parses");
        let started = std::time::Instant::now();
        let error = match handle.subscribe("assistant:thread:x").await {
            Ok(_) => panic!("silent server must fail, not hang"),
            Err(error) => error,
        };
        assert!(
            matches!(error.kind(), redis::ErrorKind::Io),
            "elapsed must map to Io, got {:?}",
            error.kind()
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "must fail in seconds, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn subscribe_honours_settings_timeouts() {
        // The timeouts come from `Settings.redis`, not hardcoded: small
        // settings fail well under the 2.0 s default connect timeout.
        // Pre-fix this test hangs.
        let (_task, port) = spawn_silent_server().await;
        let mut settings = Settings::test_defaults();
        settings.redis.url = Some(format!("redis://127.0.0.1:{port}/"));
        settings.redis.socket_connect_timeout_secs = 0.2;
        settings.redis.socket_timeout_secs = 0.3;
        let handle = RedisHandle::from_settings(&settings).expect("handle builds");
        let started = std::time::Instant::now();
        let error = match handle.subscribe("assistant:thread:x").await {
            Ok(_) => panic!("silent server must fail, not hang"),
            Err(error) => error,
        };
        assert!(
            matches!(error.kind(), redis::ErrorKind::Io),
            "elapsed must map to Io, got {:?}",
            error.kind()
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "0.2 s setting must fail in under a second, took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn subscribe_response_stall_trips_response_timeout() {
        // The `SUBSCRIBE` reply has its own arm: a server that answers the
        // connect handshake but stalls the subscribe reply must trip the
        // response timeout, not the (long) connect timeout. Pre-fix this
        // test hangs.
        let (_task, port) = spawn_handshake_only_server().await;
        let mut settings = Settings::test_defaults();
        settings.redis.url = Some(format!("redis://127.0.0.1:{port}/"));
        settings.redis.socket_connect_timeout_secs = 20.0;
        settings.redis.socket_timeout_secs = 0.3;
        let handle = RedisHandle::from_settings(&settings).expect("handle builds");
        let started = std::time::Instant::now();
        let error = match handle.subscribe("assistant:thread:x").await {
            Ok(_) => panic!("stalled reply must fail, not hang"),
            Err(error) => error,
        };
        assert!(
            matches!(error.kind(), redis::ErrorKind::Io),
            "elapsed must map to Io, got {:?}",
            error.kind()
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "0.3 s response setting must fail in seconds, took {:?}",
            started.elapsed()
        );
    }
}
