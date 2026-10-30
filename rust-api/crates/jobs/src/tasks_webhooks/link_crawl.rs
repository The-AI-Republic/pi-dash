//! Work-item link crawler task (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry point of
//! `apps/api/pi_dash/bgtasks/work_item_link_task.py:262-273`
//! (`crawl_work_item_link_title`). The pure crawl pipeline lives in
//! `pidash-services` (`tasks_webhooks::link_crawl`); this module owns the
//! Celery wire surface (task name, `(id, url)` payload), the live network
//! seams (async HTTPS with redirects off behind a sync bridge, std DNS)
//! and the `issue_links` lookup + full-row save.
//!
//! Ordering is crawl-FIRST, DB-lookup-SECOND (`:264` before `:267`): a dead
//! link still pays for the crawl. A missing or soft-deleted `IssueLink` row
//! only warns and acks (`IssueLink.DoesNotExist → return`, `:269-271`).
//!
//! The deterministic deltas of the Python full `save()` are the `metadata`
//! write, the `auto_now` `updated_at` touch, and the worker-context audit
//! blanking (`created_by`/`updated_by` NULL, `base.py:21-31` — no crum
//! user), so the port issues exactly that `UPDATE` (no `update_fields`
//! narrowing, matching the source).

use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use reqwest::redirect::Policy;
use serde_json::Value;
use uuid::Uuid;

use pidash_db::Pools;
use pidash_services::tasks_webhooks::link_crawl::{
    self, CrawlReport, DnsFailure, HttpRequest, HttpResponse, ValidateError, CRAWL_TASK_NAME,
    FAVICON_HEAD_TIMEOUT_SECS, GET_TIMEOUT_SECS,
};

use crate::worker::{Handler, Registry, Verdict};

pub use pidash_services::tasks_webhooks::link_crawl::CRAWL_TASK_NAME as LINK_CRAWL_TASK_NAME;

/// Every Celery task name this module owns.
pub const TASK_NAMES: [&str; 1] = [CRAWL_TASK_NAME];

/// `SELECT` mirroring `IssueLink.objects.get(id=id)`
/// (`db_table = "issue_links"`, `issue.py:480`; UUID pk, `base.py:18`).
/// The default manager is the `SoftDeletionManager` (`mixins.py:56-66`),
/// so soft-deleted rows are invisible to the lookup.
pub const FIND_LINK_SQL: &str =
    r#"SELECT id, metadata FROM issue_links WHERE id = $1 AND deleted_at IS NULL"#;

/// Full-row save delta: `metadata`, the `auto_now` `updated_at` touch, and
/// the worker-context audit blanking (`base.py:21-31` — no crum user, so
/// the full save persists both NULLs; same semantics as PB-2008/PB-2009).
pub const SAVE_LINK_METADATA_SQL: &str = r#"UPDATE issue_links SET metadata = $1, updated_at = NOW(), created_by_id = NULL, updated_by_id = NULL WHERE id = $2"#;

/// `(id, url)` out of a Celery payload. The Python signature is
/// `crawl_work_item_link_title(id, url)`, so `.delay(id, url)` arrives as
/// two positional string args; keyword and mixed forms bind the same way
/// Python binds them, and anything else is a `TypeError`-equivalent
/// rejection (retried with budget, like every handler failure).
pub fn parse_link_args(args: &Value, kwargs: &Value) -> Result<(String, String), String> {
    let reject = || {
        Err(format!(
            "{CRAWL_TASK_NAME} takes (id, url) string arguments"
        ))
    };
    let positional = args.as_array().cloned().unwrap_or_default();
    if positional.len() > 2 {
        return reject();
    }
    let id = positional
        .first()
        .and_then(Value::as_str)
        .or_else(|| kwargs.get("id").and_then(Value::as_str));
    let url = positional
        .get(1)
        .and_then(Value::as_str)
        .or_else(|| kwargs.get("url").and_then(Value::as_str));
    // A bound slot holding a non-string, a missing slot, or a duplicate
    // (positional plus the same keyword) all reject.
    let bound_count = positional.len()
        + usize::from(kwargs.get("id").is_some())
        + usize::from(kwargs.get("url").is_some());
    match (id, url) {
        (Some(id), Some(url)) if bound_count == 2 => Ok((id.to_owned(), url.to_owned())),
        _ => reject(),
    }
}

