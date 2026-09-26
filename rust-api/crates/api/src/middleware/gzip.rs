#![forbid(unsafe_code)]

//! Gzip response compression, mirroring `django.middleware.gzip.GZipMiddleware`.
//!
//! Python reference: `django/middleware/gzip.py` (Django 6.0.5). Rules
//! reproduced exactly, in order:
//!
//! 1. Responses shorter than 200 bytes pass through untouched. (Django also
//!    compresses streaming responses; every response here is a buffered
//!    body, so the short-body rule is the only length gate.)
//! 2. Responses that already carry a `Content-Encoding` pass through.
//! 3. `Vary: Accept-Encoding` is patched — only once the first two checks
//!    pass, and even when the client does not accept gzip.
//! 4. The client must accept gzip: `Accept-Encoding` matches `\bgzip\b`.
//! 5. The compressed bytes replace the body only when strictly shorter;
//!    `Content-Length` is refreshed to the compressed size.
//! 6. A strong `ETag` (starting with `"`) is weakened with a `W/` prefix,
//!    per RFC 9110 §8.8.1. `Content-Encoding: gzip` is set last.
//!
//! Compression uses flate2 at the default level, matching Python
//! `zlib.compress` defaults closely enough that the only contract that
//! matters — "shorter, valid gzip" — holds either way.
//!
//! Known limitation: bodies are buffered fully before the length check,
//! like Django's non-streaming path. Django compresses *streaming*
//! responses in chunks (`compress_sequence`); this layer buffers those too.
//! In practice proxied responses already carry Django's own
//! `Content-Encoding` when compressed and pass through via rule 2, and
//! Rust-owned routes return small buffered bodies — but a future large
//! streaming download owned by Rust wants chunk-wise compression instead.

use std::convert::Infallible;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response};
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
