//! The swagger-ui and redoc page handlers (D-23).
//!
//! Ports `SpectacularSwaggerView` and `SpectacularRedocView`
//! (`pi_dash/urls.py:36-44`, single `TemplateHTMLRenderer` each — hence no
//! `Vary` header). The served bytes are the FX-OPENAPI-05 normalized
//! templates, included verbatim: utoipa-swagger-ui defaults do not match,
//! so the fixture is the template. The swagger page carries one
//! per-render hole — the CSRF token assignment — filled here with a fresh
//! 64-character token per request, like Django's masked `get_token()`;
//! the redoc page is byte-static.
//!
//! Unsafe methods answer the `TemplateHTMLRenderer.get_exception_template`
//! fallback (`405 Method Not Allowed`, FX-OPENAPI-05 `method_denials`);
//! throttled callers answer the matching 429 fallback through
//! [`throttle`](super::throttle) (`UiPage` renderer).
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::Response;
use rand::distr::{Alphanumeric, SampleString};

use crate::state::AppState;

use super::throttle::DenialRenderer;

/// UI page content type: `TemplateHTMLRenderer.media_type`.
pub const CONTENT_TYPE_HTML: &str = "text/html; charset=utf-8";

/// Exact bytes of the unsafe-method 405 on both UI pages (FX-OPENAPI-05
/// `method_denials`): the `TemplateHTMLRenderer` exception fallback
/// (`'%d %s'`, no `405.html` template exists).
pub const NOT_ALLOWED_BODY: &str = "405 Method Not Allowed";

/// The served swagger-ui template: FX-OPENAPI-05's normalized capture with
/// the CSRF hole emptied (`CSRFTOKEN"] = "";`). The hole is refilled per
/// render by [`fill_csrf_token`].
const SWAGGER_UI_TEMPLATE: &str =
    include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-05.swagger_ui.normalized.html");
/// The served redoc template: FX-OPENAPI-05's normalized capture, byte
/// for byte (the redoc page has no per-render hole).
const REDOC_TEMPLATE: &str =
    include_str!("../../../../fixtures/v1_openapi/FX-OPENAPI-05.redoc.normalized.html");

/// The emptied CSRF assignment in [`SWAGGER_UI_TEMPLATE`], exactly once.
const CSRF_HOLE: &str = "CSRFTOKEN\"] = \"\";";
/// Length of Django's CSRF token (`get_token()`, 64 chars; fixture
/// `csrf_hole.token_length`).
const CSRF_TOKEN_LEN: usize = 64;

/// `GET /api/schema/swagger-ui/`: the swagger-ui page with a fresh CSRF
/// token per render. Gated on the cutover flip, throttled.
pub async fn serve_swagger_ui(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let ident = super::peer_ident(&req);
    let session = super::session_handle(&req);
    if let Some(denied) = super::throttle_verdict(&state, ident, session).await {
        return super::throttled(DenialRenderer::UiPage, denied.wait, false);
    }
    super::view_response(
        StatusCode::OK,
        CONTENT_TYPE_HTML,
        fill_csrf_token(new_csrf_token()).into_bytes(),
        false,
    )
}

/// `GET /api/schema/redoc/`: the redoc page, byte-static. Gated on the
/// cutover flip, throttled.
pub async fn serve_redoc(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let ident = super::peer_ident(&req);
    let session = super::session_handle(&req);
    if let Some(denied) = super::throttle_verdict(&state, ident, session).await {
        return super::throttled(DenialRenderer::UiPage, denied.wait, false);
    }
    super::view_response(
        StatusCode::OK,
        CONTENT_TYPE_HTML,
        REDOC_TEMPLATE.as_bytes().to_vec(),
        false,
    )
}

