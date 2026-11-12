#![forbid(unsafe_code)]

//! Gzip response compression, mirroring `django.middleware.gzip.GZipMiddleware`.
//!
//! Python reference: `django/middleware/gzip.py` (Django 6.0.5). Rules
//! reproduced exactly, in order:
//!
//! 1. Responses shorter than 200 bytes pass through untouched — unless
//!    streaming: Django's length gate is `not response.streaming` only,
//!    so streams skip it. The Rust-owned stream shape is
//!    `Content-Type: text/event-stream` (the chat and assistant SSE
//!    endpoints); only that media type takes the streaming path.
//! 2. Responses that already carry a `Content-Encoding` pass through.
//! 3. `Vary: Accept-Encoding` is patched — only once the first two checks
//!    pass, and even when the client does not accept gzip. Streaming
//!    responses get the patch too.
//! 4. The client must accept gzip: `Accept-Encoding` matches `\bgzip\b`.
//!    A stream whose client declines passes through untouched.
//! 5. A stream whose client accepts is wrapped in a chunk-wise gzip
//!    encoder with `Content-Length` deleted, so headers flush before
//!    the first frame. A buffered body is replaced only when the
//!    compressed form is strictly shorter, with `Content-Length`
//!    refreshed to the compressed size.
//! 6. A strong `ETag` (starting with `"`) is weakened with a `W/` prefix,
//!    per RFC 9110 §8.8.1. `Content-Encoding: gzip` is set last.
//!
//! Compression uses flate2 at the default level, matching Python
//! `zlib.compress` defaults closely enough that the only contract that
//! matters — "shorter, valid gzip" — holds either way. The streaming
//! encoder sync-flushes after every chunk while Django's
//! `compress_sequence` lets zlib hold small chunks until its window
//! fills; decoded bytes are identical either way (and the wire bytes
//! are non-deterministic by design — Django's `max_random_bytes`), but
//! per-chunk flushing is what forwards each SSE frame promptly.

use std::convert::Infallible;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response};
use bytes::Bytes;
use http_body::Frame;
use tower::{Layer, Service};

/// Django's "not worth compressing" floor, in bytes.
pub const MIN_COMPRESS_BYTES: usize = 200;

/// Does `value` match the regex `\bgzip\b`? Word characters are ASCII
/// alphanumerics plus underscore, like Python's `re`.
pub fn accepts_gzip(value: &str) -> bool {
    let bytes = value.as_bytes();
    let needle = b"gzip";
    if bytes.len() < needle.len() {
        return false;
    }
    fn is_word(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }
    bytes.windows(needle.len()).enumerate().any(|(i, window)| {
        window == needle
            && (i == 0 || !is_word(bytes[i - 1]))
            && (i + needle.len() == bytes.len() || !is_word(bytes[i + needle.len()]))
    })
}

/// Compress `body` with gzip at the default level. Returns `None` when the
/// compressed form is not strictly shorter, mirroring Django's
/// keep-the-original rule.
pub fn compress_if_shorter(body: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(body).ok()?;
    let compressed = encoder.finish().ok()?;
    (compressed.len() < body.len()).then_some(compressed)
}

#[derive(Debug, Clone, Default)]
pub struct GzipLayer;