/// Live DNS for the SSRF guard: `socket.getaddrinfo(hostname, None)`
/// (`:54-58`). Resolution errors are `gaierror`; an empty list is its own
/// branch (`:60-61`). CPython IDNA-encodes a `str` host before resolving,
/// so the lookup runs on the ASCII (punycode) form; an IDNA refusal falls
/// back to the raw host, which keeps numeric hosts resolving without DNS
/// exactly as before (PIDASHCONV-1672).
pub fn live_resolve(hostname: &str) -> Result<Vec<IpAddr>, DnsFailure> {
    let ascii = idna::domain_to_ascii(hostname).unwrap_or_else(|_| hostname.to_owned());
    match (ascii.as_str(), 0).to_socket_addrs() {
        Ok(addrs) => {
            let ips: Vec<IpAddr> = addrs.map(|addr| addr.ip()).collect();
            if ips.is_empty() {
                Err(DnsFailure::NoAddresses)
            } else {
                Ok(ips)
            }
        }
        Err(_) => Err(DnsFailure::Unresolvable),
    }
}

fn live_validate(url: &str) -> Result<(), ValidateError> {
    link_crawl::validate_url_ip(url, &live_resolve)
}

/// One crawl client: per-operation timeouts mirroring
/// `requests.get(..., timeout=N)` (connect plus each socket read), with NO
/// total deadline — Django has none either (PIDASHCONV-1669: the old
/// per-request `.timeout()` cut slow-but-steady bodies off at N seconds).
/// The blocking builder has no per-read setting, so this is the async
/// client; `read_timeout` applies to each read and resets after a
/// successful one. Redirects stay off (`allow_redirects=False`); the pure
/// loop owns hop validation.
fn build_client(timeout_secs: u64) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(timeout_secs))
        .read_timeout(Duration::from_secs(timeout_secs))
        .build()
}

/// Header bytes to text the way `requests` exposes them: the http layer
/// decodes latin-1 (`sessions.py:get_redirect_target`), so every byte —
/// including non-ASCII obs-text rejected by `HeaderValue::to_str` — maps
/// to U+0000..=U+00FF. A UTF-8 `Location` thus arrives as the mojibake
/// `str` the redirect loop follows, never as `""`.
fn decode_header_value(value: &reqwest::header::HeaderValue) -> String {
    value.as_bytes().iter().map(|byte| *byte as char).collect()
}

/// One live request through the async client.
async fn live_request(
    client: &reqwest::Client,
    req: &HttpRequest,
    method: reqwest::Method,
) -> Result<HttpResponse, String> {
    let mut builder = client.request(method, req.url.as_str());
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let response = builder.send().await.map_err(|err| err.to_string())?;
    let status = response.status().as_u16();
    let mut headers = Vec::new();
    for (name, value) in response.headers() {
        headers.push((name.to_string(), decode_header_value(value)));
    }
    // `requests.Response.is_redirect`: redirect status WITH a location.
    let is_redirect = matches!(status, 301 | 302 | 303 | 307 | 308)
        && headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("location"));
    // `requests.get(allow_redirects=False)` still resolves the redirect
    // target eagerly (`sessions.py:739` → `get_redirect_target`, which
    // re-decodes the latin-1 header as UTF-8): non-UTF-8 `Location`
    // bytes raise there (`UnicodeDecodeError`, a `ValueError`), so Django
    // never follows them. Fail the fetch the same way instead of
    // following mojibake. (The warning branch/text stay library-specific:
    // CPython codec messages are Tier 3.)
    if is_redirect
        && response
            .headers()
            .get(reqwest::header::LOCATION)
            .is_some_and(|location| std::str::from_utf8(location.as_bytes()).is_err())
    {
        return Err("Response Location header is not valid UTF-8".to_owned());
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| err.to_string())?
        .to_vec();
    Ok(HttpResponse {
        status,
        headers: link_crawl::HttpHeaders(headers),
        body,
        is_redirect,
    })
}

