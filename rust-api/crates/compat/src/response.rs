//! DRF response envelopes (OVL-04 part 2, PIDASHCONV-1542).
//!
//! Every `api` module kept private copies of these builders (65
//! `json_response` alone, differing mostly in the `expect` literal).
//! Each wire shape below has exactly one canonical implementation;
//! OSS modules import it (sometimes under their old local name: `raw`,
//! `json_status`, `json_body`, `status_response`, `raw_json_response`).
//!
//! Shapes that exist in only one module stay there (oauth text-500,
//! the `Vary: Cookie` json_ok, the `FieldErrors` 400, the page-detail
//! fetcher); non-axum lookalikes stay too (services `ErrorBody`,
//! assistant/types/jobs `HttpResponse`, the GitLab provider types).

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;

/// The 500 JSON body, byte-identical in all 55 OSS copies.
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Render pre-serialized JSON `body` with an explicit status.
/// `body` is `impl Into<String>` so the `String`, `&str` and
/// `&'static str` copies all collapse onto this one function.
pub fn json_response(status: StatusCode, body: impl Into<String>) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.into()))
        .expect("json response")
}

/// Serialize `value` to compact JSON and render it with an explicit
/// status. Takes `T` by value: `Value`, `&Value` and `&T` all
/// implement `Serialize`, so every OSS spelling collapses here.
pub fn json_value<T: serde::Serialize>(status: StatusCode, value: T) -> Response {
    json_response(
        status,
        serde_json::to_string(&value).expect("serializable response"),
    )
}

/// Render pre-serialized JSON `body` with 200.
pub fn json_ok(body: impl Into<String>) -> Response {
    json_response(StatusCode::OK, body)
}

/// Serialize `value` to compact JSON and render it with 200.
pub fn json_ok_value<T: serde::Serialize>(value: T) -> Response {
    json_value(StatusCode::OK, value)
}

/// Render pre-serialized JSON `body` with 201.
pub fn json_created(body: impl Into<String>) -> Response {
    json_response(StatusCode::CREATED, body)
}

/// The stock JSON 500 all OSS modules render.
pub fn server_error() -> Response {
    json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
}

/// Empty body with an explicit status and the JSON content type.
pub fn empty_response(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::empty())
        .expect("empty response")
}

/// Empty body with an explicit status and NO content-type header.
/// Three modules (plus three empty-500s and the 204s) render this
/// shape instead of [`empty_response`]; the header difference is wire
/// bytes, so the shapes stay separate functions.
pub fn empty_response_no_content_type(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .body(axum::body::Body::empty())
        .expect("empty response")
}

/// Empty 204 with no content-type header.
pub fn no_content() -> Response {
    empty_response_no_content_type(StatusCode::NO_CONTENT)
}

/// The `{"error":...}` JSON string (moved from `device_flow.rs`; the
/// `runner_enroll` copy built the same bytes via `json!`).
pub fn error_body(message: &str) -> String {
    format!(
        "{{\"error\":{}}}",
        serde_json::to_string(message).expect("error message serializes")
    )
}

/// Escape U+2028/U+2029 in an already-serialized JSON body (the
/// `<script>`-breaking line terminators JSON allows verbatim).
/// Canonicalizes 8 copies (6 inline in `v1_work_items`, `escape_u2028`,
/// `escape_json_text`).
pub fn escape_u2028(body: &str) -> String {
    body.replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// 302 with a `Location` and no content-type (the `gitlab`/`google`
/// redirect core).
pub fn redirect_found(location: HeaderValue) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// 302 with a `Location` and the Django HTML content-type (the
/// `github_app`/`github` redirect core).
pub fn redirect_found_html(location: HeaderValue) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("redirect response")
}

