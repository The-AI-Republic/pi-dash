//! The OpenAPI document handler: `GET /api/schema/` (D-23).
//!
//! Ports `SpectacularAPIView` (`pi_dash/urls.py:33`) with its two renderer
//! classes: YAML by default, JSON via `?format=json`, 404 on unknown
//! formats. The document bytes come from the services-layer builder
//! ([`pidash_services::v1_openapi::doc`], PIDASHCONV-532); this module owns
//! the HTTP shell — content types, `Content-Disposition`, `Allow`/`Vary`,
//! the 301 twin, and the unsafe-method 405s.
//!
//! DRF order reproduced (`views.py:dispatch`, verified against the pinned
//! 3.15.2 source): the cutover gate, then `check_throttles`, then
//! method/format dispatch — a throttled unsafe method answers 429, not
//! 405, and a throttled `?format=xml` answers 429, not 404. The denial
//! renderer follows the requested format (JSON iff `?format=json`, else
//! the YAML default).
//!
//! `?format=` follows `QueryDict.get` (last value wins; empty counts as
//! absent — DRF's `if format:`), matched against renderer `format` attrs
//! (`yaml`/`json`, exact, case-sensitive); anything else 404s
//! (`negotiation.py:filter_renderers`, bare `Http404`).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;

use crate::state::AppState;

use super::throttle::DenialRenderer;

/// YAML denial/200 content type: the OpenAPI YAML renderer carries
/// `charset=utf-8` (`renderers.py`, `format='yaml'`).
pub const CONTENT_TYPE_YAML: &str = "application/vnd.oai.openapi; charset=utf-8";
/// JSON denial/200 content type: the OpenAPI JSON renderer sets no charset
/// (`renderers.py`, `format='json'`).
pub const CONTENT_TYPE_JSON: &str = "application/vnd.oai.openapi+json";

/// `Content-Disposition` on the YAML 200 (`_get_schema_response`:
/// `inline; filename="<TITLE>.yaml"`).
pub const DISPOSITION_YAML: &str = "inline; filename=\"The Pi Dash REST API.yaml\"";
/// `Content-Disposition` on the JSON 200 (`_get_schema_response`:
/// `inline; filename="<TITLE>.json"`).
pub const DISPOSITION_JSON: &str = "inline; filename=\"The Pi Dash REST API.json\"";

/// Exact bytes of the `?format=xml` 404 (FX-OPENAPI-05 `unknown_format`):
/// DRF's `NotFound` through the YAML renderer.
pub const UNKNOWN_FORMAT_BODY: &str = "detail:\n  string: Not found.\n  code: not_found\n";

/// Exact bytes of the unsafe-method 405s (FX-OPENAPI-05 `method_denials`,
/// `/api/schema/`): DRF's `MethodNotAllowed` through the YAML renderer.
pub const NOT_ALLOWED_BODY_POST: &str =
    "detail:\n  string: Method \"POST\" not allowed.\n  code: method_not_allowed\n";
/// Exact bytes of the unsafe-method 405s (FX-OPENAPI-05 `method_denials`,
/// `/api/schema/`): DRF's `MethodNotAllowed` through the YAML renderer.
pub const NOT_ALLOWED_BODY_PUT: &str =
    "detail:\n  string: Method \"PUT\" not allowed.\n  code: method_not_allowed\n";
/// Exact bytes of the unsafe-method 405s (FX-OPENAPI-05 `method_denials`,
/// `/api/schema/`): DRF's `MethodNotAllowed` through the YAML renderer.
pub const NOT_ALLOWED_BODY_PATCH: &str =
    "detail:\n  string: Method \"PATCH\" not allowed.\n  code: method_not_allowed\n";
/// Exact bytes of the unsafe-method 405s (FX-OPENAPI-05 `method_denials`,
/// `/api/schema/`): DRF's `MethodNotAllowed` through the YAML renderer.
pub const NOT_ALLOWED_BODY_DELETE: &str =
    "detail:\n  string: Method \"DELETE\" not allowed.\n  code: method_not_allowed\n";

