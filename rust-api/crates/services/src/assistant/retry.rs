//! Provider retry policy (OVL-07, PIDASHCONV-1545).
//!
//! The single canonical copy of the `openai/_base_client.py` retry port:
//! `_should_retry` (`:773-807`), `_sleep_for_retry` /
//! `_calculate_retry_timeout`, and `_parse_retry_after_header`. Two
//! private copies existed — the turn streaming loop
//! (`pidash_api::turn::streaming`) and the D-11 send loop
//! (`pidash_jobs::oss_worker::model_runtime::provider`) — with identical
//! logic; both now use this module, and the cloud overlay's turn seam
//! (OVL-12) calls it instead of carrying a third copy.
//!
//! Moved verbatim: the `x-should-retry` override, the 408/409/429/5xx
//! set, the `(0, 60]` retry-after window with `retry-after-ms` first,
//! the exponential backoff with UUID jitter, and the HTTP-date fallback
//! (`email.utils.parsedate_tz` / `mktime_tz`: IMF-fixdate parses as
//! RFC 2822).

/// Retry budget for one provider request (both send loops).
pub const MAX_RETRIES: u32 = 2;
/// First backoff step, seconds.
pub const INITIAL_RETRY_DELAY_SECS: f64 = 0.5;
/// Backoff ceiling, seconds.
pub const MAX_RETRY_DELAY_SECS: f64 = 8.0;
/// Largest honoured `retry-after`, seconds.
pub const RETRY_AFTER_MAX_SECS: f64 = 60.0;

/// `_should_retry` (`openai/_base_client.py:773-807`): the
/// `x-should-retry` override wins, else 408/409/429/5xx retry.
pub fn should_retry_status(status: u16, headers: &reqwest::header::HeaderMap) -> bool {
    if let Some(flag) = headers
        .get("x-should-retry")
        .and_then(|value| value.to_str().ok())
    {
        if flag == "true" {
            return true;
        }
        if flag == "false" {
            return false;
        }
    }
    status == 408 || status == 409 || status == 429 || status >= 500
}

/// `_sleep_for_retry` / `_calculate_retry_timeout`: the parsed
/// `retry-after` wins when within `(0, 60]`, else exponential
/// backoff with jitter.
pub async fn sleep_for_retry(retries_taken: u32, headers: Option<&reqwest::header::HeaderMap>) {
    let mut seconds = retry_after_seconds(headers)
        .filter(|secs| secs.is_finite() && *secs > 0.0 && *secs <= RETRY_AFTER_MAX_SECS);
    if seconds.is_none() {
        let capped = retries_taken.min(1000) as f64;
        let backoff = (INITIAL_RETRY_DELAY_SECS * 2_f64.powf(capped)).min(MAX_RETRY_DELAY_SECS);
        seconds = Some((backoff * (1.0 - 0.25 * unit_random())).max(0.0));
    }
    if let Some(secs) = seconds {
        tokio::time::sleep(std::time::Duration::from_secs_f64(secs)).await;
    }
}

/// `_parse_retry_after_header`: `retry-after-ms` first, then
/// `retry-after` as seconds (floats honoured), then as an HTTP-date
/// (`email.utils.parsedate_tz` / `mktime_tz`: IMF-fixdate parses as
/// RFC 2822). Same port as the D-11 send loop.
pub fn retry_after_seconds(headers: Option<&reqwest::header::HeaderMap>) -> Option<f64> {
    let headers = headers?;
    if let Some(millis) = headers
        .get("retry-after-ms")
        .and_then(|value| value.to_str().ok())
        .and_then(|text| text.parse::<f64>().ok())
    {
        return Some(millis / 1000.0);
    }
    let text = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())?;
    if let Ok(seconds) = text.parse::<f64>() {
        return Some(seconds);
    }
    let when = chrono::DateTime::parse_from_rfc2822(text).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0);
    Some(when.timestamp() as f64 - now)
}

/// A uniform `[0, 1)` draw for retry jitter, from UUID randomness
/// (same source as the D-11 send loop).
fn unit_random() -> f64 {
    (uuid::Uuid::new_v4().as_u128() as f64) / (u128::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_parsing() {
        use reqwest::header::{HeaderMap, HeaderValue};
        assert_eq!(retry_after_seconds(None), None);
        let mut headers = HeaderMap::new();
        assert_eq!(retry_after_seconds(Some(&headers)), None);
        headers.insert("retry-after-ms", HeaderValue::from_static("1500"));
        assert_eq!(retry_after_seconds(Some(&headers)), Some(1.5));
        headers.remove("retry-after-ms");
        headers.insert("retry-after", HeaderValue::from_static("3"));
        assert_eq!(retry_after_seconds(Some(&headers)), Some(3.0));
        headers.insert("retry-after", HeaderValue::from_static("2.5"));
        assert_eq!(retry_after_seconds(Some(&headers)), Some(2.5));
    }
}
