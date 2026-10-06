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
    // The edition's session-refresh endpoint. `None` for a server whose
    // session cannot be refreshed (OSS): there a 401 is final.
    refresh_url: Option<Url>,
    refresh_timeout: Duration,
    client: Client,
    // Serialize cookie writes with logout, and discard responses from the old
    // session so an in-flight refresh cannot sign the user back in.
    pub generation: Arc<Mutex<u64>>,
    // Held for the whole of a refresh, so there is one in flight at most and
    // everything that started before it finished shares its outcome.
    refresh: Arc<Mutex<RefreshState>>,
    // Abort signals for in-flight requests and streams, keyed by the page's
    // request id, so a canceled request stops instead of completing unseen.
    inflight: std::sync::Mutex<HashMap<String, Arc<Notify>>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Refresh {
    /// The server rotated the session cookies; replay the request.
    Rotated,
    /// The refresh endpoint answered 401/403: the session is over.
    Rejected,
    /// Network error, timeout, 5xx or anything else. Says nothing about the
    /// session, so it must never be reported to the page as a 401.
    Failed,
}

struct RefreshState {
    // Counts finished refresh attempts. A request records it before it is
    // sent; if it has moved by the time that request sees a 401, the attempt
    // that moved it already covers this request.
    epoch: u64,
    last: Refresh,
}

