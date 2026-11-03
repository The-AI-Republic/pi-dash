//! HMAC-SHA256 webhook signature guard (stage 5, PIDASHCONV-436).
//!
//! Ports `verify_webhook_signature`
//! (`apps/api/pi_dash/utils/github_app_auth.py:201-210`), the auth check on
//! `POST /integrations/github/app/webhook/` (`github.py:873-874`).
//! Fixture id: FX-GHA-01 `hmac_vectors`
//! (`rust-api/fixtures/app_integrations/fx-gha-01-app-flow.json`, executed
//! against real `hmac`/`hashlib`).
//!
//! Python source:
//!
//! ```python
//! expected = "sha256=" + hmac.new(
//!     config["webhook_secret"].encode("utf-8"), raw_body, hashlib.sha256,
//! ).hexdigest()
//! return hmac.compare_digest(expected, signature)
//! ```
//!
//! with the early `False` when the header is missing or lacks the
//! `sha256=` prefix (`github_app_auth.py:203-204`). The missing-config
//! branch (`require_github_app_config(webhook=True)` raising
//! `GithubAppConfigError`, answered 409 by the handler) is the caller's
//! job: this module takes the already-resolved secret, so there is no
//! config lookup to port here.
//!
//! The comparison itself never early-exits: [`constant_time_eq`] folds the
//! length difference and every byte difference into one accumulator, the
//! same timing discipline as `hmac.compare_digest` on ASCII strings.

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Header prefix pinning the digest algorithm (`github_app_auth.py:203`).
pub const SIGNATURE_PREFIX: &str = "sha256=";

/// Render the expected signature header for one secret + raw wire body:
/// `"sha256=" + hex(HMAC-SHA256(secret, body))`.
///
/// The MAC runs over the **wire bytes** (`request.body`), not a
/// re-serialization — the GitHub-side equivalent of fixture B4's warning
/// about `json.dumps` separators.
pub fn signature_for_secret(secret: &str, raw_body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(raw_body);
    format!(
        "{SIGNATURE_PREFIX}{}",
        hex::encode(mac.finalize().into_bytes())
    )
}

/// Mirror of `verify_webhook_signature`: `false` for a missing header or a
/// header without the `sha256=` prefix, otherwise a constant-time compare
/// against the expected value.
pub fn verify_webhook_signature(secret: &str, raw_body: &[u8], signature: Option<&str>) -> bool {
    let Some(presented) = signature else {
        return false;
    };
    if !presented.starts_with(SIGNATURE_PREFIX) {
        return false;
    }
    let expected = signature_for_secret(secret, raw_body);
    constant_time_eq(expected.as_bytes(), presented.as_bytes())
}

/// Constant-time byte comparison with no early exit: length and content
/// differences both fold into `diff`, which is tested once at the end.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u32;
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= u32::from(x ^ y);
    }
    diff == 0
}

/// Render one byte slice as lowercase hex (no `hex` crate in the tree).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let bytes = bytes.as_ref();
        let mut out = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            out.push(DIGITS[(byte >> 4) as usize] as char);
            out.push(DIGITS[(byte & 0x0f) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec-test";
    const BODY: &[u8] = br#"{"zen":"keep it simple"}"#;
    /// FX-GHA-01 `hmac_vectors.sample` (executed against real hmac/hashlib;
    /// re-confirmed with an independent `hashlib` run in the port commit).
    const SAMPLE: &str = "sha256=133e9393896b910ee36321e21c787ba11898b9ed3a71fb4abda214ed853311b3";

    #[test]
    fn renders_the_fixture_vector() {
        assert_eq!(signature_for_secret(SECRET, BODY), SAMPLE);
    }

    #[test]
    fn accepts_the_fixture_vector() {
        assert!(verify_webhook_signature(SECRET, BODY, Some(SAMPLE)));
    }

    #[test]
    fn rejects_tampered_body() {
        assert!(!verify_webhook_signature(
            SECRET,
            br#"{"zen":"keep it complex"}"#,
            Some(SAMPLE)
        ));
    }

    #[test]
    fn rejects_missing_header() {
        assert!(!verify_webhook_signature(SECRET, BODY, None));
    }

    #[test]
    fn rejects_wrong_prefix() {
        // `sha1=`-style header: the Python `startswith("sha256=")` guard.
        assert!(!verify_webhook_signature(
            SECRET,
            BODY,
            Some("sha1=133e9393")
        ));
    }

    #[test]
    fn rejects_wrong_secret() {
        assert!(!verify_webhook_signature(
            "other-secret",
            BODY,
            Some(SAMPLE)
        ));
    }

    #[test]
    fn compare_has_no_early_exit() {
        // Equal length, differ in the last byte only: a memcmp-style
        // early exit would still return the right answer, so pin the
        // timing-relevant property instead — unequal lengths also deny.
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"a", b"b"));
    }
}
