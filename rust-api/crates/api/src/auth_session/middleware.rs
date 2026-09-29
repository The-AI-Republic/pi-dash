//! D-16 session-cookie semantics + tower wiring (PIDASHCONV-393).
//!
//! The live implementation is foundation code, reused read-only:
//! `SessionMiddleware.process_request`'s cookie selection lives in
//! [`pidash_auth::session::cookie_name_for_path`], and the full
//! `process_response` tower layer is
//! [`SessionLayer`](crate::middleware::session::SessionLayer) (F-08).
//! This module is the D-16 composition point over them:
//!
//! * [`auth_cookie_name_for_path`] — which cookie an auth request uses
//!   (`middleware/session.py:22-27,44-45`: `"instances" in request.path`
//!   selects `ADMIN_SESSION_COOKIE_NAME`, else `SESSION_COOKIE_NAME`).
//! * [`auth_session_layer`] — the tower layer auth routes wrap in, built
//!   from the D-16 cookie settings (`settings/common.py:597-607`:
//!   `session-id`/604800, `admin-session-id`/3600). Layer order stays with
//!   F-08 [`stack`](crate::middleware::stack); the settings half of the
//!   config is [`SessionConfig::from_settings`](crate::middleware::session::SessionConfig::from_settings).
//!
//! Cookie values, ages and flags follow Django exactly: delete is an empty
//! value with `Max-Age=0` and the epoch expiry; saves carry `Path=/`,
//! `SameSite=Lax`, the configured domain, and `Secure`/`HttpOnly` only when
//! truthy; browser-close sessions omit `Max-Age`/`expires`; saves are
//! skipped for 5xx responses; a failed save is a 500 (Django raises
//! `SessionInterrupted` there).
//!
//! The `#[cfg(test)]` suite replays the `middleware` vectors of
//! `rust-api/fixtures/auth_session/FX-AUTH-05.guards.json` (recorded by
//! PIDASHCONV-279) against the real composed layer.

pub use crate::middleware::session::{
    MemorySessionStore, PgSessionStore, RequestSession, SessionConfig, SessionExpiry,
    SessionHandle, SessionLayer, SessionRow, SessionStore, StoreError, StoredSession,
};
pub use pidash_auth::session::{ADMIN_SESSION_COOKIE_NAME, SESSION_COOKIE_NAME};

/// Which session cookie `path` uses, through the configured names:
/// paths containing `"instances"` read the admin cookie, the rest the
/// ordinary one. The substring test is verbatim Python (`in request.path`),
/// shared with [`pidash_auth::session::cookie_name_for_path`]. The borrow
/// comes from `config` (the configured cookie names).
pub fn auth_cookie_name_for_path<'a, S>(config: &'a SessionConfig<S>, path: &str) -> &'a str {
    config.cookie_name_for(path)
}