impl DesktopHttp {
    pub fn new(api_url: Url) -> Result<Self, reqwest::Error> {
        // The updater installs this provider only when it checks for updates;
        // our client is constructed earlier, including in non-updater builds.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Ok(Self {
            api_url,
            refresh_url: None,
            refresh_timeout: Duration::from_secs(30),
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            generation: Default::default(),
            refresh: Arc::new(Mutex::new(RefreshState {
                epoch: 0,
                last: Refresh::Failed,
            })),
            inflight: Default::default(),
        })
    }

    /// Refresh the session at `path` when a request gets a 401, then replay
    /// it. For editions with a short-lived access cookie and a refresh
    /// endpoint; `path` is on the API origin.
    pub fn with_session_refresh(mut self, path: &str) -> Result<Self, String> {
        let url = self
            .api_url
            .join(path)
            .ok()
            .filter(|url| path.starts_with('/') && allowed_api_url(url, &self.api_url))
            .ok_or_else(|| format!("session refresh path is not an API path: {path}"))?;
        self.refresh_url = Some(url);
        Ok(self)
    }

    pub fn refreshes_session(&self) -> bool {
        self.refresh_url.is_some()
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
const REFRESH_FAILED: &str = "session refresh failed";
const SESSION_CHANGED: &str = "session changed during request";

/// The webview's persistent cookie store, as far as this module uses it.
pub(crate) trait SessionCookies: Clone + Send + Sync + 'static {
    fn all(&self) -> Option<Vec<Cookie<'static>>>;
    fn set(&self, cookie: Cookie<'static>) -> bool;
    fn delete(&self, cookie: Cookie<'static>) -> bool;
}

impl<R: tauri::Runtime> SessionCookies for tauri::WebviewWindow<R> {
    fn all(&self) -> Option<Vec<Cookie<'static>>> {
        self.cookies().ok()
    }
    fn set(&self, cookie: Cookie<'static>) -> bool {
        self.set_cookie(cookie).is_ok()
    }
    fn delete(&self, cookie: Cookie<'static>) -> bool {
        self.delete_cookie(cookie).is_ok()
    }
}

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

fn cookie_header(mut cookies: Vec<Cookie<'static>>, url: &Url) -> Option<header::HeaderValue> {
    let jar = Jar::default();
    // cookies_for_url in Wry on macOS only compares exact domains and loses
    // .example.com cookies for api.example.com. Use the full native jar and
    // let the HTTP cookie store enforce domain, path, Secure and expiry.
    //
    // The native store can hold two cookies of one name that differ only in
    // host-only-ness, and for a deployment that sets a cookie domain it always
    // does: the sign-in navigation lets the webview store the server's
    // ``Domain=.example.com`` pair itself, while every rotation afterwards is
    // written back through a domain accessor that strips the leading dot, so it
    // lands on a *separate* host-only cookie and the navigation's copy keeps its
    // original value forever. Both serialize to the same jar key here, so the
    // one added last decides what we send -- and an expired duplicate does not
    // merely lose, it *evicts* the live cookie and we send no cookie at all.
    // Either way the API 401s, the page's refresh succeeds, its replay reads
    // this same jar and 401s again, and the user is bounced to sign-in one
    // access-token lifetime after every sign-in.
    //
    // So drop the dead ones first, then add the rest oldest-expiry-first: when
    // duplicates collapse, the longest-lived -- the most recently rotated --
    // value wins. Session cookies sort first and never displace a dated one.
    let now = unix_now();
    cookies.retain(|cookie| !cookie_expired(cookie));
    cookies.sort_by_key(|cookie| cookie_expiry(cookie, now));
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

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// When the cookie stops being valid, or `None` for a session cookie. `Max-Age`
/// takes precedence over `Expires`, per RFC 6265 section 5.2.2.
fn cookie_expiry(cookie: &Cookie<'_>, now: i64) -> Option<i64> {
    if let Some(age) = cookie.max_age() {
        return Some(now.saturating_add(age.whole_seconds()));
    }
    cookie.expires_datetime().map(|at| at.unix_timestamp())
}

fn cookie_expired(cookie: &Cookie<'_>) -> bool {
    let now = unix_now();
    cookie_expiry(cookie, now).is_some_and(|expiry| expiry <= now)
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
        return Err(SESSION_CHANGED.into());
    }
    Ok(())
}

async fn within<T>(deadline: Option<Instant>, work: impl Future<Output = T>) -> Result<T, String> {
    match deadline {
        Some(deadline) => tokio::time::timeout_at(deadline.into(), work)
            .await
            .map_err(|_| TIMED_OUT.into()),
        None => Ok(work.await),
    }
}

/// One session's view of the API: the cookie store to read and write, and the
/// generation that must still be current when it does.
#[derive(Clone)]
struct Transport<C> {
    client: Client,
    api_url: Url,
    generation: Arc<Mutex<u64>>,
    session: u64,
    cookies: C,
}

impl<C: SessionCookies> Transport<C> {
    /// Send one request, following same-origin API redirects, and return the
    /// final response with its cookies already written to the webview.
    async fn fetch(
        &self,
        mut method: Method,
        mut url: Url,
        mut headers: header::HeaderMap,
        mut body: Option<Vec<u8>>,
        deadline: Option<Instant>,
    ) -> Result<reqwest::Response, String> {
        for _ in 0..10 {
            let mut request = self
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
                let guard = self.generation.lock().await;
                if *guard != self.session {
                    return Err(SESSION_CHANGED.into());
                }
                if let Some(cookies) = cookie_header(
                    self.cookies.all().ok_or("cannot read session cookies")?,
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
                let guard = self.generation.lock().await;
                if *guard != self.session {
                    return Err(SESSION_CHANGED.into());
                }
                for header in response.headers().get_all(header::SET_COOKIE) {
                    if let Some(cookie) = header
                        .to_str()
                        .ok()
                        .and_then(|raw| response_cookie(raw, &url))
                    {
                        let stored = if cookie_expired(&cookie) {
                            self.cookies.delete(cookie)
                        } else {
                            self.cookies.set(cookie)
                        };
                        if !stored {
                            return Err("cannot update session cookies".into());
                        }
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
                if !allowed_api_url(&target, &self.api_url) {
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
            return Ok(response);
        }
        Err("too many API redirects".into())
    }
}

impl DesktopHttp {
    /// Send `request` for the page at `origin`. A 401 is answered by one
    /// session refresh and a replay, so the page sees a 401 only when the
    /// session is really over.
    async fn send_as<C: SessionCookies>(
        &self,
        cookies: C,
        origin: &str,
        request: ApiRequest,
        body: Option<Vec<u8>>,
        deadline: Option<Instant>,
    ) -> Result<(reqwest::Response, u64), String> {
        let url = request.url;
        if !allowed_api_url(&url, &self.api_url) {
            return Err("API URL is not allowed".into());
        }
        let method =
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
            let name = header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "invalid header name")?;
            if safe_request_header(&name) {
                headers.append(name, value.parse().map_err(|_| "invalid header value")?);
            }
        }
        headers.insert(
            header::ORIGIN,
            origin.parse().map_err(|_| "invalid origin")?,
        );
        let session = *self.generation.lock().await;
        let transport = Transport {
            client: self.client.clone(),
            api_url: self.api_url.clone(),
            generation: self.generation.clone(),
            session,
            cookies,
        };
        // Waits out a refresh in flight rather than racing it with cookies
        // that are about to be replaced.
        let seen = within(deadline, self.refresh.lock()).await?.epoch;
        let response = transport
            .fetch(
                method.clone(),
                url.clone(),
                headers.clone(),
                body.clone(),
                deadline,
            )
            .await?;
        let Some(refresh_url) = self.refresh_url.as_ref().filter(|refresh| {
            response.status() == reqwest::StatusCode::UNAUTHORIZED && refresh.path() != url.path()
        }) else {
            return Ok((response, session));
        };
        let mut refresh_headers = header::HeaderMap::new();
        refresh_headers.insert(header::ORIGIN, headers[header::ORIGIN].clone());
        let refresh = self.refresh_session(&transport, refresh_url, refresh_headers, seen);
        match within(deadline, refresh).await? {
            Refresh::Rotated => {
                let response = transport
                    .fetch(method, url, headers, body, deadline)
                    .await?;
                Ok((response, session))
            }
            Refresh::Rejected => Ok((response, session)),
            Refresh::Failed => {
                check_session(self, session).await?;
                Err(REFRESH_FAILED.into())
            }
        }
    }

    async fn refresh_session<C: SessionCookies>(
        &self,
        transport: &Transport<C>,
        url: &Url,
        headers: header::HeaderMap,
        seen: u64,
    ) -> Refresh {
        let mut state = self.refresh.clone().lock_owned().await;
        if state.epoch != seen {
            return state.last;
        }
        let transport = transport.clone();
        let url = url.clone();
        let deadline = Instant::now() + self.refresh_timeout;
        // Detached: the page can cancel the request that happened to start
        // the refresh while others wait on it, and a rotation abandoned after
        // the server answered would leave the cookies it replaced in the store.
        tauri::async_runtime::spawn(async move {
            let outcome = match transport
                .fetch(Method::POST, url, headers, None, Some(deadline))
                .await
            {
                Ok(response) if response.status().is_success() => Refresh::Rotated,
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    Refresh::Rejected
                }
                _ => Refresh::Failed,
            };
            state.epoch += 1;
            state.last = outcome;
            outcome
        })
        .await
        .unwrap_or(Refresh::Failed)
    }
}

/// Send `request` on behalf of the app UI in `window`.
async fn send(
    window: &tauri::WebviewWindow,
    state: &DesktopHttp,
    request: ApiRequest,
    body: Option<Vec<u8>>,
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
    let origin = if source.scheme() == "tauri" {
        crate::BUNDLE_ORIGIN.to_owned()
    } else {
        source.origin().ascii_serialization()
    };
    state
        .send_as(window.clone(), &origin, request, body, deadline)
        .await
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

/// Forward a response to `emit` as a head, then its body as UTF-8 text.
async fn pump(
    state: &DesktopHttp,
    mut response: reqwest::Response,
    generation: u64,
    emit: impl Fn(StreamEvent) -> Result<(), String>,
) -> Result<(), String> {
    emit(StreamEvent::Head(response_head(&response)))?;
    let mut pending = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "API stream failed")? {
        check_session(state, generation).await?;
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
            emit(StreamEvent::Data { text })?;
        }
    }
    emit(StreamEvent::End)
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
        let (response, generation) = send(&window, &state, request, None, None).await?;
        pump(&state, response, generation, |event| {
            channel.send(event).map_err(|_| "stream closed".into())
        })
        .await
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn api() -> Url {
        Url::parse("https://api.example.com").unwrap()
    }

    /// The webview's cookie store, for tests.
    #[derive(Clone, Default)]
    struct Jar(Arc<std::sync::Mutex<Vec<Cookie<'static>>>>);

    impl Jar {
        fn holding(cookies: &[&str]) -> Self {
            let jar = Self::default();
            for raw in cookies {
                jar.set(cookie(raw));
            }
            jar
        }
        fn value(&self, name: &str) -> Option<String> {
            let cookies = self.0.lock().unwrap();
            let found = cookies.iter().find(|c| c.name() == name);
            found.map(|c| c.value().to_owned())
        }
    }

    impl SessionCookies for Jar {
        fn all(&self) -> Option<Vec<Cookie<'static>>> {
            Some(self.0.lock().unwrap().clone())
        }
        fn set(&self, cookie: Cookie<'static>) -> bool {
            self.delete(cookie.clone());
            self.0.lock().unwrap().push(cookie);
            true
        }
        fn delete(&self, cookie: Cookie<'static>) -> bool {
            self.0
                .lock()
                .unwrap()
                .retain(|c| c.name() != cookie.name() || c.path() != cookie.path());
            true
        }
    }

    struct Reply {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: &'static str,
        delay: Duration,
        // Close the connection without answering, like a dropped network.
        hang_up: bool,
    }

    fn reply(status: u16, body: &'static str) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body,
            delay: Duration::ZERO,
            hang_up: false,
        }
    }

    /// A stand-in for the cloud API: `pidash_access` lasts until the server
    /// forgets it, and `POST /api/auth/refresh/` trades `pidash_refresh` for a
    /// new pair. `refresh` decides how that endpoint answers.
    struct Cloud {
        url: Url,
        refreshes: Arc<AtomicUsize>,
        // (method, path, cookie header) of every request, in arrival order.
        seen: Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
    }

    const REFRESH_PATH: &str = "/api/auth/refresh/";
    const EXPIRED: &[&str] = &[
        "pidash_access=stale; Path=/; HttpOnly",
        "pidash_refresh=r1; Path=/api/auth/refresh/; HttpOnly",
    ];

    fn rotated() -> Reply {
        Reply {
            headers: vec![
                (
                    "set-cookie",
                    "pidash_access=fresh; Path=/; HttpOnly; Max-Age=600".into(),
                ),
                (
                    "set-cookie",
                    "pidash_refresh=r2; Path=/api/auth/refresh/; HttpOnly; Max-Age=3600".into(),
                ),
            ],
            ..reply(200, "{}")
        }
    }

    async fn cloud(refresh: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Cloud {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let refreshes = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let refresh = Arc::new(refresh);
        let (count, log) = (refreshes.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let (count, log, refresh) = (count.clone(), log.clone(), refresh.clone());
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(n) if n > 0 => raw.extend_from_slice(&chunk[..n]),
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&raw).into_owned();
                    let mut lines = head.lines();
                    let mut request_line = lines.next().unwrap_or("").split(' ');
                    let method = request_line.next().unwrap_or("").to_owned();
                    let path = request_line.next().unwrap_or("").to_owned();
                    let cookies = lines
                        .find_map(|line| {
                            let (name, value) = line.split_once(": ")?;
                            name.eq_ignore_ascii_case("cookie")
                                .then(|| value.to_owned())
                        })
                        .unwrap_or_default();
                    log.lock()
                        .unwrap()
                        .push((method, path.clone(), cookies.clone()));
                    let reply = if path == REFRESH_PATH {
                        count.fetch_add(1, Ordering::SeqCst);
                        refresh(&cookies)
                    } else if !cookies.contains("pidash_access=fresh") {
                        reply(
                            401,
                            r#"{"detail":"Authentication credentials were not provided."}"#,
                        )
                    } else if path.ends_with("/events/") {
                        Reply {
                            headers: vec![("content-type", "text/event-stream".into())],
                            ..reply(200, "data: hello\n\n")
                        }
                    } else {
                        reply(200, r#"{"id":"user-1"}"#)
                    };
                    tokio::time::sleep(reply.delay).await;
                    if reply.hang_up {
                        return;
                    }
                    let mut out = format!(
                        "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                        reply.status,
                        reply.body.len()
                    );
                    for (name, value) in reply.headers {
                        out.push_str(&format!("{name}: {value}\r\n"));
                    }
                    out.push_str("\r\n");
                    out.push_str(reply.body);
                    let _ = socket.write_all(out.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        Cloud {
            url,
            refreshes,
            seen,
        }
    }

    impl Cloud {
        fn http(&self) -> DesktopHttp {
            DesktopHttp::new(self.url.clone())
                .unwrap()
                .with_session_refresh(REFRESH_PATH)
                .unwrap()
        }
        fn refreshes(&self) -> usize {
            self.refreshes.load(Ordering::SeqCst)
        }
        fn request(&self, method: &str, path: &str) -> ApiRequest {
            ApiRequest {
                id: "r1".into(),
                url: self.url.join(path).unwrap(),
                method: method.into(),
                headers: Vec::new(),
                has_body: false,
                timeout_ms: None,
            }
        }
    }

    const UI: &str = "tauri://localhost";

    async fn get(
        http: &DesktopHttp,
        server: &Cloud,
        jar: &Jar,
        path: &str,
    ) -> Result<(u16, String), String> {
        let (response, _) = http
            .send_as(jar.clone(), UI, server.request("GET", path), None, None)
            .await?;
        let status = response.status().as_u16();
        Ok((status, response.text().await.unwrap()))
    }

    #[tokio::test]
    async fn an_expired_access_cookie_is_refreshed_and_the_request_replayed() {
        let server = cloud(|_| rotated()).await;
        let http = server.http();
        let jar = Jar::holding(EXPIRED);
        // The 60s agent-runtime poll; any other request takes the same path.
        let path = "/api/users/me/ai-assistant/agent-profile/";
        assert_eq!(
            get(&http, &server, &jar, path).await,
            Ok((200, r#"{"id":"user-1"}"#.into()))
        );
        assert_eq!(server.refreshes(), 1);
        assert_eq!(jar.value("pidash_access").as_deref(), Some("fresh"));
        assert_eq!(jar.value("pidash_refresh").as_deref(), Some("r2"));
        // The jar's order within a Cookie header is not stable.
        let seen: Vec<_> = server
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|(method, path, cookies)| {
                let mut cookies: Vec<_> = cookies.split("; ").collect();
                cookies.sort();
                format!("{method} {path} {}", cookies.join("; "))
            })
            .collect();
        assert_eq!(
            seen,
            [
                format!("GET {path} pidash_access=stale"),
                // The refresh cookie only ever travels to the refresh endpoint.
                format!("POST {REFRESH_PATH} pidash_access=stale; pidash_refresh=r1"),
                format!("GET {path} pidash_access=fresh"),
            ]
        );
    }

    #[tokio::test]
    async fn a_request_body_survives_the_replay() {
        let server = cloud(|_| rotated()).await;
        let http = server.http();
        let jar = Jar::holding(EXPIRED);
        let mut request = server.request("POST", "/api/workspaces/acme/issues/");
        request.has_body = true;
        let (response, _) = http
            .send_as(jar, UI, request, Some(b"{}".to_vec()), None)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].0, "POST");
        assert_eq!(seen[2].0, "POST");
    }

    #[tokio::test]
    async fn an_event_stream_opens_after_a_refresh_instead_of_showing_its_401_head() {
        let server = cloud(|_| rotated()).await;
        let http = server.http();
        let request = server.request("GET", "/api/runners/chat/sessions/s1/events/");
        let (response, generation) = http
            .send_as(Jar::holding(EXPIRED), UI, request, None, None)
            .await
            .unwrap();
        let events = std::sync::Mutex::new(Vec::new());
        pump(&http, response, generation, |event| {
            events.lock().unwrap().push(match event {
                StreamEvent::Head(head) => format!("head {}", head.status),
                StreamEvent::Data { text } => format!("data {text:?}"),
                StreamEvent::End => "end".into(),
            });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            events.into_inner().unwrap(),
            ["head 200", r#"data "data: hello\n\n""#, "end"]
        );
        assert_eq!(server.refreshes(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn concurrent_401s_share_one_refresh() {
        let server = cloud(|_| Reply {
            delay: Duration::from_millis(200),
            ..rotated()
        })
        .await;
        let http = Arc::new(server.http());
        let jar = Jar::holding(EXPIRED);
        let requests: Vec<_> = (0..8)
            .map(|n| {
                let (http, jar) = (http.clone(), jar.clone());
                let request = server.request("GET", &format!("/api/items/{n}/"));
                tokio::spawn(async move {
                    let (response, _) = http.send_as(jar, UI, request, None, None).await?;
                    Ok::<_, String>(response.status().as_u16())
                })
            })
            .collect();
        for request in requests {
            assert_eq!(request.await.unwrap(), Ok(200));
        }
        assert_eq!(server.refreshes(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_transient_refresh_failure_is_an_error_never_a_401() {
        for failure in [
            || reply(503, "busy"),
            || reply(500, "oops"),
            // Not this server's refresh endpoint: a misconfiguration.
            || reply(404, "not found"),
            || Reply {
                hang_up: true,
                ..reply(0, "")
            },
        ] {
            let server = cloud(move |_| Reply {
                delay: Duration::from_millis(100),
                ..failure()
            })
            .await;
            let http = Arc::new(server.http());
            let jar = Jar::holding(EXPIRED);
            let requests: Vec<_> = (0..4)
                .map(|n| {
                    let (http, jar) = (http.clone(), jar.clone());
                    let request = server.request("GET", &format!("/api/items/{n}/"));
                    tokio::spawn(async move {
                        let sent = http.send_as(jar, UI, request, None, None).await;
                        sent.map(|(response, _)| response.status().as_u16())
                    })
                })
                .collect();
            for request in requests {
                assert_eq!(request.await.unwrap(), Err(REFRESH_FAILED.to_owned()));
            }
            // One attempt for the whole burst, and the session is untouched.
            assert_eq!(server.refreshes(), 1);
            assert_eq!(jar.value("pidash_refresh").as_deref(), Some("r1"));

            // Nothing is remembered: the next request tries again.
            let path = "/api/users/me/";
            assert_eq!(
                get(&http, &server, &jar, path).await,
                Err(REFRESH_FAILED.to_owned())
            );
            assert_eq!(server.refreshes(), 2);
        }
    }

    #[tokio::test]
    async fn a_refresh_that_never_answers_times_out_as_a_failure() {
        let server = cloud(|_| Reply {
            delay: Duration::from_secs(30),
            ..rotated()
        })
        .await;
        let mut http = server.http();
        http.refresh_timeout = Duration::from_millis(100);
        let jar = Jar::holding(EXPIRED);
        assert_eq!(
            get(&http, &server, &jar, "/api/users/me/").await,
            Err(REFRESH_FAILED.to_owned())
        );
    }

    #[tokio::test]
    async fn a_rejected_refresh_hands_the_page_the_original_401() {
        for status in [401, 403] {
            let server = cloud(move |_| reply(status, r#"{"detail":"session_revoked"}"#)).await;
            let http = server.http();
            let jar = Jar::holding(EXPIRED);
            let (status, body) = get(&http, &server, &jar, "/api/users/me/").await.unwrap();
            assert_eq!(status, 401);
            assert!(body.contains("Authentication credentials were not provided"));
            assert_eq!(server.refreshes(), 1);
        }
    }

    #[tokio::test]
    async fn a_401_after_a_successful_refresh_is_final() {
        // The server rotates the refresh cookie but keeps refusing the access
        // cookie it issues.
        let server = cloud(|_| Reply {
            headers: vec![("set-cookie", "pidash_access=refused; Path=/".into())],
            ..reply(200, "{}")
        })
        .await;
        let http = server.http();
        let jar = Jar::holding(EXPIRED);
        let (status, _) = get(&http, &server, &jar, "/api/users/me/").await.unwrap();
        assert_eq!(status, 401);
        assert_eq!(server.refreshes(), 1);
        assert_eq!(server.seen.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn the_refresh_endpoint_itself_is_never_refreshed() {
        let server = cloud(|_| reply(401, r#"{"detail":"expired"}"#)).await;
        let http = server.http();
        let request = server.request("POST", REFRESH_PATH);
        let (response, _) = http
            .send_as(Jar::holding(EXPIRED), UI, request, None, None)
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(server.refreshes(), 1);
    }

    #[tokio::test]
    async fn without_a_refresh_endpoint_a_401_is_final() {
        let server = cloud(|_| rotated()).await;
        let http = DesktopHttp::new(server.url.clone()).unwrap();
        assert!(!http.refreshes_session());
        let jar = Jar::holding(EXPIRED);
        let (status, _) = get(&http, &server, &jar, "/api/users/me/").await.unwrap();
        assert_eq!(status, 401);
        assert_eq!(server.refreshes(), 0);
    }

    // The page aborts requests freely (route changes, SWR dedupe). If the one
    // that started the refresh is among them, the rotation must still land:
    // every other caller is waiting on it.
    #[tokio::test(flavor = "multi_thread")]
    async fn canceling_the_request_that_started_a_refresh_does_not_abandon_it() {
        let server = cloud(|_| Reply {
            delay: Duration::from_millis(200),
            ..rotated()
        })
        .await;
        let http = Arc::new(server.http());
        let jar = Jar::holding(EXPIRED);
        let first = {
            let (http, jar) = (http.clone(), jar.clone());
            let request = server.request("GET", "/api/users/me/");
            tokio::spawn(async move { http.send_as(jar, UI, request, None, None).await.is_ok() })
        };
        while server.refreshes() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        first.abort();
        assert_eq!(
            get(&http, &server, &jar, "/api/users/me/").await,
            Ok((200, r#"{"id":"user-1"}"#.into()))
        );
        assert_eq!(server.refreshes(), 1);
        assert_eq!(jar.value("pidash_refresh").as_deref(), Some("r2"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_out_during_a_refresh_discards_the_rotated_cookies() {
        let server = cloud(|_| Reply {
            delay: Duration::from_millis(200),
            ..rotated()
        })
        .await;
        let http = Arc::new(server.http());
        let jar = Jar::holding(EXPIRED);
        let request = {
            let (http, jar) = (http.clone(), jar.clone());
            let request = server.request("GET", "/api/users/me/");
            tokio::spawn(async move {
                let sent = http.send_as(jar, UI, request, None, None).await;
                sent.map(|(response, _)| response.status().as_u16())
            })
        };
        while server.refreshes() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // What desktop_clear_web_data does.
        *http.generation.lock().await += 1;
        jar.0.lock().unwrap().clear();
        assert_eq!(request.await.unwrap(), Err(SESSION_CHANGED.to_owned()));
        assert_eq!(jar.value("pidash_access"), None);
    }

    #[test]
    fn the_refresh_path_must_be_an_api_path() {
        let http = || DesktopHttp::new(api()).unwrap();
        assert!(http().with_session_refresh("/api/auth/refresh/").is_ok());
        for bad in [
            "/sign-in",
            "https://evil.test/api/auth/refresh/",
            "api/x",
            "",
        ] {
            assert!(http().with_session_refresh(bad).is_err(), "{bad}");
        }
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

    // A deployment with AIREPUBLIC_COOKIE_DOMAIN set (production does: see
    // deployments/ec2/SETUP-PROD.md) leaves the native store holding *two*
    // cookies per name. The sign-in navigation stores the server's
    // ``Domain=.airepublic.com`` copy through the webview; every rotation after
    // that is written from Rust, where the cookie crate's ``domain()`` strips
    // the leading dot, so it lands on a separate host-only cookie and the
    // navigation's copy is never updated again. Both collapse to one jar key,
    // so the request must carry the freshest value regardless of the order the
    // native store reports them in -- and must never carry none at all.
    #[test]
    fn rotated_cookie_wins_over_a_stale_duplicate_in_any_order() {
        let url = Url::parse("https://pidash.airepublic.com/api/users/me/").unwrap();
        let fresh = cookie(
            "pidash_access=rotated; Domain=airepublic.com; Path=/; Secure; HttpOnly; Max-Age=600",
        );
        let stale = cookie(
            "pidash_access=from_sign_in; Domain=.airepublic.com; Path=/; Secure; HttpOnly; Max-Age=60",
        );
        for order in [
            vec![fresh.clone(), stale.clone()],
            vec![stale, fresh.clone()],
        ] {
            let sent = cookie_header(order, &url)
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            assert_eq!(sent, "pidash_access=rotated");
        }

        // Once the sign-in copy passes the access TTL it must be ignored, not
        // allowed to evict the rotated cookie and sign the user out.
        let expired = cookie(
            "pidash_access=from_sign_in; Domain=.airepublic.com; Path=/; Secure; HttpOnly; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        );
        for order in [vec![fresh.clone(), expired.clone()], vec![expired, fresh]] {
            let sent = cookie_header(order, &url)
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            assert_eq!(sent, "pidash_access=rotated");
        }
    }

    #[test]
    fn a_session_cookie_never_displaces_a_dated_duplicate() {
        let url = Url::parse("https://pidash.airepublic.com/api/users/me/").unwrap();
        let dated = cookie(
            "pidash_access=rotated; Domain=airepublic.com; Path=/; Secure; HttpOnly; Max-Age=600",
        );
        let session =
            cookie("pidash_access=from_sign_in; Domain=.airepublic.com; Path=/; Secure; HttpOnly");
        for order in [vec![dated.clone(), session.clone()], vec![session, dated]] {
            let sent = cookie_header(order, &url)
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default();
            assert_eq!(sent, "pidash_access=rotated");
        }
    }

    // The duplicate ranking has to work on the shape the native store actually
    // hands back, which is never the one the server sent: Wry rebuilds every
    // cookie from ``expiresDate``, so ``window.cookies()`` yields an absolute
    // ``Expires`` and never a ``Max-Age``. Measured on macOS 26: a cookie that
    // expires while sitting in the store is still reported, and reported *after*
    // the live one -- so the dead sign-in copy is exactly the last insert that
    // would evict the rotated cookie.
    #[test]
    fn duplicates_are_ranked_by_the_absolute_expiry_the_native_store_returns() {
        let url = Url::parse("https://pidash.airepublic.com/api/users/me/").unwrap();
        let rotated = cookie(
            "pidash_access=rotated; Domain=airepublic.com; Path=/; Secure; HttpOnly; Expires=Wed, 01 Jan 2031 00:00:00 GMT",
        );
        assert_eq!(
            rotated.max_age(),
            None,
            "the native store reports no Max-Age"
        );
        let sign_in_live = cookie(
            "pidash_access=from_sign_in; Domain=.airepublic.com; Path=/; Secure; HttpOnly; Expires=Tue, 01 Jan 2030 00:00:00 GMT",
        );
        let sign_in_dead = cookie(
            "pidash_access=from_sign_in; Domain=.airepublic.com; Path=/; Secure; HttpOnly; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        );
        for order in [
            vec![rotated.clone(), sign_in_live.clone()],
            vec![sign_in_live, rotated.clone()],
            // The order the native store was measured to report: dead copy last.
            vec![rotated.clone(), sign_in_dead.clone()],
            vec![sign_in_dead, rotated],
        ] {
            assert_eq!(
                cookie_header(order, &url)
                    .map(|v| v.to_str().unwrap().to_owned())
                    .unwrap_or_default(),
                "pidash_access=rotated"
            );
        }
    }

    #[test]
    fn max_age_takes_precedence_over_expires_when_ranking_duplicates() {
        let now = 1_700_000_000;
        // Max-Age wins even when Expires disagrees (RFC 6265 section 5.2.2).
        let c = cookie("a=b; Max-Age=600; Expires=Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(cookie_expiry(&c, now), Some(now + 600));
        assert!(!cookie_expired(&c));
        // A session cookie has no expiry and so sorts before any dated cookie.
        assert_eq!(cookie_expiry(&cookie("a=b"), now), None);
    }
}
