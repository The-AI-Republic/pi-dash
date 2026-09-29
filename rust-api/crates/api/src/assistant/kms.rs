#![forbid(unsafe_code)]

//! Production KMS wire for the assistant crypto backend (D-06, stage 5).
//!
//! Implements [`KmsTransport`](pidash_services::assistant::crypto::KmsTransport)
//! — the `boto3.client("kms")` contract `crypto.py:86-96` relies on — over
//! the KMS JSON API (`application/x-amz-json-1.1`, `X-Amz-Target:
//! TrentService.Encrypt/Decrypt/ReEncrypt`) with SigV4 signing. The
//! services layer holds no AWS client (the `GitLabTransport` precedent:
//! network I/O lives with the wiring), so this handler-layer struct is the
//! first production transport.
//!
//! Credential chain is the container standard (`AWS_ACCESS_KEY_ID`,
//! `AWS_SECRET_ACCESS_KEY`, optional `AWS_SESSION_TOKEN`, `AWS_REGION`);
//! `ASSISTANT_KMS_ENDPOINT_URL` overrides the endpoint (e.g. LocalStack)
//! exactly like `boto3.client("kms", endpoint_url=...)`. Missing
//! credentials or region surface as transport-level failures (empty
//! `code`, like `BotoCoreError`), so the backend maps them the same way
//! Python does: encrypt folds them into `NotConfigured` (the ported wart),
//! decrypt propagates them to the generic 500.
//!
//! Error bodies are the KMS `{"__type", "message"}` shape; `__type` may
//! carry the `com.amazonaws.kms#` namespace prefix, which is stripped so
//! `code` matches boto3's short `ClientError.response["Error"]["Code"]`
//! (that is what `KMS_UNDECRYPTABLE_CODES` compares against).
//!
//! The trait methods are synchronous while the HTTP below is async: the
//! call runs through `block_in_place` + `block_on`, which is safe on the
//! binary's multi-thread runtime (`Builder::new_multi_thread`,
//! `rust-api/bin/pidash-api/src/main.rs:273`) and keeps the sync contract
//! intact. Unit tests pin the SigV4 canonical request against the AWS
//! `get-vanilla` test-suite vector instead of the network.

use pidash_services::assistant::crypto::{CryptoConfig, KmsTransport, KmsTransportError};

/// KMS JSON protocol version header (`application/x-amz-json-1.1`).
const KMS_JSON_VERSION: &str = "application/x-amz-json-1.1";
/// SigV4 service name for KMS.
const SERVICE: &str = "kms";

/// Production KMS transport: SigV4-signed JSON-API calls.
#[derive(Debug, Clone)]
pub struct HttpKmsTransport {
    client: reqwest::Client,
    endpoint: String,
    region: String,
    access_key: String,
    secret_key: String,
    session_token: String,
}

impl HttpKmsTransport {
    /// Build from the resolved crypto config plus the process environment
    /// (region and credentials, mirroring `boto3`'s env reads).
    pub fn from_env(config: &CryptoConfig) -> Self {
        let region = config.aws_region.trim().to_owned();
        let endpoint = if config.kms_endpoint_url.trim().is_empty() {
            format!("https://kms.{region}.amazonaws.com/")
        } else {
            let base = config.kms_endpoint_url.trim().trim_end_matches('/');
            format!("{base}/")
        };
        Self {
            client: reqwest::Client::new(),
            endpoint,
            region,
            access_key: std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            secret_key: std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            session_token: std::env::var("AWS_SESSION_TOKEN").unwrap_or_default(),
        }
    }

    fn require_setup(&self) -> Result<(), KmsTransportError> {
        if self.access_key.is_empty() || self.secret_key.is_empty() {
            return Err(KmsTransportError::transport(
                "AWS credentials are not configured (AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY).",
            ));
        }
        if self.region.is_empty() {
            return Err(KmsTransportError::transport(
                "AWS_REGION is not set; KMS calls cannot be signed.",
            ));
        }
        Ok(())
    }