/// `GET /api/schema/`: the OpenAPI document (YAML default, JSON via
/// `?format=json`, 404 on unknown formats). Gated on the cutover flip,
/// throttled before format dispatch.
pub async fn serve(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let format = last_format_value(req.uri().query().unwrap_or(""));
    let renderer = match format.as_deref() {
        Some("json") => DenialRenderer::Json,
        _ => DenialRenderer::Yaml,
    };
    let ident = super::peer_ident(&req);
    let session = super::session_handle(&req);
    if let Some(denied) = super::throttle_verdict(&state, ident, session).await {
        return super::throttled(renderer, denied.wait, true);
    }
    match format.as_deref() {
        None | Some("") | Some("yaml") => doc_response(
            CONTENT_TYPE_YAML,
            DISPOSITION_YAML,
            pidash_services::v1_openapi::doc::render_yaml().into_bytes(),
        ),
        Some("json") => doc_response(
            CONTENT_TYPE_JSON,
            DISPOSITION_JSON,
            pidash_services::v1_openapi::doc::render_json().into_bytes(),
        ),
        Some(_) => super::view_response(
            StatusCode::NOT_FOUND,
            CONTENT_TYPE_YAML,
            UNKNOWN_FORMAT_BODY.as_bytes().to_vec(),
            true,
        ),
    }
}

/// Unsafe methods on `/api/schema/`: the pinned YAML 405 (throttle-checked
/// first, so a throttled caller answers 429 instead).
pub async fn method_not_allowed(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let ident = super::peer_ident(&req);
    let session = super::session_handle(&req);
    if let Some(denied) = super::throttle_verdict(&state, ident, session).await {
        return super::throttled(DenialRenderer::Yaml, denied.wait, true);
    }
    let body = match req.method().as_str() {
        "PUT" => NOT_ALLOWED_BODY_PUT,
        "PATCH" => NOT_ALLOWED_BODY_PATCH,
        "DELETE" => NOT_ALLOWED_BODY_DELETE,
        _ => NOT_ALLOWED_BODY_POST,
    };
    super::view_response(
        StatusCode::METHOD_NOT_ALLOWED,
        CONTENT_TYPE_YAML,
        body.as_bytes().to_vec(),
        true,
    )
}

/// `GET /api/schema` (any method): the APPEND_SLASH 301 to the canonical
/// URL (`CommonMiddleware.process_response`, query string preserved).
/// Served outside throttling like Django's middleware redirect, and gated
/// on the cutover flip (unflipped, Django 301s — or 404s when its own flag
/// is off — through the proxy).
///
/// Ported from production (`DEBUG=False`) behaviour: all methods 301. In
/// `DEBUG` Django raises `RuntimeError` for POST/PUT/PATCH instead (the
/// slash-guard in `get_full_path_with_slash`); that dev-only guard is not
/// ported.
pub async fn slashless_redirect(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let mut location = super::SCHEMA_PATH.to_owned();
    if let Some(query) = req.uri().query() {
        location.push('?');
        location.push_str(query);
    }
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(header::LOCATION, location)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::empty())
        .expect("static redirect response")
}

/// One doc 200: builder bytes with the YAML/JSON content type, the
/// `Content-Disposition` download hint, and the `Allow`/`Vary` shell.
fn doc_response(content_type: &'static str, disposition: &'static str, body: Vec<u8>) -> Response {
    let mut response = super::view_response(StatusCode::OK, content_type, body, true);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(disposition),
    );
    response
}