/// Tower wiring for D-16 auth routes: the F-08 session layer over a
/// settings-derived config. `store == None` stays fully transparent
/// (Django behind the proxy owns sessions until `serve` boots pools).
pub fn auth_session_layer<S: Clone>(config: SessionConfig<S>) -> SessionLayer<S> {
    SessionLayer::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, Response, StatusCode};
    use axum::routing::any;
    use pidash_auth::session::http_date;
    use pidash_auth::signing::{Signer, SESSION_SIGNING_SALT};
    use tower::{Layer, ServiceExt};

    const SECRET: &[u8] = b"d16-guards-test-secret";

    fn config(store: Option<MemorySessionStore>) -> SessionConfig<MemorySessionStore> {
        SessionConfig {
            store,
            secret_key: SECRET.to_vec(),
            cookie_name: SESSION_COOKIE_NAME.to_string(),
            admin_cookie_name: ADMIN_SESSION_COOKIE_NAME.to_string(),
            cookie_domain: None,
            cookie_secure: false,
            cookie_age_secs: 604800,
            admin_cookie_age_secs: 3600,
            save_every_request: false,
        }
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn fixture() -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/auth_session/FX-AUTH-05.guards.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn set_cookies(response: &Response<Body>) -> Vec<String> {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_owned))
            .collect()
    }

    fn has_vary_cookie(response: &Response<Body>) -> bool {
        response
            .headers()
            .get_all(header::VARY)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(',').map(str::trim).map(str::to_owned))
            .any(|v| v.eq_ignore_ascii_case("Cookie"))
    }

    fn get(path: &str, cookie: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("req");
        if let Some(cookie) = cookie {
            builder
                .headers_mut()
                .insert(header::COOKIE, cookie.parse().expect("cookie"));
        }
        builder
    }

    /// Seed one live row and return its key.
    async fn seed_row(store: &MemorySessionStore, key: &str, user: &str) {
        let now = now_unix();
        let signer = Signer::new(SECRET, SESSION_SIGNING_SALT);
        let signed = signer
            .sign_object(&serde_json::json!({"_auth_user_id": user}), now as u64)
            .expect("signs");
        store
            .save(SessionRow {
                key: Some(key.to_string()),
                session_data: signed,
                expire_date_unix: now + 604800,
                user_id: Some(user.to_string()),
                device_info: None,
            })
            .await
            .expect("seed saves");
    }

    fn touch(req: &Request<Body>, key: &str, value: &str) {
        req.extensions()
            .get::<SessionHandle>()
            .expect("session handle in extensions")
            .lock()
            .set(
                key.to_string(),
                serde_json::Value::String(value.to_string()),
            );
    }

    async fn plain_ok(_req: Request<Body>) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn set_login(req: Request<Body>) -> Response<Body> {
        touch(&req, "_auth_user_id", "user-1");
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    async fn set_login_500(req: Request<Body>) -> Response<Body> {
        touch(&req, "_auth_user_id", "user-1");
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .expect("inner")
    }

    async fn set_login_browser_close(req: Request<Body>) -> Response<Body> {
        {
            let mut session = req
                .extensions()
                .get::<SessionHandle>()
                .expect("session")
                .lock();
            session.expiry = SessionExpiry::BrowserClose;
            session.set(
                "_auth_user_id".to_string(),
                serde_json::Value::String("user-1".to_string()),
            );
        }
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("inner")
    }

    /// Drive one request through the D-16 wiring entry point
    /// ([`auth_session_layer`]) over a one-route inner router, like the
    /// F-08 session tests (concrete handler types per route).
    async fn serve(
        store: Option<MemorySessionStore>,
        inner: axum::Router,
        req: Request<Body>,
    ) -> Response<Body> {
        auth_session_layer(config(store))
            .layer(inner)
            .oneshot(req)
            .await
            .expect("infallible")
    }

    fn route(path: &str, handler: axum::routing::MethodRouter) -> axum::Router {
        axum::Router::new().route(path, handler)
    }

    #[test]
    fn cookie_selection_matches_fixture() {
        let fx = fixture();
        let vectors = fx["middleware"]["cookie_selection"]
            .as_array()
            .expect("vectors");
        assert_eq!(vectors.len(), 4);
        let cfg = config(None);
        // Fixture vector 1: ordinary auth path reads session-id.
        assert_eq!(vectors[0]["path"], "/api/auth/sign-in/");
        assert_eq!(
            auth_cookie_name_for_path(&cfg, "/api/auth/sign-in/"),
            "session-id"
        );
        assert_eq!(vectors[0]["store_key"], "AAA");
        // Fixture vector 2: instances path reads admin-session-id even
        // when both cookies are present.
        assert_eq!(vectors[1]["path"], "/api/instances/foo/");
        assert_eq!(
            auth_cookie_name_for_path(&cfg, "/api/instances/foo/"),
            "admin-session-id"
        );
        assert_eq!(vectors[1]["store_key"], "BBB");
        // Vectors 3-4: no cookie -> keyless store either way.
        assert!(vectors[2]["cookies"].as_object().expect("obj").is_empty());
        assert_eq!(vectors[2]["store_key"], serde_json::Value::Null);
        assert!(vectors[3]["cookies"].as_object().expect("obj").is_empty());
        assert_eq!(vectors[3]["store_key"], serde_json::Value::Null);
        // The ports share the verbatim substring test (not a segment
        // match): any path containing "instances" selects the admin cookie.
        assert_eq!(
            auth_cookie_name_for_path(&cfg, "/x/instances"),
            "admin-session-id"
        );
        assert_eq!(
            auth_cookie_name_for_path(&cfg, "/api/foo-instances-bar/"),
            "admin-session-id"
        );
        assert_eq!(
            auth_cookie_name_for_path(&cfg, "/api/auth/x/"),
            "session-id"
        );
    }

    #[tokio::test]
    async fn instances_path_deletes_admin_cookie_only() {
        // Behavioral half of cookie selection: with only the ordinary
        // cookie present on an instances path, the admin slot is empty and
        // keyless, so nothing is deleted; with the admin cookie present but
        // its row gone, exactly the admin cookie is deleted.
        let res = serve(
            Some(MemorySessionStore::new()),
            route("/api/instances/foo/", any(plain_ok)),
            get("/api/instances/foo/", Some("session-id=AAA")),
        )
        .await;
        assert!(set_cookies(&res).is_empty());

        let store = MemorySessionStore::new();
        let res = serve(
            Some(store),
            route("/api/instances/foo/", any(plain_ok)),
            get("/api/instances/foo/", Some("admin-session-id=BBB")),
        )
        .await;
        let cookies = set_cookies(&res);
        assert_eq!(cookies.len(), 1);
        assert!(
            cookies[0].starts_with("admin-session-id=\"\""),
            "deletes admin cookie: {}",
            cookies[0]
        );
        assert!(has_vary_cookie(&res));
    }

    #[tokio::test]
    async fn delete_empty_matches_fixture() {
        let fx = fixture();
        let want = &fx["middleware"]["response_vectors"]["delete_empty"];
        assert_eq!(want["saved"], false);
        assert_eq!(want["deleted"], serde_json::json!(["session-id"]));
        assert_eq!(want["vary"], "Cookie");

        // Cookie presented, row absent -> empty session -> delete.
        let res = serve(
            Some(MemorySessionStore::new()),
            route("/api/auth/sign-in/", any(plain_ok)),
            get("/api/auth/sign-in/", Some("session-id=AAA")),
        )
        .await;
        let cookies = set_cookies(&res);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            cookies[0],
            "session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax"
        );
        assert!(has_vary_cookie(&res));
        // Fixture cookie attributes: epoch expiry, max-age 0, path /,
        // samesite Lax, no domain/secure/httponly.
        let attrs = &want["cookies"]["session-id"];
        assert_eq!(attrs["expires"], "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(attrs["max-age"], 0);
        assert_eq!(attrs["path"], "/");
        assert_eq!(attrs["samesite"], "Lax");
        assert_eq!(attrs["domain"], "");
        assert_eq!(attrs["secure"], "");
        assert_eq!(attrs["httponly"], "");
    }

    #[tokio::test]
    async fn save_dirty_matches_fixture() {
        let fx = fixture();
        let want = &fx["middleware"]["response_vectors"]["save_dirty"];
        assert_eq!(want["saved"], true);
        assert_eq!(want["deleted"], serde_json::json!([]));
        assert_eq!(want["vary"], "Cookie");

        let store = MemorySessionStore::new();
        seed_row(&store, "SEEDKEY", "user-1").await;
        let before = now_unix();
        let res = serve(
            Some(store),
            route("/api/auth/sign-in/", any(set_login)),
            get("/api/auth/sign-in/", Some("session-id=SEEDKEY")),
        )
        .await;
        let after = now_unix();
        let cookies = set_cookies(&res);
        assert_eq!(cookies.len(), 1);
        let set = &cookies[0];
        // Django re-saves under the presented key.
        assert!(set.starts_with("session-id=SEEDKEY;"), "keeps key: {set}");
        assert!(set.contains("; Max-Age=604800;"), "age: {set}");
        assert!(set.contains("; HttpOnly"), "httponly: {set}");
        assert!(set.contains("; Path=/"), "path: {set}");
        assert!(set.contains("; SameSite=Lax"), "samesite: {set}");
        assert!(!set.contains("Secure"), "no secure flag: {set}");
        // expires ~= now + SESSION_COOKIE_AGE (allow 1s of test skew).
        let expires = set
            .split("; ")
            .find_map(|p| p.strip_prefix("expires="))
            .expect("expires present");
        let candidates: Vec<String> = ((before + 604799)..=(after + 604801))
            .map(http_date)
            .collect();
        assert!(
            candidates.iter().any(|c| c == expires),
            "expires {expires} within skew of now+604800"
        );
        assert!(has_vary_cookie(&res));
        // Fixture attribute pins.
        let attrs = &want["cookies"]["session-id"];
        assert_eq!(attrs["max-age"], 604800);
        assert_eq!(attrs["httponly"], true);
        assert_eq!(attrs["samesite"], "Lax");
        assert_eq!(attrs["path"], "/");
    }

    #[tokio::test]
    async fn admin_save_matches_fixture() {
        let fx = fixture();
        let want = &fx["middleware"]["response_vectors"]["admin_save"];
        assert_eq!(want["saved"], true);
        assert_eq!(want["deleted"], serde_json::json!([]));

        let store = MemorySessionStore::new();
        seed_row(&store, "ADMINKEY", "admin-1").await;
        let res = serve(
            Some(store),
            route("/api/instances/foo/", any(set_login)),
            get("/api/instances/foo/", Some("admin-session-id=ADMINKEY")),
        )
        .await;
        let cookies = set_cookies(&res);
        assert_eq!(cookies.len(), 1);
        let set = &cookies[0];
        assert!(
            set.starts_with("admin-session-id=ADMINKEY;"),
            "admin key: {set}"
        );
        assert!(set.contains("; Max-Age=3600;"), "admin age: {set}");
        assert!(set.contains("; HttpOnly"), "httponly: {set}");
        assert!(has_vary_cookie(&res));
        let attrs = &want["cookies"]["admin-session-id"];
        assert_eq!(attrs["max-age"], 3600);
        assert_eq!(attrs["httponly"], true);
    }

    #[tokio::test]
    async fn untouched_matches_fixture() {
        let fx = fixture();
        let want = &fx["middleware"]["response_vectors"]["untouched"];
        assert_eq!(want["saved"], false);
        assert_eq!(want["cookies"].as_object().expect("obj").len(), 0);
        assert_eq!(want["deleted"], serde_json::json!([]));
        assert_eq!(want["vary"], serde_json::Value::Null);

        let res = serve(
            Some(MemorySessionStore::new()),
            route("/api/auth/x/", any(plain_ok)),
            get("/api/auth/x/", None),
        )
        .await;
        assert!(set_cookies(&res).is_empty());
        assert!(!has_vary_cookie(&res));
    }

    #[tokio::test]
    async fn browser_close_matches_fixture() {
        let fx = fixture();
        let want = &fx["middleware"]["response_vectors"]["browser_close"];
        assert_eq!(want["saved"], true);
        let attrs = &want["cookies"]["session-id"];
        assert_eq!(attrs["max-age"], "");
        assert_eq!(attrs["expires"], "");
        assert_eq!(attrs["httponly"], true);

        let res = serve(
            Some(MemorySessionStore::new()),
            route("/api/auth/sign-in/", any(set_login_browser_close)),
            get("/api/auth/sign-in/", None),
        )
        .await;
        let cookies = set_cookies(&res);
        assert_eq!(cookies.len(), 1);
        let set = &cookies[0];
        assert!(set.starts_with("session-id="), "session cookie: {set}");
        assert!(!set.contains("Max-Age"), "no max-age: {set}");
        assert!(!set.contains("expires"), "no expires: {set}");
        assert!(set.contains("; HttpOnly"), "httponly: {set}");
        assert!(has_vary_cookie(&res));
    }

    #[tokio::test]
    async fn save_skipped_above_500() {
        // Fixture note: save skipped when response.status_code >= 500.
        let res = serve(
            Some(MemorySessionStore::new()),
            route("/api/auth/sign-in/", any(set_login_500)),
            get("/api/auth/sign-in/", None),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(set_cookies(&res).is_empty());
        // The session was still accessed, so Vary applies.
        assert!(has_vary_cookie(&res));
    }

    #[tokio::test]
    async fn save_failure_answers_500() {
        // Django raises SessionInterrupted on the concurrent-delete
        // UpdateError; the layer answers an empty 500 instead.
        #[derive(Debug, Clone)]
        struct FailingStore;
        impl SessionStore for FailingStore {
            fn load(
                &self,
                _key: String,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Option<StoredSession>, StoreError>>
                        + Send,
                >,
            > {
                Box::pin(async move { Ok(None) })
            }
            fn save(
                &self,
                _row: SessionRow,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<String, StoreError>> + Send>,
            > {
                Box::pin(async move { Err(StoreError("concurrent delete".to_string())) })
            }
        }
        let cfg = SessionConfig {
            store: Some(FailingStore),
            secret_key: SECRET.to_vec(),
            cookie_name: SESSION_COOKIE_NAME.to_string(),
            admin_cookie_name: ADMIN_SESSION_COOKIE_NAME.to_string(),
            cookie_domain: None,
            cookie_secure: false,
            cookie_age_secs: 604800,
            admin_cookie_age_secs: 3600,
            save_every_request: false,
        };
        let res = auth_session_layer(cfg)
            .layer(route("/api/auth/sign-in/", any(set_login)))
            .oneshot(get("/api/auth/sign-in/", None))
            .await
            .expect("infallible");
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn transparent_without_store() {
        // No pools yet: Django behind the proxy owns sessions; the layer
        // passes requests through untouched, even with a Cookie header
        // (and inserts no session handle, so the inner service cannot
        // touch session state).
        let res = serve(
            None,
            route("/api/auth/sign-in/", any(plain_ok)),
            get("/api/auth/sign-in/", Some("session-id=AAA")),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert!(set_cookies(&res).is_empty());
        assert!(!has_vary_cookie(&res));
    }
}
