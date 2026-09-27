// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The bundled UI is cross-site to its API. WKWebView/WebView2 can store the
//! login cookies but omit them on XHR (including refresh) and EventSource.
//! Make API requests natively, sharing the webview's persistent cookie store
//! with the login navigation and sign-out. Cookie values never cross the IPC
//! boundary.
//!
//! Request and response bodies travel as raw IPC bytes, framed as a 4-byte
//! big-endian length, a JSON head and then the body, so API traffic is not
//! re-encoded as JSON number arrays.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{
    Client, Method,
    cookie::{CookieStore, Jar},
    header,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeBody};
use tauri::{Manager, Url, webview::Cookie};
use tokio::sync::{Mutex, Notify};

pub struct DesktopHttp {
    pub api_url: Url,
    client: Client,
    // Serialize cookie writes with logout, and discard responses from the old
    // session so an in-flight refresh cannot sign the user back in.
    pub generation: Mutex<u64>,
    // Abort signals for in-flight requests and streams, keyed by the page's
    // request id, so a canceled request stops instead of completing unseen.
    inflight: std::sync::Mutex<HashMap<String, Arc<Notify>>>,
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
            inflight: Default::default(),
        })
    }

    fn track(&self, id: &str) -> Inflight<'_> {
        let cancel = Arc::new(Notify::new());
        self.inflight
            .lock()
            .unwrap()
            .insert(id.to_owned(), cancel.clone());
        Inflight {
            http: self,
            id: id.to_owned(),
            cancel,
        }
    }

    fn cancel(&self, id: &str) {
        if let Some(cancel) = self.inflight.lock().unwrap().remove(id) {
            // notify_one stores a permit, so a cancel that arrives before the
            // request starts waiting is not lost.
            cancel.notify_one();
        }
    }
}

struct Inflight<'a> {
    http: &'a DesktopHttp,
    id: String,
    cancel: Arc<Notify>,
}

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        let mut inflight = self.http.inflight.lock().unwrap();
        if inflight
            .get(&self.id)
            .is_some_and(|c| Arc::ptr_eq(c, &self.cancel))
        {
            inflight.remove(&self.id);
        }
    }
}

const CANCELED: &str = "API request canceled";
const TIMED_OUT: &str = "API request timed out";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRequest {
    id: String,
    url: Url,
    method: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    has_body: bool,
    // None or 0 means no deadline, matching Axios' default.
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHead {
    status: u16,
    status_text: String,
    headers: Vec<(String, String)>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Head(ResponseHead),
    Data { text: String },
    // Sent on the channel rather than implied by the command resolving:
    // channel messages can arrive after the invoke promise settles.
    End,
}

fn frame(head: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + head.len() + body.len());
    out.extend_from_slice(&(head.len() as u32).to_be_bytes());
    out.extend_from_slice(head);
    out.extend_from_slice(body);
    out
}

fn unframe(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let len: [u8; 4] = bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .ok_or("invalid API request frame")?;
    let len = u32::from_be_bytes(len) as usize;
    let head = bytes.get(4..4 + len).ok_or("invalid API request frame")?;
    Ok((head, &bytes[4 + len..]))
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
        // The webview's cookie API needs a domain. Some webviews store this
        // as a domain cookie rather than host-only, widening it to the API
        // host's subdomains; acceptable because only the API sets cookies.
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

fn response_head(response: &reqwest::Response) -> ResponseHead {
    let status = response.status();
    ResponseHead {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").into(),
        headers: response
            .headers()
            .iter()
            .filter(|(name, _)| *name != header::SET_COOKIE)
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.to_string(), v.to_owned()))
            })
            .collect(),
    }
}

async fn check_session(state: &DesktopHttp, generation: u64) -> Result<(), String> {
    if *state.generation.lock().await != generation {
        return Err("session changed during request".into());
    }
    Ok(())
}

/// Send `request`, following same-origin API redirects, and return the final
/// response with its cookies already written to the webview.
async fn send(
    window: &tauri::WebviewWindow,
    state: &DesktopHttp,
    request: ApiRequest,
    mut body: Option<Vec<u8>>,
    deadline: Option<Instant>,
) -> Result<(reqwest::Response, u64), String> {
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

    for _ in 0..10 {
        let mut request = state
            .client
            .request(method.clone(), url.clone())
            .headers(headers.clone());
        if let Some(deadline) = deadline {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(TIMED_OUT)?;
            request = request.timeout(remaining);
        }
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
                TIMED_OUT
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
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
            && let Some(location) = response.headers().get(header::LOCATION)
        {
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
        return Ok((response, generation));
    }
    Err("too many API redirects".into())
}

fn read_request(
    request: &tauri::ipc::Request<'_>,
) -> Result<(ApiRequest, Option<Vec<u8>>), String> {
    // The postMessage IPC fallback delivers bytes as a JSON array.
    let fallback;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes,
        InvokeBody::Json(value) => {
            fallback = serde_json::from_value::<Vec<u8>>(value.clone())
                .map_err(|_| "invalid API request frame")?;
            &fallback
        }
    };
    let (head, body) = unframe(bytes)?;
    let request: ApiRequest =
        serde_json::from_slice(head).map_err(|_| "invalid API request frame")?;
    let body = request.has_body.then(|| body.to_vec());
    Ok((request, body))
}