/// Unsafe methods on either UI page: the pinned `405 Method Not Allowed`
/// fallback (throttle-checked first, so a throttled caller answers 429
/// instead).
pub async fn method_not_allowed(State(state): State<AppState>, req: Request) -> Response {
    if !super::rust_serves(&state) {
        return crate::edge::proxy(State(state), req).await;
    }
    let ident = super::peer_ident(&req);
    let session = super::session_handle(&req);
    if let Some(denied) = super::throttle_verdict(&state, ident, session).await {
        return super::throttled(DenialRenderer::UiPage, denied.wait, false);
    }
    super::view_response(
        StatusCode::METHOD_NOT_ALLOWED,
        CONTENT_TYPE_HTML,
        NOT_ALLOWED_BODY.as_bytes().to_vec(),
        false,
    )
}

/// Fill the template's CSRF hole with one token. The hole occurs exactly
/// once (pinned by `csrf_hole_occurs_once`); the replacement targets that
/// single occurrence.
fn fill_csrf_token(token: String) -> String {
    let filled = format!("CSRFTOKEN\"] = \"{token}\";");
    SWAGGER_UI_TEMPLATE.replacen(CSRF_HOLE, &filled, 1)
}

/// One fresh CSRF token: 64 alphanumeric chars, like Django's masked
/// `get_token()` (decorative here — the page only sends it on non-GET
/// schema fetches, which never happen — but the contract pins its shape).
fn new_csrf_token() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), CSRF_TOKEN_LEN)
}

#[cfg(test)]
mod tests {
    use super::super::{routes, ALLOW, REDOC_PATH, SWAGGER_UI_PATH};
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