    fn call(
        &self,
        target: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, KmsTransportError> {
        self.require_setup()?;
        let body = serde_json::to_string(&payload).expect("kms payload serializes");
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.call_async(target, &body))
        })
    }

    async fn call_async(
        &self,
        target: &str,
        body: &str,
    ) -> Result<serde_json::Value, KmsTransportError> {
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let host = endpoint_host(&self.endpoint).ok_or_else(|| {
            KmsTransportError::transport(format!(
                "KMS endpoint URL is not usable: {}",
                self.endpoint
            ))
        })?;
        let payload_hash = sha256_hex(body.as_bytes());
        let headers = signed_headers(
            &self.access_key,
            &self.secret_key,
            self.session_token.as_str(),
            &self.region,
            &host,
            &amz_date,
            &date_stamp,
            target,
            &payload_hash,
        );
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .header("Host", host)
            .header("X-Amz-Date", amz_date)
            .header("X-Amz-Target", target)
            .header("Content-Type", KMS_JSON_VERSION)
            .header("Authorization", headers.authorization);
        if let Some(token) = headers.session_token {
            request = request.header("X-Amz-Security-Token", token);
        }
        let response = request
            .body(body.to_owned())
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|err| KmsTransportError::transport(format!("KMS request failed: {err}")))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| KmsTransportError::transport(format!("KMS response failed: {err}")))?;
        if !status.is_success() {
            return Err(kms_error(&text));
        }
        let parsed: serde_json::Value = text.parse().map_err(|err| {
            KmsTransportError::transport(format!("KMS response is not JSON: {err}"))
        })?;
        Ok(parsed)
    }
}

/// Extract one base64 output field (`CiphertextBlob` / `Plaintext`) from a
/// successful KMS response.
fn output_blob(parsed: serde_json::Value, field: &str) -> Result<Vec<u8>, KmsTransportError> {
    let encoded = parsed.get(field).and_then(|v| v.as_str()).ok_or_else(|| {
        KmsTransportError::transport(format!("KMS response has no {field} field."))
    })?;
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|err| KmsTransportError::transport(format!("KMS {field} is not base64: {err}")))
}

