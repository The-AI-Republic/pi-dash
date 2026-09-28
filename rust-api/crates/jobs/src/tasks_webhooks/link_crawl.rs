//! Work-item link crawler task (D-08, jobs layer).
//!
//! Port of the `@shared_task` entry point of
//! `apps/api/pi_dash/bgtasks/work_item_link_task.py:262-273`
//! (`crawl_work_item_link_title`). The pure crawl pipeline lives in
//! `pidash-services` (`tasks_webhooks::link_crawl`); this module owns the
//! Celery wire surface (task name, `(id, url)` payload), the live network
//! seams (blocking HTTPS with redirects off, std DNS) and the
//! `issue_links` lookup + full-row save.
//!
//! Ordering is crawl-FIRST, DB-lookup-SECOND (`:264` before `:267`): a dead
//! link still pays for the crawl. A missing `IssueLink` row only warns and
//! acks (`IssueLink.DoesNotExist → return`, `:269-271`).
//!
//! The deterministic deltas of the Python full `save()` are the `metadata`
//! write plus the `auto_now` `updated_at` touch, so the port issues exactly
//! that `UPDATE` (no `update_fields` narrowing, matching the source).

use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::Client;
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
pub const FIND_LINK_SQL: &str = r#"SELECT id, metadata FROM issue_links WHERE id = $1"#;

/// Full-row save delta: `metadata` plus the `auto_now` `updated_at` touch.
pub const SAVE_LINK_METADATA_SQL: &str =
    r#"UPDATE issue_links SET metadata = $1, updated_at = NOW() WHERE id = $2"#;

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
/// branch (`:60-61`).
pub fn live_resolve(hostname: &str) -> Result<Vec<IpAddr>, DnsFailure> {
    match (hostname, 0).to_socket_addrs() {
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

/// One live request through the blocking client (`allow_redirects=False`
/// lives in the client policy; the pure loop owns hop validation).
fn live_request(
    client: &Client,
    req: &HttpRequest,
    method: reqwest::Method,
) -> Result<HttpResponse, String> {
    let mut builder = client
        .request(method, req.url.as_str())
        .timeout(Duration::from_secs(req.timeout_secs));
    for (name, value) in &req.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let response = builder.send().map_err(|err| err.to_string())?;
    let status = response.status().as_u16();
    let mut headers = Vec::new();
    for (name, value) in response.headers() {
        headers.push((name.to_string(), value.to_str().unwrap_or("").to_owned()));
    }
    // `requests.Response.is_redirect`: redirect status WITH a location.
    let is_redirect = matches!(status, 301 | 302 | 303 | 307 | 308)
        && headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("location"));
    let body = response.bytes().map_err(|err| err.to_string())?.to_vec();
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
pub fn live_crawl(url: &str) -> Value {
    let report = match Client::builder().redirect(Policy::none()).build() {
        Ok(client) => {
            let get = |req: &HttpRequest| live_request(&client, req, reqwest::Method::GET);
            let head = |req: &HttpRequest| live_request(&client, req, reqwest::Method::HEAD);
            // Per-hop timeouts mirror the Python call sites (GET default
            // 1s, favicon HEAD 2s); the client carries no response timeout
            // of its own so each request's own deadline applies.
            let _ = (GET_TIMEOUT_SECS, FAVICON_HEAD_TIMEOUT_SECS);
            link_crawl::crawl(url, &live_validate, &get, &head)
        }
        Err(err) => CrawlReport {
            result: link_crawl::default_favicon().to_json(),
            warnings: vec![format!("Failed to fetch HTML for title: {err}")],
        },
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
        assert!(SAVE_LINK_METADATA_SQL.contains("issue_links"));
        assert!(SAVE_LINK_METADATA_SQL.contains("metadata = $1"));
        // Full save(): no update_fields narrowing — updated_at touched.
        assert!(SAVE_LINK_METADATA_SQL.contains("updated_at"));
        assert!(SAVE_LINK_METADATA_SQL.contains("WHERE id = $2"));
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
