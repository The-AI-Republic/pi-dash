// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The bundled UI is cross-site to its API. WKWebView/WebView2 can store the
//! login cookies but omit them on XHR (including refresh). Make API requests
//! natively, sharing the webview's persistent cookie store with the login
//! navigation and sign-out. Cookie values never cross the IPC boundary.

use reqwest::{
    Client, Method,
    cookie::{CookieStore, Jar},
    header,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, Url, webview::Cookie};
use tokio::sync::Mutex;

pub struct DesktopHttp {
    pub api_url: Url,
    client: Client,
    // Serialize cookie writes with logout, and discard responses from the old
    // session so an in-flight refresh cannot sign the user back in.
    pub generation: Mutex<u64>,
}

impl DesktopHttp {
    pub fn new(api_url: Url) -> Result<Self, reqwest::Error> {
        // The updater installs this provider only when it checks for updates;
        // our client is constructed earlier, including in non-updater builds.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Ok(Self {
            api_url,
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            generation: Mutex::new(0),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRequest {
    url: Url,
    method: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiResponse {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn allowed_api_url(url: &Url, api: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && same_origin(url, api)
        && url.username().is_empty()
        && url.password().is_none()
        && (url.path().starts_with("/api/") || url.path().starts_with("/auth/"))
}

fn cookie_header(cookies: Vec<Cookie<'static>>, url: &Url) -> Option<header::HeaderValue> {
    let jar = Jar::default();
    // cookies_for_url in Wry on macOS only compares exact domains and loses
    // .example.com cookies for api.example.com. Use the full native jar and
    // let the HTTP cookie store enforce domain, path, Secure and expiry.
    for cookie in cookies {
        jar.add_cookie_str(&cookie.to_string(), url);
    }
    jar.cookies(url)
}

fn response_cookie(raw: &str, url: &Url) -> Option<Cookie<'static>> {
    let mut cookie = Cookie::parse(raw.to_owned()).ok()?.into_owned();
    let host = url.host_str()?;
    if let Some(domain) = cookie.domain() {
        let domain = domain.to_ascii_lowercase();
        if host != domain && !host.ends_with(&format!(".{domain}")) {
            return None;
        }
    } else {
        cookie.set_domain(host.to_owned());
    }
    if !cookie.path().is_some_and(|p| p.starts_with('/')) {
        let path = url
            .path()
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        cookie.set_path(if path.is_empty() { "/" } else { path }.to_owned());
    }
    Some(cookie)
}

fn cookie_expired(cookie: &Cookie<'_>) -> bool {
    if let Some(age) = cookie.max_age() {
        return age.whole_seconds() <= 0;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    cookie
        .expires_datetime()
        .is_some_and(|expiry| expiry.unix_timestamp() <= now)
}

fn safe_request_header(name: &header::HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "cookie"
            | "host"
            | "origin"
            | "referer"
            | "content-length"
            | "connection"
            | "transfer-encoding"
    ) && !name.as_str().starts_with("sec-")
        && !name.as_str().starts_with("proxy-")
}

#[tauri::command]
pub async fn desktop_api_request(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, DesktopHttp>,
    request: ApiRequest,
) -> Result<ApiResponse, String> {
    let config = window.state::<crate::AppConfig>();
    let source = window
        .url()
        .map_err(|_| "cannot determine request source")?;
    let trusted = config.bundle_root.as_ref().unwrap_or(&config.target_url);
    if window.label() != "main" || !same_origin(&source, trusted) {
        return Err("native API requests require the app UI".into());
    }
    let mut url = request.url;
    if !allowed_api_url(&url, &state.api_url) {
        return Err("API URL is not allowed".into());
    }
    let mut method =
        Method::from_bytes(request.method.as_bytes()).map_err(|_| "invalid HTTP method")?;
    if !matches!(
        method,
        Method::GET
            | Method::HEAD
            | Method::POST
            | Method::PUT
            | Method::PATCH
            | Method::DELETE
            | Method::OPTIONS
    ) {
        return Err("HTTP method is not allowed".into());
    }
    let mut headers = header::HeaderMap::new();
    for (name, value) in request.headers {
        let name =
            header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| "invalid header name")?;
        if safe_request_header(&name) {
            headers.append(name, value.parse().map_err(|_| "invalid header value")?);
        }
    }
    let origin = if source.scheme() == "tauri" {
        crate::BUNDLE_ORIGIN.to_owned()
    } else {
        source.origin().ascii_serialization()
    };
    headers.insert(
        header::ORIGIN,
        origin.parse().map_err(|_| "invalid origin")?,
    );
    let generation = *state.generation.lock().await;
    let mut body = request.body;
    let timeout =
        std::time::Duration::from_millis(request.timeout_ms.filter(|n| *n > 0).unwrap_or(60_000));
    let deadline = std::time::Instant::now() + timeout;

    for _ in 0..10 {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or("API request timed out")?;
        let mut request = state
            .client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .timeout(remaining);
        {
            let guard = state.generation.lock().await;
            if *guard != generation {
                return Err("session changed during request".into());
            }
            if let Some(cookies) = cookie_header(
                window
                    .cookies()
                    .map_err(|_| "cannot read session cookies")?,
                &url,
            ) {
                request = request.header(header::COOKIE, cookies);
            }
        }
        if let Some(data) = &body {
            request = request.body(data.clone());
        }
        // Do not return reqwest's URL-bearing errors: auth URLs may contain codes.
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                "API request timed out"
            } else {
                "API request failed"
            }
        })?;
        let status = response.status();
        {
            let guard = state.generation.lock().await;
            if *guard != generation {
                return Err("session changed during request".into());
            }
            for header in response.headers().get_all(header::SET_COOKIE) {
                if let Some(cookie) = header
                    .to_str()
                    .ok()
                    .and_then(|raw| response_cookie(raw, &url))
                {
                    if cookie_expired(&cookie) {
                        window.delete_cookie(cookie)
                    } else {
                        window.set_cookie(cookie)
                    }
                    .map_err(|_| "cannot update session cookies")?;
                }
            }
        }
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            if let Some(location) = response.headers().get(header::LOCATION) {
                let target = location
                    .to_str()
                    .ok()
                    .and_then(|v| url.join(v).ok())
                    .ok_or("invalid API redirect")?;
                if !allowed_api_url(&target, &state.api_url) {
                    return Err("API redirect is not allowed".into());
                }
                if (status.as_u16() == 303 && method != Method::HEAD)
                    || (matches!(status.as_u16(), 301 | 302) && method == Method::POST)
                {
                    method = Method::GET;
                    body = None;
                    headers.remove(header::CONTENT_TYPE);
                }
                url = target;
                continue;
            }
        }
        let headers = response
            .headers()
            .iter()
            .filter(|(name, _)| *name != header::SET_COOKIE)
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.to_string(), v.to_owned()))
            })
            .collect();
        let body = response
            .bytes()
            .await
            .map_err(|_| "cannot read API response")?
            .to_vec();
        return Ok(ApiResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").into(),
            headers,
            body,
        });
    }
    Err("too many API redirects".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn api() -> Url {
        Url::parse("https://api.example.com").unwrap()
    }

    #[tokio::test]
    async fn client_initializes_before_the_updater() {
        assert!(DesktopHttp::new(api()).is_ok());
    }
    fn cookie(raw: &str) -> Cookie<'static> {
        Cookie::parse(raw.to_owned()).unwrap().into_owned()
    }
    fn header(cookies: Vec<Cookie<'static>>, path: &str) -> String {
        cookie_header(cookies, &api().join(path).unwrap())
            .map(|v| v.to_str().unwrap().to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn native_requests_send_lax_domain_cookies_but_respect_path_and_expiry() {
        let cookies = vec![
            cookie("access=valid; Domain=.example.com; Path=/; Secure; HttpOnly; SameSite=Lax"),
            cookie(
                "refresh=secret; Domain=.example.com; Path=/api/auth/refresh/; Secure; HttpOnly; SameSite=Lax",
            ),
            cookie("unrelated=secret; Domain=other.test; Path=/"),
            cookie(
                "expired=secret; Domain=.example.com; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
            ),
        ];
        assert_eq!(header(cookies.clone(), "/api/users/me/"), "access=valid");
        let refresh = header(cookies, "/api/auth/refresh/");
        assert!(refresh.contains("access=valid"));
        assert!(refresh.contains("refresh=secret"));
        assert!(!refresh.contains("unrelated"));
        assert!(!refresh.contains("expired"));
    }

    #[test]
    fn secure_and_path_scopes_are_preserved() {
        let c = cookie("session=s; Domain=api.example.com; Path=/api/auth; Secure");
        assert!(
            cookie_header(
                vec![c.clone()],
                &Url::parse("http://api.example.com/api/auth").unwrap()
            )
            .is_none()
        );
        assert_eq!(header(vec![c.clone()], "/api/auth/refresh/"), "session=s");
        assert_eq!(header(vec![c], "/api/authentication/"), "");
    }

    #[test]
    fn response_cookies_keep_refresh_scope_and_expiration() {
        let url = api().join("/api/auth/refresh/").unwrap();
        let refresh = response_cookie(
            "refresh=new; Path=/api/auth/refresh/; HttpOnly; Secure; SameSite=Lax; Max-Age=3600",
            &url,
        )
        .unwrap();
        assert_eq!(refresh.domain(), Some("api.example.com"));
        assert_eq!(refresh.path(), Some("/api/auth/refresh/"));
        assert_eq!(refresh.http_only(), Some(true));
        assert!(!cookie_expired(&refresh));
        let deleted =
            response_cookie("refresh=; Path=/api/auth/refresh/; Max-Age=0", &url).unwrap();
        assert!(cookie_expired(&deleted));
        assert!(response_cookie("bad=s; Domain=evil.example", &url).is_none());
        assert_eq!(
            response_cookie("session=s", &url).unwrap().path(),
            Some("/api/auth/refresh")
        );
    }

    #[test]
    fn only_configured_api_and_auth_paths_are_allowed_including_redirects() {
        for allowed in [
            "https://api.example.com/api/users/me/",
            "https://api.example.com/auth/get-csrf-token/",
        ] {
            assert!(allowed_api_url(&Url::parse(allowed).unwrap(), &api()));
        }
        for forbidden in [
            "https://evil.test/api/users/me/",
            "https://api.example.com.evil.test/api/users/me/",
            "http://api.example.com/api/users/me/",
            "https://api.example.com:444/api/users/me/",
            "https://user@api.example.com/api/users/me/",
            "https://api.example.com/sign-in",
            "file:///api/secrets",
            "https://api.example.com/api/../../secrets",
        ] {
            assert!(
                !allowed_api_url(&Url::parse(forbidden).unwrap(), &api()),
                "{forbidden}"
            );
        }
    }

    #[test]
    fn caller_cannot_override_cookie_or_origin_headers() {
        for name in [
            "Cookie",
            "Origin",
            "Host",
            "Sec-Fetch-Site",
            "Content-Length",
        ] {
            assert!(!safe_request_header(&name.parse().unwrap()));
        }
        for name in ["Content-Type", "X-CSRFToken", "Accept"] {
            assert!(safe_request_header(&name.parse().unwrap()));
        }
    }

    #[test]
    fn bundle_origin_validation_does_not_treat_all_custom_urls_as_same_origin() {
        let bundle = Url::parse("tauri://localhost/").unwrap();
        assert!(same_origin(
            &bundle,
            &Url::parse("tauri://localhost/acme/").unwrap()
        ));
        assert!(!same_origin(
            &bundle,
            &Url::parse("tauri://evil.test/").unwrap()
        ));
        assert!(!same_origin(&bundle, &api()));
    }
}