/// Django `QueryDict.get('format')`: the last `format` value, or `None`
/// when absent. Percent-decoding included (`serde_urlencoded`, like
/// Django's query parser).
fn last_format_value(query: &str) -> Option<String> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query).unwrap_or_default();
    pairs
        .into_iter()
        .filter(|(key, _)| key == "format")
        .map(|(_, value)| value)
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::super::{routes, ALLOW, SCHEMA_PATH, SCHEMA_SLASHLESS_PATH};
    use super::*;
    use axum::Router;
    use tower::ServiceExt;

    /// FX-OPENAPI-05 handler goldens, parsed from the committed fixture.
    fn handlers_fixture() -> serde_json::Value {
        let raw = include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-05.handlers.json");
        serde_json::from_str(raw).expect("fixture parses")
    }

    fn flipped_app() -> Router {
        let state = crate::state::AppState::with_edge(
            "test",
            crate::edge::EdgeHandle::for_tests("http://127.0.0.1:1"),
        );
        state.edge().set_flag(crate::edge::Prefix::App, true);
        routes().with_state(state)
    }

    fn get(path: &str, ident: &str) -> Request {
        Request::get(path)
            .header("x-forwarded-for", ident)
            .body(axum::body::Body::empty())
            .expect("request")
    }

    fn method(method: &str, path: &str, ident: &str) -> Request {
        Request::builder()
            .method(method)
            .uri(path)
            .header("x-forwarded-for", ident)
            .body(axum::body::Body::empty())
            .expect("request")
    }

    async fn body_bytes(response: Response) -> Vec<u8> {
        axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .expect("body")
            .to_vec()
    }

    /// Default rendering (FX-OPENAPI-04 `default_rendering`): YAML 200 with
    /// the `vnd.oai.openapi` content type (no `+json`), the download
    /// disposition, and the `Allow`/`Vary` shell; bytes are the builder's.
    #[tokio::test]
    async fn doc_default_is_yaml_with_shell() {
        let response = flipped_app()
            .oneshot(get(SCHEMA_PATH, "schema-yaml"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            CONTENT_TYPE_YAML
        );
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION).expect("cd"),
            DISPOSITION_YAML
        );
        assert_eq!(headers.get(header::ALLOW).expect("allow"), ALLOW);
        assert_eq!(headers.get(header::VARY).expect("vary"), "Accept");
        let body = body_bytes(response).await;
        assert_eq!(
            body,
            pidash_services::v1_openapi::doc::render_yaml().as_bytes()
        );
    }

    /// `?format=json` (FX-OPENAPI-04 `json_rendering`): same doc as JSON,
    /// `+json` content type without charset, JSON disposition.
    #[tokio::test]
    async fn doc_json_format_matches_yaml_doc() {
        let response = flipped_app()
            .oneshot(get("/api/schema/?format=json", "schema-json"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            CONTENT_TYPE_JSON
        );
        assert!(!CONTENT_TYPE_JSON.contains("charset"));
        assert_eq!(
            headers.get(header::CONTENT_DISPOSITION).expect("cd"),
            DISPOSITION_JSON
        );
        let body = body_bytes(response).await;
        assert_eq!(
            body,
            pidash_services::v1_openapi::doc::render_json().as_bytes()
        );
        let doc: serde_json::Value = serde_json::from_slice(&body).expect("json parses");
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(doc["paths"].as_object().expect("paths").len(), 121);
        assert_eq!(doc["info"]["title"], "The Pi Dash REST API");
    }

    /// `?format=` dispatch: `yaml` and empty select the YAML renderer like
    /// the default; repeats take the last value (`QueryDict.get`); matching
    /// is exact and case-sensitive.
    #[tokio::test]
    async fn format_dispatch_follows_drf_rules() {
        for (query, expect_json) in [
            ("format=yaml", false),
            ("format=", false),
            ("format=xml&format=json", true),
            ("format=json&format=yaml", false),
        ] {
            let response = flipped_app()
                .oneshot(get(&format!("/api/schema/?{query}"), "schema-fmt"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::OK, "{query}");
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("ct")
                .clone();
            if expect_json {
                assert_eq!(content_type, CONTENT_TYPE_JSON, "{query}");
            } else {
                assert_eq!(content_type, CONTENT_TYPE_YAML, "{query}");
            }
        }
        let response = flipped_app()
            .oneshot(get("/api/schema/?format=JSON", "schema-fmt-case"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Unknown formats 404 with the exact fixture bytes
    /// (FX-OPENAPI-05 `unknown_format`).
    #[tokio::test]
    async fn unknown_format_404_matches_fixture() {
        let golden = handlers_fixture();
        let unknown = &golden["unknown_format"];
        let response = flipped_app()
            .oneshot(get("/api/schema/?format=xml", "schema-xml"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            unknown["content_type"].as_str().expect("ct")
        );
        assert_eq!(headers.get(header::ALLOW).expect("allow"), ALLOW);
        assert_eq!(headers.get(header::VARY).expect("vary"), "Accept");
        assert!(headers.get(header::CONTENT_DISPOSITION).is_none());
        let body = body_bytes(response).await;
        assert_eq!(body, unknown["body"].as_str().expect("body").as_bytes());
        assert_eq!(body, UNKNOWN_FORMAT_BODY.as_bytes());
    }

    /// Every unsafe method 405s with its exact fixture body
    /// (FX-OPENAPI-05 `method_denials`, `/api/schema/`).
    #[tokio::test]
    async fn unsafe_methods_405_match_fixture() {
        let golden = handlers_fixture();
        let denials = &golden["method_denials"][SCHEMA_PATH];
        for (http_method, body_const) in [
            ("POST", NOT_ALLOWED_BODY_POST),
            ("PUT", NOT_ALLOWED_BODY_PUT),
            ("PATCH", NOT_ALLOWED_BODY_PATCH),
            ("DELETE", NOT_ALLOWED_BODY_DELETE),
        ] {
            let expected = &denials[http_method];
            let response = flipped_app()
                .oneshot(method(http_method, SCHEMA_PATH, "schema-405"))
                .await
                .expect("serve");
            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{http_method}"
            );
            let headers = response.headers().clone();
            assert_eq!(
                headers.get(header::ALLOW).expect("allow"),
                expected["allow"].as_str().expect("allow"),
                "{http_method}"
            );
            assert_eq!(
                headers.get(header::CONTENT_TYPE).expect("ct"),
                expected["content_type"].as_str().expect("ct"),
                "{http_method}"
            );
            assert_eq!(
                headers.get(header::VARY).expect("vary"),
                "Accept",
                "{http_method}"
            );
            let body = body_bytes(response).await;
            assert_eq!(
                body,
                expected["body"].as_str().expect("body").as_bytes(),
                "{http_method}"
            );
            assert_eq!(body, body_const.as_bytes(), "{http_method}");
        }
    }

    /// HEAD rides the GET handler with the body stripped, headers intact.
    #[tokio::test]
    async fn head_serves_headers_without_body() {
        let response = flipped_app()
            .oneshot(method("HEAD", SCHEMA_PATH, "schema-head"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            CONTENT_TYPE_YAML
        );
        assert_eq!(headers.get(header::ALLOW).expect("allow"), ALLOW);
        assert!(body_bytes(response).await.is_empty());
    }

    /// Slashless URL 301s to the canonical one (FX-OPENAPI-05
    /// `slashless_redirect`): relative Location, empty body, no DRF shell.
    /// All methods 301 (production APPEND_SLASH); the query string carries
    /// over (`get_full_path`).
    #[tokio::test]
    async fn slashless_redirects_to_canonical() {
        let golden = handlers_fixture();
        let slashless = &golden["slashless_redirect"];
        for http_method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let response = flipped_app()
                .oneshot(method(http_method, SCHEMA_SLASHLESS_PATH, "schema-301"))
                .await
                .expect("serve");
            assert_eq!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY,
                "{http_method}"
            );
            let headers = response.headers().clone();
            assert_eq!(
                headers.get(header::LOCATION).expect("location"),
                slashless["location"].as_str().expect("location"),
                "{http_method}"
            );
            assert_eq!(
                headers.get(header::CONTENT_TYPE).expect("ct"),
                slashless["content_type"].as_str().expect("ct"),
                "{http_method}"
            );
            assert!(headers.get(header::ALLOW).is_none(), "{http_method}");
            assert!(headers.get(header::VARY).is_none(), "{http_method}");
            assert!(body_bytes(response).await.is_empty(), "{http_method}");
        }
        let response = flipped_app()
            .oneshot(get("/api/schema?a=1&b=2", "schema-301-q"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        assert_eq!(
            response.headers().get(header::LOCATION).expect("location"),
            "/api/schema/?a=1&b=2"
        );
    }

    /// The 301 burns no throttle budget (CommonMiddleware redirects outside
    /// DRF throttling): 31 slashless hits still 301, and the same ident's
    /// doc budget is untouched.
    #[tokio::test]
    async fn slashless_burns_no_throttle_budget() {
        for _ in 0..31 {
            let response = flipped_app()
                .oneshot(get(SCHEMA_SLASHLESS_PATH, "schema-301-free"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::MOVED_PERMANENTLY);
        }
        let response = flipped_app()
            .oneshot(get(SCHEMA_PATH, "schema-301-free"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
    }

    /// Sustained burst 429s from attempt 31 with the exact YAML denial
    /// (FX-OPENAPI-05 `denial_renderers.yaml_429`) plus `Retry-After`.
    #[tokio::test]
    async fn burst_throttles_with_yaml_denial() {
        let golden = handlers_fixture();
        let denial = &golden["throttle"]["denial_renderers"]["yaml_429"];
        for _ in 0..30 {
            let response = flipped_app()
                .oneshot(get(SCHEMA_PATH, "schema-burst"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = flipped_app()
            .oneshot(get(SCHEMA_PATH, "schema-burst"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            denial["content_type"].as_str().expect("ct")
        );
        assert_eq!(headers.get(header::ALLOW).expect("allow"), ALLOW);
        assert_eq!(headers.get(header::VARY).expect("vary"), "Accept");
        let retry_after: i64 = headers
            .get(header::RETRY_AFTER)
            .expect("retry-after")
            .to_str()
            .expect("ascii")
            .parse()
            .expect("int");
        assert!(retry_after >= 1);
        let body = body_bytes(response).await;
        assert_eq!(body, denial["body"].as_str().expect("body").as_bytes());
    }

    /// The JSON denial renders through the JSON renderer
    /// (FX-OPENAPI-05 `denial_renderers.json_429`).
    #[tokio::test]
    async fn burst_throttles_with_json_denial() {
        let golden = handlers_fixture();
        let denial = &golden["throttle"]["denial_renderers"]["json_429"];
        for _ in 0..30 {
            let response = flipped_app()
                .oneshot(get("/api/schema/?format=json", "schema-jburst"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = flipped_app()
            .oneshot(get("/api/schema/?format=json", "schema-jburst"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::CONTENT_TYPE).expect("ct"),
            denial["content_type"].as_str().expect("ct")
        );
        let body = body_bytes(response).await;
        assert_eq!(body, denial["body"].as_str().expect("body").as_bytes());
    }

    /// Throttle check runs before method dispatch (FX-OPENAPI-05
    /// `unsafe_while_throttled`): a throttled POST answers 429, not 405.
    #[tokio::test]
    async fn unsafe_while_throttled_answers_429() {
        let golden = handlers_fixture();
        let pinned = &golden["throttle"]["denial_renderers"]["unsafe_while_throttled"];
        for _ in 0..30 {
            let response = flipped_app()
                .oneshot(get(SCHEMA_PATH, "schema-uwt"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = flipped_app()
            .oneshot(method("POST", SCHEMA_PATH, "schema-uwt"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(header::ALLOW).expect("allow"),
            pinned["allow"].as_str().expect("allow")
        );
        assert!(headers.get(header::RETRY_AFTER).is_some());
        let body = body_bytes(response).await;
        assert_eq!(body, pinned["body"].as_str().expect("body").as_bytes());
    }

    /// API keys do not identify on these session-auth-only views: keyed
    /// and anonymous fetches are byte-identical and spend the same budget.
    #[tokio::test]
    async fn api_keys_share_the_anonymous_budget() {
        let keyed = Request::get(SCHEMA_PATH)
            .header("x-forwarded-for", "schema-keyed")
            .header("X-Api-Key", "pidash_test_key_aaaa")
            .body(axum::body::Body::empty())
            .expect("request");
        let response = flipped_app().oneshot(keyed).await.expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let keyed_body = body_bytes(response).await;
        let response = flipped_app()
            .oneshot(get(SCHEMA_PATH, "schema-keyed-anon"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_bytes(response).await, keyed_body);
        for _ in 0..29 {
            let keyed = Request::get(SCHEMA_PATH)
                .header("x-forwarded-for", "schema-keyed")
                .header("X-Api-Key", "pidash_test_key_aaaa")
                .body(axum::body::Body::empty())
                .expect("request");
            let response = flipped_app().oneshot(keyed).await.expect("serve");
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = flipped_app()
            .oneshot(get(SCHEMA_PATH, "schema-keyed"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