#[tauri::command]
pub async fn desktop_api_request(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, DesktopHttp>,
    request: tauri::ipc::Request<'_>,
) -> Result<tauri::ipc::Response, String> {
    let (request, body) = read_request(&request)?;
    let inflight = state.track(&request.id);
    let deadline = request
        .timeout_ms
        .filter(|n| *n > 0)
        .map(|n| Instant::now() + Duration::from_millis(n));
    let work = async {
        let (response, _) = send(&window, &state, request, body, deadline).await?;
        let head = serde_json::to_vec(&response_head(&response)).map_err(|e| e.to_string())?;
        let body = response.bytes().await.map_err(|e| {
            if e.is_timeout() {
                TIMED_OUT
            } else {
                "cannot read API response"
            }
        })?;
        Ok(tauri::ipc::Response::new(frame(&head, &body)))
    };
    tokio::select! {
        result = work => result,
        _ = inflight.cancel.notified() => Err(CANCELED.into()),
    }
}

/// Stream a response body (Server-Sent Events) to the page as UTF-8 text.
/// Resolves when the body ends; the page cancels it with
/// `desktop_api_cancel`.
#[tauri::command]
pub async fn desktop_api_stream(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, DesktopHttp>,
    request: ApiRequest,
    channel: Channel<StreamEvent>,
) -> Result<(), String> {
    let inflight = state.track(&request.id);
    let work = async {
        let (mut response, generation) = send(&window, &state, request, None, None).await?;
        channel
            .send(StreamEvent::Head(response_head(&response)))
            .map_err(|_| "stream closed")?;
        let mut pending = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "API stream failed")? {
            check_session(&state, generation).await?;
            pending.extend_from_slice(&chunk);
            // Emit only whole UTF-8 characters; keep a split one for the next chunk.
            let valid = match std::str::from_utf8(&pending) {
                Ok(_) => pending.len(),
                Err(e) if e.error_len().is_none() => e.valid_up_to(),
                Err(_) => return Err("API stream is not UTF-8".into()),
            };
            if valid > 0 {
                let rest = pending.split_off(valid);
                let text = String::from_utf8(std::mem::replace(&mut pending, rest))
                    .map_err(|_| "API stream is not UTF-8")?;
                channel
                    .send(StreamEvent::Data { text })
                    .map_err(|_| "stream closed")?;
            }
        }
        channel
            .send(StreamEvent::End)
            .map_err(|_| "stream closed")?;
        Ok(())
    };
    tokio::select! {
        result = work => result,
        _ = inflight.cancel.notified() => Err(CANCELED.into()),
    }
}

#[tauri::command]
pub fn desktop_api_cancel(state: tauri::State<'_, DesktopHttp>, id: String) {
    state.cancel(&id);
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
    fn frames_round_trip_and_reject_truncation() {
        let framed = frame(br#"{"a":1}"#, &[0, 255]);
        assert_eq!(
            unframe(&framed).unwrap(),
            (&br#"{"a":1}"#[..], &[0u8, 255][..])
        );
        assert!(unframe(&framed[..3]).is_err());
        assert!(unframe(&framed[..8]).is_err());
    }

    #[tokio::test]
    async fn cancel_is_delivered_even_before_the_request_waits() {
        let http = DesktopHttp::new(api()).unwrap();
        let inflight = http.track("r1");
        http.cancel("r1");
        tokio::time::timeout(Duration::from_secs(1), inflight.cancel.notified())
            .await
            .expect("cancel permit was lost");
        drop(inflight);
        assert!(http.inflight.lock().unwrap().is_empty());
    }

    #[test]
    fn finished_requests_do_not_leak_or_evict_a_reused_id() {
        let http = DesktopHttp::new(api()).unwrap();
        let first = http.track("r1");
        let second = http.track("r1");
        drop(first);
        assert!(http.inflight.lock().unwrap().contains_key("r1"));
        drop(second);
        assert!(http.inflight.lock().unwrap().is_empty());
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