    async fn body_text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        String::from_utf8(bytes.to_vec()).expect("utf-8")
    }

    /// Mirror of the contract suite's `normalize` (`test_ui_pages.py`):
    /// blank the per-render CSRF token so page bytes compare.
    fn normalize(body: &str) -> String {
        let prefix = "CSRFTOKEN\"] = \"";
        let (head, rest) = body.split_once(prefix).expect("one csrf hole");
        let (_, tail) = rest.split_once("\";").expect("hole closes");
        format!("{head}{prefix}\";{tail}")
    }

    /// The template's CSRF hole occurs exactly once (FX-OPENAPI-05
    /// `csrf_hole.occurrences`).
    #[test]
    fn csrf_hole_occurs_once() {
        assert_eq!(SWAGGER_UI_TEMPLATE.matches(CSRF_HOLE).count(), 1);
    }

    /// The swagger page normalizes to the fixture template and carries the
    /// pinned markers plus a fresh 64-char token per render (FX-OPENAPI-05
    /// `ui_pages.swagger_ui`).
    #[tokio::test]
    async fn swagger_page_matches_template_with_token() {
        let golden = handlers_fixture();
        let pinned = &golden["ui_pages"]["swagger_ui"];
        let response = flipped_app()
            .oneshot(get(SWAGGER_UI_PATH, "ui-swagger"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE).expect("ct"),
            pinned["content_type"].as_str().expect("ct")
        );
        assert_eq!(
            headers.get(axum::http::header::ALLOW).expect("allow"),
            ALLOW
        );
        assert!(headers.get(axum::http::header::VARY).is_none());
        let body = body_text(response).await;
        assert!(body.contains("<title>The Pi Dash REST API</title>"));
        assert!(body.contains("swagger-ui-bundle"));
        assert!(body.contains("url: \"/api/schema/\""));
        assert_eq!(normalize(&body), SWAGGER_UI_TEMPLATE);

        let prefix = "CSRFTOKEN\"] = \"";
        let rest = body.split_once(prefix).expect("hole").1;
        let token = rest.split_once("\";").expect("close").0;
        assert_eq!(token.len(), CSRF_TOKEN_LEN);
        assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));

        let again = flipped_app()
            .oneshot(get(SWAGGER_UI_PATH, "ui-swagger-2"))
            .await
            .expect("serve");
        let again = body_text(again).await;
        assert_ne!(body, again, "token is per-render");
        assert_eq!(normalize(&again), SWAGGER_UI_TEMPLATE);
    }

    /// The redoc page is the fixture template byte for byte (FX-OPENAPI-05
    /// `ui_pages.redoc`, no CSRF hole).
    #[tokio::test]
    async fn redoc_page_matches_template() {
        let golden = handlers_fixture();
        let pinned = &golden["ui_pages"]["redoc"];
        let response = flipped_app()
            .oneshot(get(REDOC_PATH, "ui-redoc"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE).expect("ct"),
            pinned["content_type"].as_str().expect("ct")
        );
        assert_eq!(
            headers.get(axum::http::header::ALLOW).expect("allow"),
            ALLOW
        );
        assert!(headers.get(axum::http::header::VARY).is_none());
        let body = body_text(response).await;
        assert_eq!(body, REDOC_TEMPLATE);
        assert!(body.contains("<title>The Pi Dash REST API</title>"));
        assert!(body.contains("<redoc spec-url=\"/api/schema/\">"));
    }

    /// Every unsafe method 405s on both pages with the exact fallback bytes
    /// (FX-OPENAPI-05 `method_denials`, both UI paths).
    #[tokio::test]
    async fn ui_unsafe_methods_405_match_fixture() {
        let golden = handlers_fixture();
        for path in [SWAGGER_UI_PATH, REDOC_PATH] {
            let denials = &golden["method_denials"][path];
            for http_method in ["POST", "PUT", "PATCH", "DELETE"] {
                let expected = &denials[http_method];
                let response = flipped_app()
                    .oneshot(method(http_method, path, "ui-405"))
                    .await
                    .expect("serve");
                assert_eq!(
                    response.status(),
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{http_method} {path}"
                );
                let headers = response.headers().clone();
                assert_eq!(
                    headers.get(axum::http::header::ALLOW).expect("allow"),
                    expected["allow"].as_str().expect("allow"),
                    "{http_method} {path}"
                );
                assert_eq!(
                    headers.get(axum::http::header::CONTENT_TYPE).expect("ct"),
                    expected["content_type"].as_str().expect("ct"),
                    "{http_method} {path}"
                );
                assert!(headers.get(axum::http::header::VARY).is_none());
                let body = body_text(response).await;
                assert_eq!(
                    body,
                    expected["body"].as_str().expect("body"),
                    "{http_method} {path}"
                );
                assert_eq!(body, NOT_ALLOWED_BODY, "{http_method} {path}");
            }
        }
    }

    /// A burst against a UI page 429s with the template-fallback denial
    /// (FX-OPENAPI-05 `throttle.denial_body`, `429 Too Many Requests`).
    #[tokio::test]
    async fn ui_burst_throttles_with_fallback_denial() {
        for _ in 0..30 {
            let response = flipped_app()
                .oneshot(get(REDOC_PATH, "ui-burst"))
                .await
                .expect("serve");
            assert_eq!(response.status(), StatusCode::OK);
        }
        let response = flipped_app()
            .oneshot(get(REDOC_PATH, "ui-burst"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let headers = response.headers().clone();
        assert_eq!(
            headers.get(axum::http::header::CONTENT_TYPE).expect("ct"),
            CONTENT_TYPE_HTML
        );
        assert_eq!(
            headers.get(axum::http::header::ALLOW).expect("allow"),
            ALLOW
        );
        assert!(headers.get(axum::http::header::RETRY_AFTER).is_some());
        let body = body_text(response).await;
        assert_eq!(
            body,
            handlers_fixture()["throttle"]["denial_body"]
                .as_str()
                .expect("body")
        );
    }

    /// Keyed and anonymous UI fetches normalize identically (tenant
    /// invariance at the handler level; the suite proves it live).
    #[tokio::test]
    async fn ui_keyed_matches_anonymous() {
        let keyed = Request::get(SWAGGER_UI_PATH)
            .header("x-forwarded-for", "ui-keyed")
            .header("X-Api-Key", "pidash_test_key_bbbb")
            .body(axum::body::Body::empty())
            .expect("request");
        let response = flipped_app().oneshot(keyed).await.expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let keyed_body = body_text(response).await;
        let response = flipped_app()
            .oneshot(get(SWAGGER_UI_PATH, "ui-keyed-anon"))
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            normalize(&body_text(response).await),
            normalize(&keyed_body)
        );
    }
}