/// 302 with a `Location`, the Django HTML content-type, `Vary: Cookie`
/// and `Set-Cookie` headers (the session/admin redirect core).
pub fn redirect_found_cookies(location: HeaderValue, set_cookies: Vec<String>) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::VARY, "Cookie");
    for cookie in set_cookies {
        builder = builder.header(header::SET_COOKIE, cookie);
    }
    builder
        .body(axum::body::Body::empty())
        .expect("static redirect")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    async fn body_text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        String::from_utf8(bytes.to_vec()).expect("utf8 body")
    }

    fn content_type(response: &Response) -> Option<&str> {
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
    }

    #[tokio::test]
    async fn json_response_renders_status_ct_and_verbatim_body() {
        for (status, body) in [
            (StatusCode::OK, "{\"a\":1}".to_owned()),
            (StatusCode::CREATED, "[1,2]".to_owned()),
            (StatusCode::BAD_REQUEST, String::new()),
        ] {
            let response = json_response(status, body.clone());
            assert_eq!(response.status(), status);
            assert_eq!(content_type(&response), Some("application/json"));
            assert_eq!(body_text(response).await, body);
        }
        // The &str / &'static str copies share the generic.
        let response = json_response(StatusCode::OK, "{\"b\":2}");
        assert_eq!(body_text(response).await, "{\"b\":2}");
    }

    #[tokio::test]
    async fn tuple_form_matches_builder_bytes() {
        // Four modules built the envelope as a tuple; they now call
        // json_response. The wire bytes are identical either way.
        let via_tuple = (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            "{\"a\":1}".to_owned(),
        )
            .into_response();
        let via_canonical = json_response(StatusCode::OK, "{\"a\":1}");
        assert_eq!(via_tuple.status(), via_canonical.status());
        assert_eq!(
            via_tuple.headers().get(header::CONTENT_TYPE),
            via_canonical.headers().get(header::CONTENT_TYPE)
        );
        assert_eq!(body_text(via_tuple).await, body_text(via_canonical).await);
    }

    #[tokio::test]
    async fn serialize_generics_accept_every_spelling() {
        let value = serde_json::json!({"a": [1, "x"]});
        let expected = "{\"a\":[1,\"x\"]}";
        for response in [
            json_value(StatusCode::OK, value.clone()),
            json_value(StatusCode::OK, &value),
            json_ok_value(value.clone()),
            json_ok_value(&value),
        ] {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(content_type(&response), Some("application/json"));
            assert_eq!(body_text(response).await, expected);
        }
        #[derive(serde::Serialize)]
        struct Row {
            id: u32,
        }
        let response = json_ok_value(&Row { id: 7 });
        assert_eq!(body_text(response).await, "{\"id\":7}");
        let ok = json_ok("{\"a\":1}".to_owned());
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(body_text(ok).await, "{\"a\":1}");
        let created = json_created("{\"a\":1}".to_owned());
        assert_eq!(created.status(), StatusCode::CREATED);
        assert_eq!(content_type(&created), Some("application/json"));
        assert_eq!(body_text(created).await, "{\"a\":1}");
    }

    #[tokio::test]
    async fn server_error_renders_stock_body() {
        let response = server_error();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(content_type(&response), Some("application/json"));
        assert_eq!(body_text(response).await, SERVER_ERROR_BODY);
        assert_eq!(
            SERVER_ERROR_BODY,
            "{\"error\":\"Something went wrong please try again later\"}"
        );
    }

    #[tokio::test]
    async fn empty_shapes_differ_only_by_content_type() {
        let with_ct = empty_response(StatusCode::NO_CONTENT);
        assert_eq!(with_ct.status(), StatusCode::NO_CONTENT);
        assert_eq!(content_type(&with_ct), Some("application/json"));
        assert_eq!(body_text(with_ct).await, "");

        let bare = empty_response_no_content_type(StatusCode::NO_CONTENT);
        assert_eq!(bare.status(), StatusCode::NO_CONTENT);
        assert_eq!(content_type(&bare), None);
        assert_eq!(body_text(bare).await, "");

        let nc = no_content();
        assert_eq!(nc.status(), StatusCode::NO_CONTENT);
        assert_eq!(content_type(&nc), None);
        assert_eq!(body_text(nc).await, "");

        // `StatusCode::into_response` (the space/assets spelling) is the
        // same 204 the canonical renders.
        let via_unit = StatusCode::NO_CONTENT.into_response();
        assert_eq!(via_unit.status(), StatusCode::NO_CONTENT);
        assert_eq!(content_type(&via_unit), None);

        // Same for the empty-500 spelling one oauth module used.
        let bare_500 = empty_response_no_content_type(StatusCode::INTERNAL_SERVER_ERROR);
        let unit_500 = StatusCode::INTERNAL_SERVER_ERROR.into_response();
        assert_eq!(bare_500.status(), unit_500.status());
        assert_eq!(content_type(&bare_500), content_type(&unit_500));
        assert_eq!(body_text(bare_500).await, body_text(unit_500).await);
    }

    #[test]
    fn error_body_and_escape_match_copies() {
        assert_eq!(error_body("boom"), "{\"error\":\"boom\"}");
        assert_eq!(error_body("a\"b"), "{\"error\":\"a\\\"b\"}");
        // The enroll copy built the same string via json!.
        assert_eq!(
            error_body("x"),
            serde_json::json!({"error": "x"}).to_string()
        );
        assert_eq!(escape_u2028("a\u{2028}b\u{2029}c"), "a\\u2028b\\u2029c");
        assert_eq!(escape_u2028("plain"), "plain");
    }

    #[tokio::test]
    async fn redirect_cores_render_location_shapes() {
        let location = HeaderValue::from_static("https://x.example/cb?y=1");

        let bare = redirect_found(location.clone());
        assert_eq!(bare.status(), StatusCode::FOUND);
        assert_eq!(bare.headers().get(header::LOCATION), Some(&location));
        assert_eq!(content_type(&bare), None);
        assert_eq!(body_text(bare).await, "");

        let html = redirect_found_html(location.clone());
        assert_eq!(html.status(), StatusCode::FOUND);
        assert_eq!(html.headers().get(header::LOCATION), Some(&location));
        assert_eq!(content_type(&html), Some("text/html; charset=utf-8"));

        let cookies = redirect_found_cookies(
            location.clone(),
            vec!["a=1; Path=/".to_owned(), "b=2; Path=/".to_owned()],
        );
        assert_eq!(cookies.status(), StatusCode::FOUND);
        assert_eq!(cookies.headers().get(header::LOCATION), Some(&location));
        assert_eq!(content_type(&cookies), Some("text/html; charset=utf-8"));
        assert_eq!(
            cookies
                .headers()
                .get(header::VARY)
                .and_then(|v| v.to_str().ok()),
            Some("Cookie")
        );
        let set_cookies: Vec<_> = cookies
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .collect();
        assert_eq!(set_cookies.len(), 2);
    }
}
