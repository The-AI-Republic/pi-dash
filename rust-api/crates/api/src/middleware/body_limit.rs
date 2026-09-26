#![forbid(unsafe_code)]

//! Request body size cap, mirroring
//! `pi_dash.middleware.request_body_size.RequestBodySizeLimitMiddleware`.
//!
//! Python reference:
//! `apps/api/pi_dash/middleware/request_body_size.py`. The middleware reads
//! `request.body`, which raises `RequestDataTooBig` once the body exceeds
//! `DATA_UPLOAD_MAX_MEMORY_SIZE` (`FILE_SIZE_LIMIT`, default `5242880`),
//! and answers that failure with a `413` JSON body. The exact bytes below
//! were rendered from Django's own `JsonResponse` (default `', '` / `': '`
//! separators — `serde_json`'s compact form would differ, so the bytes are
//! hardcoded and pinned by test).
//!
//! The limit is `<=`-shaped on both sides: a body of exactly the limit
//! passes, `limit + 1` is rejected — same as Django, which only raises once
//! the buffered body grows past the maximum.

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use tower::{Layer, Service};

/// `FILE_SIZE_LIMIT` default, mirroring `DATA_UPLOAD_MAX_MEMORY_SIZE` in
/// `apps/api/pi_dash/settings/common.py`.
pub const DEFAULT_BODY_LIMIT_BYTES: u64 = 5_242_880;

/// Exact bytes Django renders for the 413 rejection (verified against
/// `JsonResponse` on Django 6.0.5 — do not "tidy" the spacing).
pub const BODY_TOO_LARGE_JSON: &[u8] = b"{\"error\": \"REQUEST_BODY_TOO_LARGE\", \"detail\": \"The size of the request body exceeds the maximum allowed size.\"}";

/// True when a `Limited` read failed because the body exceeded the cap
/// (as opposed to an underlying transport failure). `Limited` boxes both
/// shapes into `Box<dyn Error>`, so only a downcast tells them apart.
pub fn is_limit_exceeded(error: Box<dyn std::error::Error + Send + Sync>) -> bool {
    error
        .downcast_ref::<http_body_util::LengthLimitError>()
        .is_some()
}

/// Build the 413 rejection response.
pub fn body_too_large_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::PAYLOAD_TOO_LARGE)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::CONTENT_LENGTH,
            BODY_TOO_LARGE_JSON.len().to_string(),
        )
        .body(Body::from(BODY_TOO_LARGE_JSON))
        .expect("static 413 response")
}

#[derive(Debug, Clone)]
pub struct BodyLimitLayer {
    limit_bytes: u64,
}

impl BodyLimitLayer {
    /// `limit_bytes` is `Settings.file_size_limit` (`FILE_SIZE_LIMIT`).
    pub fn new(limit_bytes: u64) -> Self {
        Self { limit_bytes }
    }
}

impl<S> Layer<S> for BodyLimitLayer {
    type Service = BodyLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        BodyLimitService {
            inner,
            limit_bytes: self.limit_bytes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BodyLimitService<S> {
    inner: S,
    limit_bytes: u64,
}

impl<S> Service<Request<Body>> for BodyLimitService<S>
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
        let limit = self.limit_bytes;
        // A declared `Content-Length` past the limit is rejected without
        // reading the body; anything else is buffered up to `limit + 1`
        // bytes, exactly like Django reading `request.body` past the
        // maximum. Both shapes reject at `limit + 1`, never at `limit`.
        if req
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .is_some_and(|len| len > limit)
        {
            return Box::pin(async move { Ok(body_too_large_response()) });
        }
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let (parts, body) = req.into_parts();
            // `Limited` fails the moment the body grows past `limit`; that
            // failure IS the rejection. Any other read failure (client
            // gone, upstream truncated) fails closed with a 502 — Django
            // raises out of `request.body` rather than running the view.
            let limit_plus_one: usize = limit.saturating_add(1).try_into().unwrap_or(usize::MAX);
            let bytes = match http_body_util::BodyExt::collect(http_body_util::Limited::new(
                body,
                limit_plus_one,
            ))
            .await
            {
                Ok(collected) => collected.to_bytes(),
                Err(error) => {
                    return Ok(if is_limit_exceeded(error) {
                        body_too_large_response()
                    } else {
                        crate::middleware::truncated_body()
                    });
                }
            };
            if (bytes.len() as u64) > limit {
                return Ok(body_too_large_response());
            }
            let req = Request::from_parts(parts, Body::from(bytes));
            inner.call(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::{Layer, ServiceExt};

    /// Concrete echo service (see cors tests for why `Router`): posts the
    /// request body back byte-identical, proving transparent forwarding.
    fn echo() -> axum::Router {
        axum::Router::new().route(
            "/api/x/",
            axum::routing::post(|body: axum::body::Bytes| async move { body }),
        )
    }

    async fn body_bytes(response: Response<Body>) -> Vec<u8> {
        http_body_util::BodyExt::collect(response.into_body())
            .await
            .expect("body")
            .to_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn limit_failures_classify_as_413_not_502() {
        // A real over-limit read produces LengthLimitError…
        let error = http_body_util::BodyExt::collect(http_body_util::Limited::new(
            http_body_util::Full::new(&b"0123456789"[..]),
            5,
        ))
        .await
        .expect_err("5-byte cap on 10 bytes");
        assert!(is_limit_exceeded(error));
        // …while a transport failure does not (and fails closed as 502).
        let transport: Box<dyn std::error::Error + Send + Sync> =
            Box::new(std::io::Error::other("boom"));
        assert!(!is_limit_exceeded(transport));
    }

    #[test]
    fn rejection_bytes_match_django_json_response() {
        // Rendered from Django's JsonResponse on 6.0.5 — default json
        // separators (', ', ': '), application/json, status 413.
        assert_eq!(
            BODY_TOO_LARGE_JSON,
            b"{\"error\": \"REQUEST_BODY_TOO_LARGE\", \"detail\": \"The size of the request body exceeds the maximum allowed size.\"}"
        );
        let response = body_too_large_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn body_at_exactly_the_limit_passes() {
        let response = BodyLimitLayer::new(8)
            .layer(echo())
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::from(vec![b'x'; 8]))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, vec![b'x'; 8]);
    }

    #[tokio::test]
    async fn body_one_past_the_limit_is_rejected() {
        let response = BodyLimitLayer::new(8)
            .layer(echo())
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::from(vec![b'x'; 9]))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(body_bytes(response).await, BODY_TOO_LARGE_JSON);
    }

    #[tokio::test]
    async fn declared_content_length_past_the_limit_is_rejected_unread() {
        // The echo route would answer 200 on the empty body, so the 413
        // below proves rejection happened before the inner service ran.
        let response = BodyLimitLayer::new(8)
            .layer(echo())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/x/")
                    .header(header::CONTENT_LENGTH, "100")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn small_body_forwards_byte_identical() {
        let payload = b"{\"hello\": \"world\"}".to_vec();
        let response = BodyLimitLayer::new(DEFAULT_BODY_LIMIT_BYTES)
            .layer(echo())
            .oneshot(
                Request::post("/api/x/")
                    .body(Body::from(payload.clone()))
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, payload);
    }
}