impl GzipLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for GzipLayer {
    type Service = GzipService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GzipService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct GzipService<S> {
    inner: S,
}

/// Django's `response.streaming` half, keyed on the one Rust-owned
/// stream shape: `StreamingHttpResponse(..., content_type=
/// "text/event-stream")`. Compares the bare media type (before any `;`
/// parameters), case-insensitively, so it cannot misfire on buffered
/// JSON/HTML bodies — and buffered SSE-shaped error bodies never carry
/// this content type either.
fn is_streaming_sse(headers: &header::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|content_type| content_type.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// Fixed 10-byte gzip header (RFC 1952 §2.3.1): magic, deflate, no
/// flags, zero mtime, no extra compression hints, OS 255 (unknown) —
/// byte-identical to what `flate2::write::GzEncoder` (and Django's
/// `GzipFile` at the default level) emits.
const GZIP_HEADER: [u8; 10] = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];

/// Chunk-wise gzip wrapper for streaming responses: Django's
/// `compress_sequence` half of `GZipMiddleware.process_response`.
///
/// Each inner data frame is fed through a raw-deflate compressor with
/// a sync flush and forwarded immediately, so headers (returned
/// without touching the body) and every frame stream instead of
/// buffering; the gzip trailer (CRC32 + ISIZE) is appended when the
/// inner body ends. Inner errors pass through untouched — a truncated
/// stream stays truncated, never a 200 with a half member.
struct StreamingGzip {
    inner: Body,
    comp: flate2::Compress,
    crc: flate2::Crc,
    header_sent: bool,
    finished: bool,
}

impl StreamingGzip {
    fn new(inner: Body) -> Self {
        Self {
            inner,
            // `false`: raw deflate; the gzip framing is written by hand
            // (the header above, the trailer in `run_finish`).
            comp: flate2::Compress::new(flate2::Compression::default(), false),
            crc: flate2::Crc::new(),
            header_sent: false,
            finished: false,
        }
    }

    /// Feed one inner frame through the compressor with a sync flush,
    /// returning every byte produced. The scratch buffer clears zlib's
    /// worst case for the input, so one pass consumes it; the loop only
    /// repeats if a flush ever leaves input behind.
    fn run_sync(&mut self, mut input: &[u8]) -> std::io::Result<Vec<u8>> {
        self.crc.update(input);
        let mut out = Vec::new();
        while !input.is_empty() {
            let mut scratch = vec![0u8; input.len() + (input.len() >> 4) + 64];
            let (before_in, before_out) = (self.comp.total_in(), self.comp.total_out());
            self.comp
                .compress(input, &mut scratch, flate2::FlushCompress::Sync)?;
            let consumed = (self.comp.total_in() - before_in) as usize;
            let produced = (self.comp.total_out() - before_out) as usize;
            out.extend_from_slice(&scratch[..produced]);
            input = &input[consumed.min(input.len())..];
        }
        // An empty frame yields no bytes; the poll loop keeps going.
        // (The header is only ever prepended to produced bytes, so it
        // is never stranded without a body behind it.)
        Ok(out)
    }

    /// Finish the member: remaining deflate bytes plus the gzip trailer
    /// (CRC32 of the raw input, ISIZE mod 2^32, both little-endian).
    fn run_finish(&mut self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let mut scratch = vec![0u8; 128];
            let before_out = self.comp.total_out();
            let status = self
                .comp
                .compress(&[], &mut scratch, flate2::FlushCompress::Finish)?;
            let produced = (self.comp.total_out() - before_out) as usize;
            out.extend_from_slice(&scratch[..produced]);
            if status == flate2::Status::StreamEnd {
                break;
            }
        }
        out.extend_from_slice(&self.crc.sum().to_le_bytes());
        out.extend_from_slice(&self.crc.amount().to_le_bytes());
        Ok(out)
    }

    /// Prefix the gzip header ahead of the first produced bytes.
    fn with_header(&mut self, mut chunk: Vec<u8>) -> Vec<u8> {
        if self.header_sent {
            return chunk;
        }
        self.header_sent = true;
        let mut headed = Vec::with_capacity(GZIP_HEADER.len() + chunk.len());
        headed.extend_from_slice(&GZIP_HEADER);
        headed.append(&mut chunk);
        headed
    }
}