/// The crawl with live seams, returning the metadata dict to persist.
/// Infallible by construction (mirrors the Python guards); warnings go to
/// the worker log at the matching levels.
///
/// The pipeline seam is synchronous, so each hop `block_on`s the async
/// client on a throwaway current-thread runtime built for the crawl
/// (link crawls are infrequent; one runtime per crawl is cheaper than one
/// DNS lookup). GET hops use the 1s client, favicon HEAD the 2s client,
/// mirroring the Python call sites.
pub fn live_crawl(url: &str) -> Value {
    let failed = |detail: String| CrawlReport {
        result: link_crawl::default_favicon().to_json(),
        warnings: vec![format!("Failed to fetch HTML for title: {detail}")],
    };
    let report = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => {
            match (
                build_client(GET_TIMEOUT_SECS),
                build_client(FAVICON_HEAD_TIMEOUT_SECS),
            ) {
                (Ok(get_client), Ok(head_client)) => {
                    let get = |req: &HttpRequest| {
                        runtime.block_on(live_request(&get_client, req, reqwest::Method::GET))
                    };
                    let head = |req: &HttpRequest| {
                        runtime.block_on(live_request(&head_client, req, reqwest::Method::HEAD))
                    };
                    link_crawl::crawl(url, &live_validate, &get, &head)
                }
                (Err(err), _) | (_, Err(err)) => failed(err.to_string()),
            }
        }
        Err(err) => failed(err.to_string()),
    };
    for warning in &report.warnings {
        tracing::warn!(task = CRAWL_TASK_NAME, "{warning}");
    }
    report.result
}

