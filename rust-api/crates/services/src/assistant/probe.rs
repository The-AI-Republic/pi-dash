//! Provider connection probe (OVL-07, PIDASHCONV-1545).
//!
//! The non-HTTP half of `UserLLMConfigTestEndpoint._run_test`
//! (`assistant/views/llm_config.py:160-185`), moved out of
//! `pidash_api::assistant::llm_config` so the cloud overlay's config
//! twin (`rust-cloud/.../assistant/config.rs`, which copied
//! `classify_probe` and `probe_provider` line for line) calls it
//! instead.
//!
//! Moved verbatim: the minimal-prompt wire shape, the Anthropic
//! branch ignoring `base_url` exactly like `build_model`
//! (`runtime/llm.py:99-103`), the non-following client, the 60s
//! timeout, and the keyword classifier with its branch order.

use pidash_types::assistant::serializers;

/// Outcome of the provider probe: success, a classified failure with its
/// `(code, detail)`, or an unexpected transport failure (the outer
/// `except Exception`, `llm_config.py:101-102`).
#[derive(Debug, PartialEq)]
pub enum ProbeOutcome {
    Ok,
    Classified(&'static str, &'static str),
    TransportFailure,
}

/// Classify a probe HTTP round-trip the way `_run_test`
/// (`llm_config.py:160-185`) classifies the SDK exception text: 2xx is
/// success; otherwise the lowercased `"<status> <body>"` is scanned with
/// the same three keyword groups in the same order, defaulting to
/// `provider_unreachable`.
pub fn classify_probe(status: u16, body: &str) -> ProbeOutcome {
    if (200..300).contains(&status) {
        return ProbeOutcome::Ok;
    }
    let text = format!("{status} {}", body.to_lowercase());
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["401", "unauthorized", "api key", "authentication"]) {
        ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
    } else if has(&["connection", "timeout", "unreachable", "resolve"]) {
        ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
    } else if has(&["model", "not found", "does not exist"]) {
        ProbeOutcome::Classified("model_invalid", "Model not accepted by the provider.")
    } else {
        ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
    }
}

/// Probe the configured provider with a single minimal prompt
/// (`agent.run("Reply with the single word: ok", request_limit=1)`,
/// `llm_config.py:168-172`). The Anthropic branch ignores `base_url`
/// exactly like `build_model` (`runtime/llm.py:99-103`); everything else
/// posts to `{base_url}/chat/completions`. Only the response class and
/// the keyword scan above are observed — bodies are never surfaced.
pub async fn probe_provider(
    provider_kind: &str,
    base_url: &str,
    model_name: &str,
    api_key: &str,
) -> ProbeOutcome {
    // No redirect following: pydantic-ai builds its SDK clients over its
    // own non-following httpx client (`create_async_http_client`), so a
    // 302 is observed as-is by `_run_test`, not chased.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("probe client builds");
    let result = if provider_kind == serializers::PROVIDER_ANTHROPIC {
        client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .body(
                serde_json::to_string(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": "Reply with the single word: ok"}],
                    "max_tokens": 16,
                }))
                .expect("probe body serializes"),
            )
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
    } else {
        client
            .post(format!(
                "{}/chat/completions",
                base_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .body(
                serde_json::to_string(&serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": "Reply with the single word: ok"}],
                }))
                .expect("probe body serializes"),
            )
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
    };
    match result {
        Err(_) => ProbeOutcome::TransportFailure,
        Ok(response) => {
            let status = response.status().as_u16();
            match response.text().await {
                Ok(body) => classify_probe(status, &body),
                Err(_) => ProbeOutcome::TransportFailure,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_classification_matches_run_test_order() {
        assert_eq!(classify_probe(200, "ok"), ProbeOutcome::Ok);
        assert_eq!(
            classify_probe(401, r#"{"error":{"message":"Incorrect API key"}}"#),
            ProbeOutcome::Classified("provider_auth_failed", "API key rejected.")
        );
        assert_eq!(
            classify_probe(404, r#"{"error":{"message":"model not found"}}"#),
            ProbeOutcome::Classified("model_invalid", "Model not accepted by the provider.")
        );
        assert_eq!(
            classify_probe(500, "boom"),
            ProbeOutcome::Classified("provider_unreachable", "Could not reach the endpoint.")
        );
    }
}
