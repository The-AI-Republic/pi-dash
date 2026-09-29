//! Infinite SSE response bodies over `http_body_util::channel` (PIDASHCONV-265).
//!
//! Port of the `StreamingHttpResponse(stream(), ...)` half of
//! `apps/api/pi_dash/assistant/views/events.py:50-116`: the handler renders
//! the replay prefix, then feeds the live tail (each Redis publish relayed
//! verbatim as `event: chat.event\ndata: <data>\n\n`) plus a `: keepalive`
//! frame per idle second through the sender half returned here. Dropping the
//! sender (or the request's receiver going away) ends the stream, matching
//! the `finally: unsubscribe` path in Python.
//!
//! The handler sets the exact SSE headers the events view sets
//! (`text/event-stream`, `Cache-Control: no-cache`, `X-Accel-Buffering: no`,
//! `Content-Encoding: identity`) on the response; this module owns the
//! infinite transport only, not the headers.

use std::convert::Infallible;

use axum::body::Body;
use bytes::Bytes;
use http_body_util::channel::{Channel, Sender};

/// One feeder half plus the infinite body to hand to the response: send
/// each SSE frame's bytes with [`Sender::send_data`]; the stream ends when
/// the sender is dropped or the client goes away.
pub fn sse_channel() -> (Sender<Bytes, Infallible>, Body) {
    // Buffer one frame: the feeder produces at most one frame per wakeup
    // (a publish or a keepalive tick), so backpressure never builds past it.
    let (sender, channel) = Channel::new(1);
    // `Channel<Bytes, Infallible>` is already an `http_body::Body`, so
    // `Body::new` boxes it directly (no Stream bridge, no extra traits).
    (sender, Body::new(channel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn channel_body_streams_sent_frames_in_order() {
        let (mut sender, body) = sse_channel();
        // The feeder runs concurrently with the receiver in production (and
        // the channel buffers a single frame), so the sends must not run to
        // completion before the first poll: two sequential `send_data` calls
        // against a full buffer would wait for a drain that never starts.
        let feeder = tokio::spawn(async move {
            sender
                .send_data(Bytes::from_static(b"event: chat.event\ndata: {}\n\n"))
                .await
                .expect("receiver lives");
            sender
                .send_data(Bytes::from_static(b": keepalive\n\n"))
                .await
                .expect("receiver lives");
        });
        let collected = body
            .collect()
            .await
            .expect("infinite body collects")
            .to_bytes();
        feeder.await.expect("feeder sends both frames");
        assert_eq!(
            collected,
            Bytes::from_static(b"event: chat.event\ndata: {}\n\n: keepalive\n\n"),
        );
    }
}