/// Register the `crawl_work_item_link_title` handler: crawl first, then the
/// `issue_links` lookup; a missing row warns and acks, a present row gets
/// the metadata `UPDATE`. Transport-level crawl failures are already
/// contained in the metadata shape, so only payload/DB failures retry.
pub fn register_link_crawl_handler(registry: &mut Registry, pools: Pools) {
    let handler: Handler = Arc::new(move |job| {
        let pools = pools.clone();
        Box::pin(async move {
            let (link_id, url) = parse_link_args(&job.args, &job.kwargs)
                .map_err(|detail| format!("{CRAWL_TASK_NAME}: {detail}"))?;
            let metadata = tokio::task::spawn_blocking({
                let url = url.clone();
                move || live_crawl(&url)
            })
            .await
            .map_err(|err| format!("{CRAWL_TASK_NAME}: crawl join failed: {err}"))?;

            let id = Uuid::parse_str(&link_id)
                .map_err(|_| format!("{CRAWL_TASK_NAME}: invalid IssueLink id {link_id:?}"))?;
            let exists: Option<Uuid> = sqlx::query_scalar(FIND_LINK_SQL)
                .bind(id)
                .fetch_optional(pools.primary())
                .await
                .map_err(|err| format!("{CRAWL_TASK_NAME}: lookup failed: {err}"))?;
            if exists.is_none() {
                tracing::warn!("IssueLink not found for the id {link_id} and the url {url}");
                return Ok(Verdict::Ack);
            }
            sqlx::query(SAVE_LINK_METADATA_SQL)
                .bind(sqlx::types::Json(&metadata))
                .bind(id)
                .execute(pools.primary())
                .await
                .map_err(|err| format!("{CRAWL_TASK_NAME}: save failed: {err}"))?;
            Ok(Verdict::Ack)
        })
    });
    registry.register(CRAWL_TASK_NAME, handler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_name_matches_python() {
        assert_eq!(
            CRAWL_TASK_NAME,
            "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title"
        );
        assert_eq!(TASK_NAMES, [CRAWL_TASK_NAME]);
        assert_eq!(LINK_CRAWL_TASK_NAME, CRAWL_TASK_NAME);
    }

    #[test]
    fn parses_positional_id_and_url() {
        let args = json!([
            "3fa85f64-5717-4562-b3fc-2c963f66afa6",
            "https://example.com/"
        ]);
        let (id, url) = parse_link_args(&args, &json!({})).unwrap();
        assert_eq!(id, "3fa85f64-5717-4562-b3fc-2c963f66afa6");
        assert_eq!(url, "https://example.com/");
    }

    #[test]
    fn parses_keyword_id_and_url() {
        let kwargs = json!({"id": "abc", "url": "https://example.com/"});
        let (id, url) = parse_link_args(&json!([]), &kwargs).unwrap();
        assert_eq!((id.as_str(), url.as_str()), ("abc", "https://example.com/"));
    }

    #[test]
    fn binds_mixed_positional_and_keyword() {
        // `f(id, url=url)` binds like Python: one positional + one keyword.
        let (id, url) =
            parse_link_args(&json!(["abc"]), &json!({"url": "https://example.com/"})).unwrap();
        assert_eq!((id.as_str(), url.as_str()), ("abc", "https://example.com/"));
    }

    #[test]
    fn rejects_bad_payload_shapes() {
        for (args, kwargs) in [
            (json!([]), json!({})),
            (json!(["only-id"]), json!({})),
            (json!([1, 2]), json!({})),
            (json!(["a", "b", "c"]), json!({})),
            // Duplicate binding (`f(id, url, id=…)`) is a TypeError.
            (json!(["a", "b"]), json!({"id": "c"})),
            (json!([]), json!({"id": 5, "url": "https://example.com/"})),
        ] {
            let err = parse_link_args(&args, &kwargs).unwrap_err();
            assert!(
                err.contains(CRAWL_TASK_NAME) && err.contains("(id, url)"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn sql_targets_issue_links_metadata() {
        assert!(FIND_LINK_SQL.contains("issue_links"));
        assert!(FIND_LINK_SQL.contains("WHERE id = $1"));
        // SoftDeletionManager parity: stamped rows are invisible (PIDASHCONV-1281).
        assert!(FIND_LINK_SQL.contains("deleted_at IS NULL"));
        assert!(SAVE_LINK_METADATA_SQL.contains("issue_links"));
        assert!(SAVE_LINK_METADATA_SQL.contains("metadata = $1"));
        // Full save(): no update_fields narrowing — updated_at touched, and
        // the worker-context audit blanking persists both NULLs (PB-2008/2009 semantics).
        assert!(SAVE_LINK_METADATA_SQL.contains("updated_at"));
        assert!(SAVE_LINK_METADATA_SQL.contains("created_by_id = NULL"));
        assert!(SAVE_LINK_METADATA_SQL.contains("updated_by_id = NULL"));
        assert!(SAVE_LINK_METADATA_SQL.contains("WHERE id = $2"));
    }

    /// One request through the real adapter: real client constructor,
    /// real request path, throwaway runtime like `live_crawl`.
    fn fetch(
        timeout_secs: u64,
        method: reqwest::Method,
        url: &str,
    ) -> Result<HttpResponse, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = build_client(timeout_secs).unwrap();
        let req = HttpRequest {
            url: url.to_owned(),
            headers: vec![],
            timeout_secs,
        };
        runtime.block_on(live_request(&client, &req, method))
    }

    /// One-shot HTTP/1.1 server: consumes the request head, waits
    /// `before_headers_ms`, sends headers plus half the body, waits
    /// `mid_body_ms`, sends the rest. A failed tail write (client timed
    /// out and went away) is an expected outcome, not an error.
    fn drip_server(
        before_headers_ms: u64,
        mid_body_ms: u64,
        body: &'static [u8],
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = conn.read(&mut buf).unwrap();
                if n == 0 {
                    return;
                }
                seen.extend_from_slice(&buf[..n]);
                if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(before_headers_ms));
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            conn.write_all(head.as_bytes()).unwrap();
            conn.write_all(&body[..body.len() / 2]).unwrap();
            conn.flush().unwrap();
            std::thread::sleep(Duration::from_millis(mid_body_ms));
            let _ = conn.write_all(&body[body.len() / 2..]);
        });
        (format!("http://127.0.0.1:{port}/"), handle)
    }

    /// One-shot redirect server: consumes the request head, then answers
    /// `302` with the given raw `Location` bytes (no re-encoding, so the
    /// wire bytes this pins are exact).
    fn redirect_server(location: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = conn.read(&mut buf).unwrap();
                if n == 0 {
                    return;
                }
                seen.extend_from_slice(&buf[..n]);
                if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            conn.write_all(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: ")
                .unwrap();
            conn.write_all(location).unwrap();
            conn.write_all(b"\r\nConnection: close\r\n\r\n").unwrap();
        });
        (format!("http://127.0.0.1:{port}/"), handle)
    }

    /// PIDASHCONV-1669: no single read stalls for 1s, so the 1.2s drip
    /// reads whole — like `requests.get(..., timeout=1)`. The old
    /// per-request total `.timeout()` cut this off at 1s.
    #[test]
    fn slow_drip_body_reads_past_total_time() {
        let body: &'static [u8] = b"<html><head><title>Slow drip title</title></head></html>";
        let (url, server) = drip_server(600, 600, body);
        let start = std::time::Instant::now();
        let response = fetch(GET_TIMEOUT_SECS, reqwest::Method::GET, &url).unwrap();
        let elapsed = start.elapsed();
        server.join().unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, body);
        assert!(
            elapsed >= Duration::from_millis(1150),
            "drip server did not actually drip: {elapsed:?}"
        );
    }

    /// The per-read timeout still fires: headers plus half the body,
    /// then a 1.5s stall exceeds the 1s read budget — like `requests`
    /// raising `ReadTimeout`, the fetch fails instead of hanging.
    #[test]
    fn stalled_body_read_times_out() {
        let body: &'static [u8] = b"<html><head><title>Stalled</title></head></html>";
        let (url, server) = drip_server(0, 1500, body);
        let outcome = fetch(GET_TIMEOUT_SECS, reqwest::Method::GET, &url);
        server.join().unwrap();
        assert!(
            outcome.is_err(),
            "stalled read unexpectedly succeeded: {outcome:?}"
        );
    }

    /// The favicon HEAD client carries the 2s budget (`:208`): a 1.3s
    /// stall before the headers still answers. (A 1s client would fail
    /// this; the drip/stall pair above pins the GET value.)
    #[test]
    fn head_client_allows_two_second_reads() {
        let body: &'static [u8] = b"favicon-bytes";
        let (url, server) = drip_server(1300, 0, body);
        let response = fetch(FAVICON_HEAD_TIMEOUT_SECS, reqwest::Method::HEAD, &url).unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 200);
    }

    /// PIDASHCONV-1675 fix 1: `requests` exposes headers latin-1-decoded
    /// (`sessions.py:get_redirect_target`), so a UTF-8 `Location` arrives
    /// as the mojibake the redirect loop follows — never `""`.
    #[test]
    fn non_ascii_location_decodes_as_latin1_like_requests() {
        let (url, server) = redirect_server(b"/caf\xc3\xa9");
        let response = fetch(GET_TIMEOUT_SECS, reqwest::Method::GET, &url).unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 302);
        assert!(response.is_redirect);
        assert_eq!(response.headers.location(), Some("/caf\u{c3}\u{a9}"));
    }

    /// `requests.get(allow_redirects=False)` still resolves the redirect
    /// target eagerly (`sessions.py:739`), and non-UTF-8 `Location` bytes
    /// raise there — so Django never follows them. The adapter fails the
    /// fetch the same way instead of following mojibake.
    #[test]
    fn non_utf8_location_fails_like_requests_redirect_target() {
        let (url, server) = redirect_server(b"/caf\xe9");
        let outcome = fetch(GET_TIMEOUT_SECS, reqwest::Method::GET, &url);
        server.join().unwrap();
        assert!(
            outcome.is_err(),
            "non-UTF-8 Location unexpectedly decoded: {outcome:?}"
        );
    }

    #[test]
    fn live_resolve_idna_encodes_unicode_hostname() {
        // PIDASHCONV-1672: CPython IDNA-encodes a `str` host before
        // `getaddrinfo`, so `socket.getaddrinfo('münchen.de', None)`
        // resolves while the raw UTF-8 bytes fail with `gaierror(-2)`.
        // `live_resolve` must encode the same way; the fullwidth
        // 'localhost' below folds to `localhost` and resolves via the
        // hosts file, so this needs no external DNS.
        let fullwidth = "ｌｏｃａｌｈｏｓｔ";
        assert_eq!(idna::domain_to_ascii(fullwidth).unwrap(), "localhost");
        assert_eq!(
            idna::domain_to_ascii("münchen.de").unwrap(),
            "xn--mnchen-3ya.de"
        );
        let ips = live_resolve(fullwidth).expect("unicode host must resolve");
        assert!(
            ips.iter().any(|ip| ip.to_string() == "127.0.0.1"),
            "unexpected addresses: {ips:?}"
        );
    }

    #[test]
    fn live_resolve_keeps_ip_literals() {
        // The IDNA step must not disturb numeric hosts: they resolve
        // without DNS exactly as before (PIDASHCONV-1672).
        assert_eq!(
            live_resolve("127.0.0.1").unwrap(),
            vec!["127.0.0.1".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(
            live_resolve("::1").unwrap(),
            vec!["::1".parse::<IpAddr>().unwrap()]
        );
    }

    #[test]
    fn registry_owns_name_after_registration_shape() {
        // Registration needs Pools (a live database), so — like the
        // cleanup spec test — this pins the spec table the registration
        // iterates: count, uniqueness, and the Celery name prefix.
        assert_eq!(TASK_NAMES.len(), 1);
        let mut names = TASK_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 1);
        for name in names {
            assert!(
                name.starts_with("pi_dash.bgtasks.work_item_link_task."),
                "unexpected task name {name}"
            );
        }
        let registry = Registry::new();
        assert!(registry.get(CRAWL_TASK_NAME).is_none());
    }
}
