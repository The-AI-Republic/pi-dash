//! D-00 web edge handlers: the Rust port of `pi_dash.web.views`.
//!
//! The whole domain is two public function views with no DB access, no auth
//! classes, and no tenant scoping:
//!
//! - U1 `health_check` (`apps/api/pi_dash/web/views.py:8-9`), routed at
//!   `""` (`apps/api/pi_dash/web/urls.py:8`)
//! - U2 `robots_txt` (`apps/api/pi_dash/web/views.py:12-13`), routed at
//!   `"robots.txt"` (`apps/api/pi_dash/web/urls.py:8`)
//!
//! Both are served byte-identical to Django (see the `F-WEB-01`/`F-WEB-02`
//! fixture goldens under `rust-api/fixtures/web/`). The ownership rule lives
//! on one code path in [`crate::edge`]: a request is Rust-served only while
//! the web prefix flag is on and only for `GET`/`HEAD`; everything else
//! proxies so Django's exact behavior — including the CSRF-failure page for
//! unsafe methods without a token — is preserved.

use axum::extract::{Request, State};
use axum::response::Response;

use crate::state::AppState;

/// Exact bytes Django renders for `GET /`
/// (`JsonResponse({"status": "OK"})` under Django 4.2.30: default compact
/// separators, ASCII-only body so no `ensure_ascii` question arises).
pub const HEALTH_BODY: &[u8] = b"{\"status\": \"OK\"}";
/// Exact bytes Django renders for `GET /robots.txt`
/// (`HttpResponse("User-agent: *\nDisallow: /", content_type="text/plain")`:
/// exactly `text/plain`, no charset suffix).
pub const ROBOTS_BODY: &[u8] = b"User-agent: *\nDisallow: /";

/// `GET /` (U1 `health_check`). Rust-served only while the web prefix is
/// flipped; every other method, and every request while unflipped, proxies.
pub async fn health_check(State(state): State<AppState>, req: Request) -> Response {
    crate::edge::serve_web_bytes(&state, req, HEALTH_BODY, "application/json").await
}

/// `GET /robots.txt` (U2 `robots_txt`), same ownership rule as
/// [`health_check`].
pub async fn robots_txt(State(state): State<AppState>, req: Request) -> Response {
    crate::edge::serve_web_bytes(&state, req, ROBOTS_BODY, "text/plain").await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_bodies_match_django_exact_bytes() {
        // Same literals the F-WEB-01/F-WEB-02 fixture goldens pin; the
        // `edge_proxy` integration tests prove them end to end.
        assert_eq!(HEALTH_BODY, b"{\"status\": \"OK\"}");
        assert_eq!(ROBOTS_BODY, b"User-agent: *\nDisallow: /");
    }
}