impl http_body::Body for StreamingGzip {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        loop {
            match Pin::new(&mut this.inner).poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    this.finished = true;
                    let trailer = this.run_finish().map_err(axum::Error::new)?;
                    // An empty stream is still one valid (empty) member.
                    let chunk = this.with_header(trailer);
                    if chunk.is_empty() {
                        return Poll::Ready(None);
                    }
                    return Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk)))));
                }
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    // SSE bodies carry no trailers; pass them through
                    // rather than dropping them.
                    Err(trailers) => return Poll::Ready(Some(Ok(trailers))),
                    Ok(data) => {
                        let chunk = this.run_sync(&data).map_err(axum::Error::new)?;
                        let chunk = this.with_header(chunk);
                        if chunk.is_empty() {
                            continue;
                        }
                        return Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk)))));
                    }
                },
            }
        }
    }
}

fn weaken_etag(headers: &mut header::HeaderMap) {
    let Some(etag) = headers.get(header::ETAG) else {
        return;
    };
    let Ok(value) = etag.to_str() else {
        return;
    };
    if !value.starts_with('"') {
        return;
    }
    let weakened = format!("W/{value}");
    if let Ok(value) = HeaderValue::from_str(&weakened) {
        headers.insert(header::ETAG, value);
    }
}

impl<S> Service<Request<Body>> for GzipService<S>
where
    S: Service<Request<Body>, Response = Response<Body>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // The Accept-Encoding verdict is needed after the inner service
        // responds; stash it before the request moves.
        let accepts = req
            .headers()
            .get(header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(accepts_gzip);
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let response = inner.call(req).await?;
            let (mut parts, body) = response.into_parts();
            if parts.headers.contains_key(header::CONTENT_ENCODING) {
                return Ok(Response::from_parts(parts, body));
            }
            if is_streaming_sse(&parts.headers) {
                // Django's streaming path: no length gate (rule 1 is
                // `not response.streaming` only), Vary patched either
                // way, and the body passes through untouched or streams
                // through the chunk-wise encoder. The body is never
                // collected, so headers flush before the first frame —
                // collecting here would hang an infinite SSE stream.
                crate::middleware::append_vary(&mut parts.headers, "Accept-Encoding");
                if !accepts {
                    return Ok(Response::from_parts(parts, body));
                }
                parts.headers.remove(header::CONTENT_LENGTH);
                weaken_etag(&mut parts.headers);
                parts
                    .headers
                    .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
                return Ok(Response::from_parts(
                    parts,
                    Body::new(StreamingGzip::new(body)),
                ));
            }
            // A truncated upstream body fails closed (502), never an empty
            // 200 — Django raises out of `response.content`.
            let bytes = match http_body_util::BodyExt::collect(body).await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => return Ok(crate::middleware::truncated_body()),
            };
            if bytes.len() < MIN_COMPRESS_BYTES {
                return Ok(Response::from_parts(parts, Body::from(bytes)));
            }
            // Vary is patched only after the length and encoding checks,
            // and even when the client does not accept gzip.
            crate::middleware::append_vary(&mut parts.headers, "Accept-Encoding");
            if !accepts {
                return Ok(Response::from_parts(parts, Body::from(bytes)));
            }
            let Some(compressed) = compress_if_shorter(&bytes) else {
                return Ok(Response::from_parts(parts, Body::from(bytes)));
            };
            weaken_etag(&mut parts.headers);
            parts
                .headers
                .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            parts
                .headers
                .insert(header::CONTENT_LENGTH, HeaderValue::from(compressed.len()));
            Ok(Response::from_parts(parts, Body::from(compressed)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;
    use tower::{Layer, ServiceExt};

    /// Concrete inner service (see cors tests for why `Router`).
    fn body_service(body: Vec<u8>) -> axum::Router {
        axum::Router::new().route(
            "/api/x/",
            axum::routing::any(move || {
                let body = body.clone();
                async move { Body::from(body) }
            }),
        )
    }

    fn etag_service(etag: &'static str, body: Vec<u8>) -> axum::Router {
        axum::Router::new().route(
            "/api/x/",
            axum::routing::any(move || {
                let body = body.clone();
                async move { ([(header::ETAG, etag)], Body::from(body)) }
            }),
        )
    }

    fn get(accept_encoding: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(Method::GET).uri("/api/x/");
        if let Some(encodings) = accept_encoding {
            builder = builder.header(header::ACCEPT_ENCODING, encodings);
        }
        builder.body(Body::empty()).expect("request")
    }

    async fn response_body(response: Response<Body>) -> (Response<Body>, Vec<u8>) {
        let (parts, body) = response.into_parts();
        let bytes = http_body_util::BodyExt::collect(body)
            .await
            .expect("body")
            .to_bytes()
            .to_vec();
        (Response::from_parts(parts, Body::empty()), bytes)
    }

    fn decode_gzip(bytes: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut out)
            .expect("valid gzip");
        out
    }

    /// Deterministic incompressible bytes (xorshift64*; compresses poorly).
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x9E3779B97F4A7C15u64;
        (0..len)
            .map(|_| {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                (state.wrapping_mul(0x2545F4914F6CDD1D) >> 56) as u8
            })
            .collect()
    }

    #[tokio::test]
    async fn compresses_large_compressible_body() {
        let original = vec![b'a'; 1000];
        let response = GzipLayer::new()
            .layer(body_service(original.clone()))
            .oneshot(get(Some("gzip, deflate")))
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );
        let vary = response
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(vary.contains("Accept-Encoding"));
        let (response, bytes) = response_body(response).await;
        assert_eq!(decode_gzip(&bytes), original);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()),
            Some(bytes.len().to_string()).as_deref(),
        );
    }

    #[tokio::test]
    async fn skips_short_body_without_vary() {
        let original = vec![b'a'; 100];
        let response = GzipLayer::new()
            .layer(body_service(original.clone()))
            .oneshot(get(Some("gzip")))
            .await
            .expect("serve");
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        // Django returns before patching Vary for short bodies.
        assert!(!response.headers().contains_key(header::VARY));
        let (_, bytes) = response_body(response).await;
        assert_eq!(bytes, original);
    }

    #[tokio::test]
    async fn skips_already_encoded_body_without_vary() {
        let inner = axum::Router::new().route(
            "/api/x/",
            axum::routing::any(|| async {
                (
                    [(header::CONTENT_ENCODING, "br")],
                    Body::from(vec![b'a'; 1000]),
                )
            }),
        );
        let response = GzipLayer::new()
            .layer(inner)
            .oneshot(get(Some("gzip")))
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("br")
        );
        assert!(!response.headers().contains_key(header::VARY));
    }

    #[tokio::test]
    async fn patches_vary_even_when_client_declines() {
        let original = vec![b'a'; 1000];
        let response = GzipLayer::new()
            .layer(body_service(original.clone()))
            .oneshot(get(None))
            .await
            .expect("serve");
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        let vary = response
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(vary.contains("Accept-Encoding"));
        let (_, bytes) = response_body(response).await;
        assert_eq!(bytes, original);
    }

    #[tokio::test]
    async fn keeps_body_when_compression_does_not_pay() {
        let original = noise(1000);
        // Sanity: the fixture really is incompressible.
        assert!(compress_if_shorter(&original).is_none());
        let response = GzipLayer::new()
            .layer(body_service(original.clone()))
            .oneshot(get(Some("gzip")))
            .await
            .expect("serve");
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        let (_, bytes) = response_body(response).await;
        assert_eq!(bytes, original);
    }

    #[tokio::test]
    async fn weakens_strong_etag_only() {
        let response = GzipLayer::new()
            .layer(etag_service("\"abc123\"", vec![b'a'; 1000]))
            .oneshot(get(Some("gzip")))
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );
        assert_eq!(
            response
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("W/\"abc123\"")
        );
        // A weak ETag passes through unchanged.
        let response = GzipLayer::new()
            .layer(etag_service("W/\"abc123\"", vec![b'a'; 1000]))
            .oneshot(get(Some("gzip")))
            .await
            .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::ETAG)
                .and_then(|v| v.to_str().ok()),
            Some("W/\"abc123\"")
        );
    }

    /// Inner service answering one infinite SSE stream with the
    /// production transport (`sse_channel`): the response — and thus its
    /// headers — is available immediately while the body stays open, the
    /// shape `GzipService::call` must never collect.
    fn infinite_sse_service(
        body: Body,
    ) -> tower::util::BoxCloneService<Request<Body>, Response<Body>, Infallible> {
        let cell = std::sync::Arc::new(std::sync::Mutex::new(Some(body)));
        tower::util::BoxCloneService::new(tower::service_fn(move |_request: Request<Body>| {
            let cell = cell.clone();
            async move {
                let body = cell
                    .lock()
                    .expect("sse body lock")
                    .take()
                    .expect("single request");
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(body)
                        .expect("sse response builds"),
                )
            }
        }))
    }

    #[tokio::test]
    async fn streams_sse_chunkwise_when_client_accepts_gzip() {
        use std::time::Duration;

        let (mut sender, body) = crate::sse_body::sse_channel();
        // Headers flush while the stream stays open with zero frames
        // sent — the old collect-everything path hung here forever.
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            GzipLayer::new()
                .layer(infinite_sse_service(body))
                .oneshot(get(Some("gzip"))),
        )
        .await
        .expect("headers flush while the stream stays open")
        .expect("serve");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );
        let vary = response
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(vary.contains("Accept-Encoding"));
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
        // The first chunk arrives while the stream is still open.
        let frame = b"event: chat.event\nid: 1\ndata: {}\n\n";
        sender
            .send_data(Bytes::from_static(frame))
            .await
            .expect("receiver lives");
        let mut body = response.into_body();
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .expect("first chunk arrives")
        .expect("frame")
        .expect("ok")
        .into_data()
        .expect("data frame");
        assert!(!first.is_empty());
        // Once the stream ends (sender drop = feeder done), the whole
        // member — first chunk plus trailer — decodes to the frames.
        drop(sender);
        let rest = http_body_util::BodyExt::collect(body)
            .await
            .expect("rest")
            .to_bytes();
        let mut wire = first.to_vec();
        wire.extend_from_slice(&rest);
        assert_eq!(decode_gzip(&wire), frame);
    }

    #[tokio::test]
    async fn passes_sse_through_untouched_when_client_declines_gzip() {
        use std::time::Duration;

        let (mut sender, body) = crate::sse_body::sse_channel();
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            GzipLayer::new()
                .layer(infinite_sse_service(body))
                .oneshot(get(None)),
        )
        .await
        .expect("headers flush while the stream stays open")
        .expect("serve");
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        // Vary is patched even though the client declines (rule 3).
        let vary = response
            .headers()
            .get(header::VARY)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(vary.contains("Accept-Encoding"));
        // Frames pass through byte-identical, stream still open.
        let frame = b": heartbeat\n\n";
        sender
            .send_data(Bytes::from_static(frame))
            .await
            .expect("receiver lives");
        let mut body = response.into_body();
        let first = tokio::time::timeout(
            Duration::from_secs(5),
            http_body_util::BodyExt::frame(&mut body),
        )
        .await
        .expect("first chunk arrives")
        .expect("frame")
        .expect("ok")
        .into_data()
        .expect("data frame");
        assert_eq!(&first[..], frame);
    }

    #[test]
    fn accept_encoding_matches_word_boundaries() {
        assert!(accepts_gzip("gzip"));
        assert!(accepts_gzip("gzip, deflate"));
        assert!(accepts_gzip("deflate, x-gzip"));
        assert!(!accepts_gzip(""));
        assert!(!accepts_gzip("gzipped"));
        assert!(!accepts_gzip("xgzip"));
        // Python `re` without IGNORECASE: uppercase does not match.
        assert!(!accepts_gzip("GZIP"));
    }
}