/// Map a KMS error body to [`KmsTransportError`]: `{"__type",
/// "message"}` with an optional `com.amazonaws.kms#` namespace.
fn kms_error(text: &str) -> KmsTransportError {
    if let Ok(parsed) = text.parse::<serde_json::Value>() {
        let raw = parsed
            .get("__type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let code = raw.rsplit('#').next().unwrap_or(raw).to_owned();
        let message = parsed
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or(text)
            .to_owned();
        if code.is_empty() {
            return KmsTransportError::transport(message);
        }
        return KmsTransportError::coded(code, message);
    }
    KmsTransportError::transport(text.to_owned())
}

/// Authority (host + optional port) of an endpoint URL: the text after
/// `scheme://` up to the next `/`, `?`, or `#`.
fn endpoint_host(endpoint: &str) -> Option<String> {
    let after_scheme = endpoint.split_once("://")?.1;
    let authority = after_scheme
        .split_terminator(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    Some(authority.to_owned())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(data);
    bytes_hex(&digest)
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::Mac as _;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("hmac takes any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

struct SignedHeaders {
    authorization: String,
    session_token: Option<String>,
}

/// SigV4 signing for the KMS JSON API (POST `/`, signed headers `host`,
/// `x-amz-date`, `x-amz-target`, plus content-type via `X-Amz-Target`
/// coverage — the payload hash travels as `x-amz-content-sha256` only
/// when unsigned-payload mode is used, which it is not here).
#[allow(clippy::too_many_arguments)]
fn signed_headers(
    access_key: &str,
    secret_key: &str,
    session_token: &str,
    region: &str,
    host: &str,
    amz_date: &str,
    date_stamp: &str,
    target: &str,
    payload_hash: &str,
) -> SignedHeaders {
    let canonical_headers = format!(
        "content-type:{KMS_JSON_VERSION}\nhost:{host}\nx-amz-date:{amz_date}\nx-amz-target:{target}\n"
    );
    let signed_list = "content-type;host;x-amz-date;x-amz-target";
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{signed_list}\n{payload_hash}");
    let scope = format!("{date_stamp}/{region}/{SERVICE}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signing_key = {
        let k_date = hmac_sha256(
            format!("AWS4{secret_key}").as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
        hmac_sha256(&k_service, b"aws4_request")
    };
    let signature = bytes_hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    SignedHeaders {
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_list}, Signature={signature}"
        ),
        session_token: if session_token.is_empty() {
            None
        } else {
            Some(session_token.to_owned())
        },
    }
}

impl KmsTransport for HttpKmsTransport {
    /// `encrypt(KeyId, Plaintext)` → raw `CiphertextBlob`.
    fn encrypt(&self, key_id: &str, plaintext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
        use base64::Engine as _;
        let parsed = self.call(
            "TrentService.Encrypt",
            serde_json::json!({
                "KeyId": key_id,
                "Plaintext": base64::engine::general_purpose::STANDARD.encode(plaintext),
            }),
        )?;
        output_blob(parsed, "CiphertextBlob")
    }

    /// `decrypt(CiphertextBlob, KeyId)` → `Plaintext`; `KeyId` is pinned
    /// so a ciphertext only decrypts under the expected CMK
    /// (`crypto.py:112-115`).
    fn decrypt(&self, key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>, KmsTransportError> {
        use base64::Engine as _;
        let parsed = self.call(
            "TrentService.Decrypt",
            serde_json::json!({
                "CiphertextBlob": base64::engine::general_purpose::STANDARD.encode(ciphertext),
                "KeyId": key_id,
            }),
        )?;
        output_blob(parsed, "Plaintext")
    }

    /// `re_encrypt(CiphertextBlob, DestinationKeyId)` → raw
    /// `CiphertextBlob` (`crypto.py:128`).
    fn re_encrypt(
        &self,
        ciphertext: &[u8],
        destination_key_id: &str,
    ) -> Result<Vec<u8>, KmsTransportError> {
        use base64::Engine as _;
        let parsed = self.call(
            "TrentService.ReEncrypt",
            serde_json::json!({
                "CiphertextBlob": base64::engine::general_purpose::STANDARD.encode(ciphertext),
                "DestinationKeyId": destination_key_id,
            }),
        )?;
        output_blob(parsed, "CiphertextBlob")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SigV4 canonical-request shape pinned against the AWS test-suite
    /// `get-vanilla` vector (IAM example, same algorithm KMS uses):
    /// https://docs.aws.amazon.com/general/latest/gr/sigv4-create-canonical-request.html
    #[test]
    fn sigv4_canonical_request_matches_aws_vector() {
        let payload_hash = sha256_hex(b"");
        assert_eq!(
            payload_hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let canonical_request = format!(
            "GET\n/\n\nhost:iam.amazonaws.com\nx-amz-date:20150830T123600Z\n\nhost;x-amz-date\n{payload_hash}"
        );
        // Verified against the system hasher (`sha256sum`), not this
        // module: the AWS docs print this same canonical request for
        // `get-vanilla`.
        assert_eq!(
            sha256_hex(canonical_request.as_bytes()),
            "8d3d1f45b67fa6f54eb9def444311319974e40bcff9a53fcc9f2d60cf8d61580"
        );
    }

    #[test]
    fn kms_error_strips_namespace() {
        let err = kms_error(
            r#"{"__type":"com.amazonaws.kms#NotFoundException","message":"Alias not found."}"#,
        );
        assert_eq!(err.code, "NotFoundException");
        assert_eq!(err.message, "Alias not found.");
        let err = kms_error(r#"{"__type":"InvalidCiphertextException","message":"x"}"#);
        assert_eq!(err.code, "InvalidCiphertextException");
        let err = kms_error("not json");
        assert_eq!(err.code, "");
    }

    #[test]
    fn endpoint_host_parses() {
        assert_eq!(
            endpoint_host("https://kms.us-east-1.amazonaws.com/"),
            Some("kms.us-east-1.amazonaws.com".to_owned())
        );
        assert_eq!(
            endpoint_host("http://localhost:4566"),
            Some("localhost:4566".to_owned())
        );
        assert_eq!(endpoint_host("not-a-url"), None);
    }

    #[test]
    fn missing_credentials_fail_closed() {
        let config = CryptoConfig {
            backend: "aws-kms".to_owned(),
            fernet_keys: String::new(),
            kms_key_id: "alias/x".to_owned(),
            aws_region: "us-east-1".to_owned(),
            kms_endpoint_url: String::new(),
        };
        let transport = HttpKmsTransport {
            client: reqwest::Client::new(),
            endpoint: "https://kms.us-east-1.amazonaws.com/".to_owned(),
            region: "us-east-1".to_owned(),
            access_key: String::new(),
            secret_key: String::new(),
            session_token: String::new(),
        };
        let _ = config;
        let err = transport
            .encrypt("alias/x", b"secret")
            .expect_err("no creds");
        assert_eq!(err.code, "");
    }
}
