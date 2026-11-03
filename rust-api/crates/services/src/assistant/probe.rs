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
//!
//! TLS roots (PIDASHCONV-1599): Python runs the probe over
//! pydantic-ai's default httpx client, whose SSL context reads
//! `SSL_CERT_FILE`, and the deleted cloud twin added the
//! `SSL_CERT_FILE` / `REQUESTS_CA_BUNDLE` bundle to its shared
//! client (PIDASHCONV-1021) — so the probe client adds the bundle's
//! roots to the baked-in webpki roots instead of building bare.
//! Unset env builds exactly the bare client.

use pidash_types::assistant::serializers;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// Extra root-CA bundle, when the operator points at one:
/// `SSL_CERT_FILE` first (the `ssl` module default httpx honors),
/// `REQUESTS_CA_BUNDLE` as the fallback. Empty reads as unset (the
/// cloud twin's configured-URL precedent, PIDASHCONV-1021).
pub fn ca_bundle_path() -> Option<PathBuf> {
    select_ca_bundle(
        env::var_os("SSL_CERT_FILE"),
        env::var_os("REQUESTS_CA_BUNDLE"),
    )
}

/// Pure precedence half of [`ca_bundle_path`] (the unit-test seam).
fn select_ca_bundle(
    ssl_cert_file: Option<OsString>,
    requests_ca_bundle: Option<OsString>,
) -> Option<PathBuf> {
    for candidate in [ssl_cert_file, requests_ca_bundle] {
        if let Some(path) = candidate.filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Parse the bundle at `path` into extra roots. `None` on a
/// missing, unparsable, or cert-less file (`from_pem_bundle` skips
/// non-PEM bytes, so garbage parses to zero certs): `_run_test`
/// catches everything (`llm_config.py:101-102`), so a bad bundle
/// surfaces as `TransportFailure` (`provider_unreachable`), never a
/// 500 — unlike the cloud twin's startup statics, which fail loud
/// (PIDASHCONV-1021).
fn load_bundle_certs(path: &Path) -> Option<Vec<reqwest::Certificate>> {
    let pem = std::fs::read(path).ok()?;
    let certs = reqwest::Certificate::from_pem_bundle(&pem).ok()?;
    (!certs.is_empty()).then_some(certs)
}

/// Base builder for the probe client: the bare webpki-roots builder
/// plus the [`ca_bundle_path`] roots when set. Callers add their own
/// options (redirect policy) before building.
pub fn probe_client_builder() -> Option<reqwest::ClientBuilder> {
    let bundle = ca_bundle_path();
    probe_client_builder_with_bundle(bundle.as_deref())
}

/// [`probe_client_builder`] with an explicit bundle path (the
/// unit-test seam: no process env involved). `None` returns the bare
/// builder — exactly today's client. A set-but-unreadable bundle
/// returns `None` (the probe maps it to `TransportFailure`) instead
/// of handshaking against the wrong roots.
pub fn probe_client_builder_with_bundle(bundle: Option<&Path>) -> Option<reqwest::ClientBuilder> {
    let mut builder = reqwest::Client::builder();
    if let Some(path) = bundle {
        for cert in load_bundle_certs(path)? {
            builder = builder.add_root_certificate(cert);
        }
    }
    Some(builder)
}

/// Probe the configured provider with a single minimal prompt
/// (`agent.run("Reply with the single word: ok", request_limit=1)`,
/// `llm_config.py:168-172`). The Anthropic branch ignores `base_url`
/// exactly like `build_model` (`runtime/llm.py:99-103`); everything else
/// posts to `{base_url}/chat/completions`. Only the response class and
/// the keyword scan above are observed — bodies are never surfaced.
/// The client adds the [`ca_bundle_path`] roots to the webpki roots.
pub async fn probe_provider(
    provider_kind: &str,
    base_url: &str,
    model_name: &str,
    api_key: &str,
) -> ProbeOutcome {
    probe_provider_with_bundle(
        provider_kind,
        base_url,
        model_name,
        api_key,
        ca_bundle_path().as_deref(),
    )
    .await
}

/// [`probe_provider`] with an explicit bundle path (the unit-test
/// seam: no process env involved).
pub async fn probe_provider_with_bundle(
    provider_kind: &str,
    base_url: &str,
    model_name: &str,
    api_key: &str,
    bundle: Option<&Path>,
) -> ProbeOutcome {
    // No redirect following: pydantic-ai builds its SDK clients over its
    // own non-following httpx client (`create_async_http_client`), so a
    // 302 is observed as-is by `_run_test`, not chased.
    let Some(builder) = probe_client_builder_with_bundle(bundle) else {
        return ProbeOutcome::TransportFailure;
    };
    let client = builder
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
    use std::env;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    // --- PIDASHCONV-1599: the probe honors the operator CA bundle ---
    //
    // Python runs the probe over pydantic-ai's default httpx client,
    // whose SSL context reads `SSL_CERT_FILE`; the cloud twin honored
    // `SSL_CERT_FILE` / `REQUESTS_CA_BUNDLE` through its shared client
    // (PIDASHCONV-1021). The tests below pin the same for the SDK probe:
    // a private-CA endpoint probes `Ok` with the bundle and
    // `TransportFailure` without it.

    /// Fixture CA (self-signed `openssl req -x509`, valid to
    /// 2076-10-15): the bundle file the tests point the CA env vars at.
    /// Static, not minted per run: the `openssl` crate the cloud twin
    /// generates with is not in this crate's tree, and bundle parsing
    /// only decodes (expiry is checked at the handshake, where 2076 is
    /// safely far off).
    const TEST_CA_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIDMjCCAhqgAwIBAgIUA0XBu624XBHp5qZg4g+W61LSqdEwDQYJKoZIhvcNAQEL\nBQAwHjEcMBoGA1UEAwwTcGlkYXNoLTE1OTktdGVzdC1jYTAgFw0yNjEwMjgxNzEx\nMjNaGA8yMDc2MTAxNTE3MTEyM1owHjEcMBoGA1UEAwwTcGlkYXNoLTE1OTktdGVz\ndC1jYTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAKTqXC9Laxx0k5yD\nX53NXATLa9Wj7v0iW9SUhR0kUtg4e+Ht7vswm6wnWqYLj9ZyDxEnDknlfAPNIwri\nWVXcl1ulLzLwMuOSWPZSKUPlOIjkLE/IlTirjYjWAPX+rkkMbIkvKZieS7o51Moo\nrocypfWMsWYn48bPCr1nc4I4TNKsmo0bSwAFZ6bJHhuPi3t1BDVRBwkck7RheVc+\nm8+UWRIT89kJt6S+NZcT0YK9Qexl4Xgo568JgojvGjHbiHs2j+yNOHcrmmy2I3Ri\nm+YAd0dFrOcXmigoA/KSn8KJbir8mt6oFg82LJvBUE0UegFfLyC7hA1P3AQmTp2E\nIylmVnMCAwEAAaNmMGQwHQYDVR0OBBYEFA5IZsWhoO7okgJJZ9Ep+sH8UisLMB8G\nA1UdIwQYMBaAFA5IZsWhoO7okgJJZ9Ep+sH8UisLMBIGA1UdEwEB/wQIMAYBAf8C\nAQAwDgYDVR0PAQH/BAQDAgEGMA0GCSqGSIb3DQEBCwUAA4IBAQAu9Nc0CVXvV2Pf\no07wPXyEVo3x60NO8egDMJBZD/25Ruh5ztOOIxdzxYVfmyhZKWrtaru9V4CfXHEy\nFPgbRErFceDA1dSnjfWy87lGTZAQNmILVNCrTzxXVv7hRLPhwyEAVpp8hIQt7NED\nyVAQv+93DXG9bkHrk5s/8VZGh/IyqFjj9kvTb3QMrLAny3PeIMgg8HZLXQ0l8nie\n0uxJVY/JF6MF9/Jr22TIzTV7+7mFYa8BS7WJtVNs78NXXtYt/pcIUjVjbqh0KgJz\nJFrbObmQV2fpYfq+DZ0/CHuGZYafpbzEr0bZmuKZHisFOzdCfA5hcCi74FxHKbpU\nVlUL7Jhv\n-----END CERTIFICATE-----\n";

    /// Fixture leaf for the TLS fake (signed by the fixture CA, SAN
    /// `IP:127.0.0.1` + `DNS:localhost`, serverAuth, valid to
    /// 2076-10-15).
    const TEST_LEAF_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIDVTCCAj2gAwIBAgIUQVxL99JrmGbWGaCwda2+5OPqKDEwDQYJKoZIhvcNAQEL\nBQAwHjEcMBoGA1UEAwwTcGlkYXNoLTE1OTktdGVzdC1jYTAgFw0yNjEwMjgxNzEx\nMjNaGA8yMDc2MTAxNTE3MTEyM1owFDESMBAGA1UEAwwJMTI3LjAuMC4xMIIBIjAN\nBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAk/iwIKWehji3xRl2Hb+Lcefl5HsM\nTZr3gQnMuGVW6zrK/jlr6X0+wxuPupnlE3WPRWwBzqAgLbC2N518OM+S0IwYANRO\nF8yuDlfMd/GHk6E6ZnOi9Hr89jhXoYCpFvoHp78w+GcEM8Or36fyw6hjY7JXocS4\nA7RfGK+eDDnKlmUs41d2e9LEAq2dsCkJpeJg/qER+n9UAE2kjsaJpElDDZ3JJ8T9\n3aI5TfG4abucWdHsgAU02lvCxTkKLOISZWRCYVkaHywDezbNAlu9oHk4KfshLIt/\nnD5p/SV35ErUlB/eLrN1wnrv4wnUk4vG88om/c/hhUpuGmG8uLx7uZEVwwIDAQAB\no4GSMIGPMAwGA1UdEwEB/wQCMAAwDgYDVR0PAQH/BAQDAgeAMBMGA1UdJQQMMAoG\nCCsGAQUFBwMBMBoGA1UdEQQTMBGHBH8AAAGCCWxvY2FsaG9zdDAdBgNVHQ4EFgQU\ninoxCu5MUQUlHQzR4AuCwhTVfzAwHwYDVR0jBBgwFoAUDkhmxaGg7uiSAkln0Sn6\nwfxSKwswDQYJKoZIhvcNAQELBQADggEBAEzF+9hTT45PJVuVeFe/x1rY6NJjUeMG\nq2D1ZO5c3Hn+2ia+1DX3ClJZ8Q+Zs7OtXtaLylwECzFKeFZZ0XsXn/pquiVowswZ\nkmOdo+1AxNp0VFcsXTbMSn3RO0x6miAP/mg6SUfXnBLRow9IqoXWFu2It/I46JMA\nHsalAKosLd5Fv/qFWqn8IiBbuClbld++Z7BEtFUoQISrvFJ/4EyakJajpJ71nR+f\nBfvVrTbvNWxnI54xmfnGU0NIhcikqOUm9lOR+/t7p7BdBQ/nUWHVp7ZVQjZ97MTe\n1L/7oDHLRstVvVWO17JSa0hox/PMHbiQtIGhQxIMCjgt2j9X31mJJiM=\n-----END CERTIFICATE-----\n";

    /// Fixture leaf key (PKCS#8) for the TLS fake.
    const TEST_LEAF_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCT+LAgpZ6GOLfF\nGXYdv4tx5+XkewxNmveBCcy4ZVbrOsr+OWvpfT7DG4+6meUTdY9FbAHOoCAtsLY3\nnXw4z5LQjBgA1E4XzK4OV8x38YeToTpmc6L0evz2OFehgKkW+genvzD4ZwQzw6vf\np/LDqGNjslehxLgDtF8Yr54MOcqWZSzjV3Z70sQCrZ2wKQml4mD+oRH6f1QATaSO\nxomkSUMNncknxP3dojlN8bhpu5xZ0eyABTTaW8LFOQos4hJlZEJhWRofLAN7Ns0C\nW72geTgp+yEsi3+cPmn9JXfkStSUH94us3XCeu/jCdSTi8bzyib9z+GFSm4aYby4\nvHu5kRXDAgMBAAECggEAMCbQtcb/Z6EkWBzZfYO77nSusGPEVKq1mQWl22mK1+BZ\nd1IbRi9uEZKx+mCjFWp/lXE4w1MDNasYGNAEQP53QasdEoQD56XBDETQ8kUkP8vL\nE9TUdedh0wPxrEoQfDwf6pwCZQss6GVn1E+0Zawxr5s0/hzkXrw+5+saLNNPlBHY\n0kozYnXkhyEApdcEN35jvn16LO4a5FY05UU07dUBpk2KiHF68lsudi9W11KFwNWE\nXeSbS8FgAcouQUYPK5oQeZYlaKp4rTtOaRraBH8gKjf0a/yPgxPBb4FLxBUjbPbp\nq7s3ab5+5f4vO2yzfCFy+FJ80n56x7+tS4DCs+Bx5QKBgQDFsaatZdrfdKqrvEGh\ny5T9cFyTOMr6nDxjG2/ampuQ7B7cebKmS2oo2/Y9oADpYQoHq6UeQAVntcuJHfGu\nahnwovgE8QRLf2sIeNoWQqQdvGQL/f7DJZHgd4+P+GhdFX/WtWhstrvsWgh2npCi\nSG+n1qmiVFzi6R4mPrefx27QbQKBgQC/nN9k8qM/cQu9mZEoSUhWopHElYn6TZnc\n1aeltFBdu+3RadXsCu2Zvy/9ell5Gl1F9z2nKrYmNJJHAiCM94ASVOBAY/CXyW2T\n7bq7B9jgYqc56g/g0lHzNN/8AuWr9MlyY+vu4kxHg1m8k3HQeGwG7+oKMcIwRD4b\nhmyeML6A7wKBgEVkYjkjM3yUGwRFBFSifR2ovxX+LWFiTeAL1WtKMajg+MQzA35T\nomPNMdXXv/orUbX3jfHcTwlkSplKpXavT4Oj8KLrZrKJeyxyHzsJEUpOwl9jPHWL\nTnUYApBqhVeHIGRhMNJYIgaIY8gfSG1uN6MWOlz8d84+PZL3GMVA36jRAoGADIu1\n6MMmQyograpXYb7imKGOT8PGw/e+wkHO9z+GH40FdjeqxQi2I+Hrv3n6T76VKtaU\nV9SpFnlMto+CGBjAbvLVuAGTKC0NGqlvF33dClwLPPSBU6QRr43fuH1sEkU1x2KZ\neQuiGPCVuI4i3pZz5ERjG4XEhCqDsoOrqvPgFUsCgYEAv5O0Sx6eP6Pp0qtyavY7\nX2PK6XBzamzgTAUnp0DGxxZ5uatkxMOBOcP/vWsna2cm+Ji7L+zFsrOIGL0zk0w0\nl6h8+SmsICSiSDEPiHhUrrc0GVnnUEn7Xdo/F5yuhkXxHqRNV0lNcSxLzcYWM0hy\nM0450pR1Zlsh6p/5UEruYkE=\n-----END PRIVATE KEY-----\n";

    /// Minimal HTTPS fake: one canned `200 {"ok":true}` over TLS with
    /// the fixture leaf, plain `TcpListener` + `tokio-rustls` (no
    /// hyper edge). The accept loop serves until the fake drops.
    struct TlsFake {
        port: u16,
        task: tokio::task::JoinHandle<()>,
    }

    impl TlsFake {
        async fn start() -> Self {
            use rustls_pki_types::pem::PemObject;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let certs =
                vec![
                    rustls_pki_types::CertificateDer::from_pem_slice(TEST_LEAF_PEM.as_bytes())
                        .expect("leaf parses"),
                ];
            let key = rustls_pki_types::PrivateKeyDer::from_pem_slice(TEST_LEAF_KEY_PEM.as_bytes())
                .expect("leaf key parses");
            // Explicit provider: the test process installs no global
            // `CryptoProvider`, so `ServerConfig::builder()` would panic.
            let provider = rustls::crypto::ring::default_provider();
            let config = rustls::ServerConfig::builder_with_provider(provider.into())
                .with_protocol_versions(&[&rustls::version::TLS13])
                .expect("tls13")
                .with_no_client_auth()
                .with_single_cert(certs, key)
                .expect("server cert");
            let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("fake binds");
            let port = listener.local_addr().expect("fake addr").port();
            let task = tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    let Ok(tls) = acceptor.accept(stream).await else {
                        continue;
                    };
                    let (mut reader, mut writer) = tokio::io::split(tls);
                    // Read the request head, then the declared body,
                    // then answer the canned 200 (the probe posts JSON
                    // and only observes the status class).
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") && head.len() <= 65536 {
                        if reader.read_exact(&mut byte).await.is_err() {
                            break;
                        }
                        head.push(byte[0]);
                    }
                    let mut content_length = 0usize;
                    for line in String::from_utf8_lossy(&head).lines() {
                        if let Some(value) = line.strip_prefix("content-length:") {
                            content_length = value.trim().parse().unwrap_or(0);
                        } else if let Some(value) = line.strip_prefix("Content-Length:") {
                            content_length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut body).await;
                    const CANNED_BODY: &str = "{\"ok\":true}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{CANNED_BODY}",
                        CANNED_BODY.len()
                    );
                    let _ = writer.write_all(response.as_bytes()).await;
                    let _ = writer.shutdown().await;
                }
            });
            Self { port, task }
        }

        fn base_url(&self) -> String {
            format!("https://127.0.0.1:{}", self.port)
        }
    }

    impl Drop for TlsFake {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// The PEM at a unique temp path (no `tempfile` in dev-deps).
    fn write_temp_pem(tag: &str, pem: &[u8]) -> PathBuf {
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("pidash-1599-{}-{tag}-{id}.pem", std::process::id()));
        std::fs::write(&path, pem).expect("write temp pem");
        path
    }

    /// Serializes the env-touching tests: process env is global, and
    /// the test harness runs threads in parallel.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores the CA env vars on drop, even when an assert panics.
    struct EnvRestore {
        ssl_cert_file: Option<OsString>,
        requests_ca_bundle: Option<OsString>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            Self {
                ssl_cert_file: env::var_os("SSL_CERT_FILE"),
                requests_ca_bundle: env::var_os("REQUESTS_CA_BUNDLE"),
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore("SSL_CERT_FILE", self.ssl_cert_file.take());
            restore("REQUESTS_CA_BUNDLE", self.requests_ca_bundle.take());
        }
    }

    fn restore(var: &str, value: Option<OsString>) {
        match value {
            Some(value) => env::set_var(var, value),
            None => env::remove_var(var),
        }
    }

    #[test]
    fn ssl_cert_file_wins_over_requests_ca_bundle() {
        assert_eq!(
            select_ca_bundle(
                Some(OsString::from("/ssl.pem")),
                Some(OsString::from("/requests.pem")),
            ),
            Some(PathBuf::from("/ssl.pem")),
        );
    }

    #[test]
    fn requests_ca_bundle_is_the_fallback() {
        assert_eq!(
            select_ca_bundle(None, Some(OsString::from("/requests.pem"))),
            Some(PathBuf::from("/requests.pem")),
        );
        assert_eq!(
            select_ca_bundle(
                Some(OsString::from("")),
                Some(OsString::from("/requests.pem"))
            ),
            Some(PathBuf::from("/requests.pem")),
        );
    }

    #[test]
    fn unset_or_empty_reads_as_no_bundle() {
        assert_eq!(select_ca_bundle(None, None), None);
        assert_eq!(
            select_ca_bundle(Some(OsString::from("")), Some(OsString::from(""))),
            None,
        );
    }

    #[test]
    fn bundle_file_parses_and_builds_a_client() {
        let path = write_temp_pem("ca", TEST_CA_PEM.as_bytes());
        let client = probe_client_builder_with_bundle(Some(&path))
            .expect("bundle builder")
            .build()
            .expect("client with bundle builds");
        drop(client);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn unset_bundle_builds_the_bare_client() {
        // `None` runs the bare `Client::builder()` with no added
        // roots — structurally exactly today's client.
        let client = probe_client_builder_with_bundle(None)
            .expect("bare builder")
            .build()
            .expect("bare client builds");
        drop(client);
    }

    #[test]
    fn missing_bundle_file_is_no_builder() {
        let path =
            std::env::temp_dir().join(format!("pidash-1599-{}-missing.pem", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(probe_client_builder_with_bundle(Some(&path)).is_none());
    }

    #[test]
    fn unparsable_bundle_file_is_no_builder() {
        let path = write_temp_pem("junk", b"not a pem bundle");
        assert!(probe_client_builder_with_bundle(Some(&path)).is_none());
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn env_vars_flow_into_the_bundle_path() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _restore = EnvRestore::capture();

        env::remove_var("SSL_CERT_FILE");
        env::remove_var("REQUESTS_CA_BUNDLE");
        assert_eq!(ca_bundle_path(), None);

        env::set_var("REQUESTS_CA_BUNDLE", "/requests.pem");
        assert_eq!(ca_bundle_path(), Some(PathBuf::from("/requests.pem")));

        env::set_var("SSL_CERT_FILE", "/ssl.pem");
        assert_eq!(ca_bundle_path(), Some(PathBuf::from("/ssl.pem")));
    }

    #[tokio::test]
    async fn private_ca_endpoint_probes_ok_with_bundle() {
        let fake = TlsFake::start().await;
        let bundle = write_temp_pem("ca", TEST_CA_PEM.as_bytes());
        let outcome =
            probe_provider_with_bundle("openai", &fake.base_url(), "model", "key", Some(&bundle))
                .await;
        std::fs::remove_file(&bundle).expect("cleanup");
        assert_eq!(outcome, ProbeOutcome::Ok);
    }

    #[tokio::test]
    async fn private_ca_endpoint_is_unreachable_without_bundle() {
        // Negative control: without the bundle the fake's chain does
        // not verify, so the probe reports `TransportFailure` — the
        // pre-fix behavior for every private-CA endpoint.
        let fake = TlsFake::start().await;
        let outcome =
            probe_provider_with_bundle("openai", &fake.base_url(), "model", "key", None).await;
        assert_eq!(outcome, ProbeOutcome::TransportFailure);
    }

    #[tokio::test]
    async fn bad_bundle_file_probes_unreachable_not_panic() {
        // Python's `_run_test` catches everything, so a set-but-bad
        // bundle surfaces as `provider_unreachable`, never a 500.
        let fake = TlsFake::start().await;
        let missing =
            std::env::temp_dir().join(format!("pidash-1599-{}-missing.pem", std::process::id()));
        let _ = std::fs::remove_file(&missing);
        let outcome =
            probe_provider_with_bundle("openai", &fake.base_url(), "model", "key", Some(&missing))
                .await;
        assert_eq!(outcome, ProbeOutcome::TransportFailure);
    }

    #[tokio::test]
    // The env guard must span the probe awaits (a parallel test could
    // otherwise interleave an env change mid-test). Deadlock-free: one
    // global lock, never nested, and each async test runs on its own
    // runtime thread, so the hold only serializes.
    #[allow(clippy::await_holding_lock)]
    async fn private_ca_endpoint_probes_ok_with_bundle_env() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let _restore = EnvRestore::capture();
        let fake = TlsFake::start().await;
        let bundle = write_temp_pem("ca", TEST_CA_PEM.as_bytes());
        env::remove_var("SSL_CERT_FILE");
        env::remove_var("REQUESTS_CA_BUNDLE");

        // End to end through the real constructor: cleared env does
        // not verify the fake; either var pointing at the bundle does;
        // a set-but-bad `SSL_CERT_FILE` wins over a good
        // `REQUESTS_CA_BUNDLE` and probes unreachable.
        let base_url = fake.base_url();
        assert_eq!(
            probe_provider("openai", &base_url, "model", "key").await,
            ProbeOutcome::TransportFailure,
        );
        env::set_var("REQUESTS_CA_BUNDLE", &bundle);
        assert_eq!(
            probe_provider("openai", &base_url, "model", "key").await,
            ProbeOutcome::Ok,
        );
        env::set_var("SSL_CERT_FILE", &bundle);
        assert_eq!(
            probe_provider("openai", &base_url, "model", "key").await,
            ProbeOutcome::Ok,
        );
        env::set_var("SSL_CERT_FILE", "/nonexistent-pidash-1599.pem");
        assert_eq!(
            probe_provider("openai", &base_url, "model", "key").await,
            ProbeOutcome::TransportFailure,
        );
        // Restore the process env BEFORE deleting the bundle: other
        // tests construct probe clients off these vars concurrently,
        // and a set var naming a deleted file probes unreachable.
        drop(_restore);
        std::fs::remove_file(&bundle).expect("cleanup");
    }
}
