#![forbid(unsafe_code)]

//! Runner token minting + key ring (`runner/services/tokens.py`).
//!
//! Ports the mint side only: prefixes, TTLs, [`MintedToken`] /
//! [`MintedEnrollment`] shapes (`:34-92`), the `_key_ring` list form +
//! derived-from-`SECRET_KEY` fallback + `_active_kid` (`:97-125`), and
//! [`mint_access_token`] claims + `kid` header (`:128-170`).
//! Fixture: D13-F3 (`fixtures/runner_enroll/tokens/tokens.golden.json`).
//!
//! Kernel reuse (never re-ported here): [`hash_token`] / [`fingerprint`]
//! live in [`pidash_auth::token`];
//! [`pidash_auth::jwt::decode_access_token`] is the decode side; ring
//! construction goes through [`KeyRing::new`] / [`KeyRing::dev_from_secret`].
//! The mint needs the active secret *bytes*, which the kernel does not
//! expose, so [`active_signing_key`] resolves `(kid, secret)` from the same
//! settings form (the derived branch mirrors `dev_from_secret` and is pinned
//! to the same fixture vector).

use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, KeyInit, Mac};
use pidash_auth::{
    jwt::{KeyEntry, KeyRing, ACCESS_TOKEN_ALG, ACCESS_TOKEN_ISS},
    token::{fingerprint, hash_token},
};
use rand::RngCore;
use serde::Serialize;
use sha2::Sha256;
use thiserror::Error as ThisError;

// ---- prefixes + TTLs (`tokens.py:34-40`) -----------------------------------

/// `apd_en_` — short-lived one-time enrollment tokens.
pub const ENROLLMENT_PREFIX: &str = "apd_en_";
/// `rt_` — legacy long-lived per-runner refresh tokens.
pub const REFRESH_TOKEN_PREFIX: &str = "rt_";
/// `mt_` — long-lived dev-machine credentials.
pub const MACHINE_TOKEN_PREFIX: &str = "mt_";
/// `apd_cs_` — legacy connection secrets (nothing new minted; tests only).
pub const CONNECTION_SECRET_PREFIX: &str = "apd_cs_";

/// `ENROLLMENT_TTL` (`timedelta(hours=1)`) in seconds.
pub const ENROLLMENT_TTL_SECS: i64 = 3600;
/// Default `settings.ACCESS_TOKEN_TTL_SECS` (`settings/common.py:480`).
pub const DEFAULT_ACCESS_TOKEN_TTL_SECS: i64 = 3600;

// ---- minted shapes (`tokens.py:57-92`) -------------------------------------

/// `MintedToken` (`:57-62`): raw secret + stored hash + public fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedToken {
    pub raw: String,
    pub hashed: String,
    pub fingerprint: String,
}

/// `MintedEnrollment` (`:64-66`): a minted token plus its expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedEnrollment {
    pub raw: String,
    pub hashed: String,
    pub fingerprint: String,
    pub expires_at: DateTime<Utc>,
}

/// `secrets.token_urlsafe(nbytes)`: urlsafe-base64 of `nbytes` CSPRNG bytes,
/// padding stripped — 24 bytes render 32 chars, 32 bytes render 43.
fn random_urlsafe(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf)
}

fn mint_with_prefix(prefix: &str, nbytes: usize, secret_key: &str) -> MintedToken {
    let raw = format!("{prefix}{}", random_urlsafe(nbytes));
    MintedToken {
        hashed: hash_token(&raw, secret_key),
        fingerprint: fingerprint(&raw),
        raw,
    }
}

/// `mint_enrollment_token` (`:69-76`). `now` is `timezone.now()`; the hash
/// and fingerprint go through the kernel with the caller's `SECRET_KEY`.
pub fn mint_enrollment_token(secret_key: &str, now: DateTime<Utc>) -> MintedEnrollment {
    let token = mint_with_prefix(ENROLLMENT_PREFIX, 24, secret_key);
    MintedEnrollment {
        raw: token.raw,
        hashed: token.hashed,
        fingerprint: token.fingerprint,
        // `now + ENROLLMENT_TTL` (sub-second precision kept: Django datetimes
        // carry microseconds into the stored/serialized value). Saturates
        // only past year 262143, where Python's unbounded ints would not.
        expires_at: now
            .checked_add_signed(Duration::seconds(ENROLLMENT_TTL_SECS))
            .unwrap_or(DateTime::<Utc>::MAX_UTC),
    }
}

/// `mint_refresh_token` (`:79-81`).
pub fn mint_refresh_token(secret_key: &str) -> MintedToken {
    mint_with_prefix(REFRESH_TOKEN_PREFIX, 32, secret_key)
}

/// `mint_machine_token` (`:84-86`).
pub fn mint_machine_token(secret_key: &str) -> MintedToken {
    mint_with_prefix(MACHINE_TOKEN_PREFIX, 32, secret_key)
}

/// `mint_connection_secret` (`:89-92`): legacy helper retained for tests.
pub fn mint_connection_secret(secret_key: &str) -> MintedToken {
    mint_with_prefix(CONNECTION_SECRET_PREFIX, 32, secret_key)
}

// ---- key ring (`tokens.py:97-125`) -----------------------------------------

/// One `settings.RUNNER_ACCESS_TOKEN_KEYS` row (`:112-118`): `{kid,
/// secret, status?}` with status defaulting to `"active"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConfig {
    pub kid: String,
    pub secret: String,
    pub status: Option<String>,
}

/// Misconfiguration errors. The message keeps Python's `RuntimeError` text
/// verbatim so logs match.
#[derive(Debug, Clone, PartialEq, Eq, ThisError)]
pub enum MintError {
    #[error("no active access-token signing key configured")]
    NoActiveKey,
}

fn is_active(status: Option<&str>) -> bool {
    // `entry.get("status", "active") == "active"`.
    status.unwrap_or("active") == "active"
}

/// Derived-ring secret: `hex(sha256("runner/access-token/" + SECRET_KEY))`
/// (`tokens.py:110`, `settings/common.py:592` documents the `[]` default).
/// Mirrors [`KeyRing::dev_from_secret`], which exposes no secret accessor;
/// both are pinned to the D13-F3 `derived_secret_for_fixed_key` vector.
fn derived_secret_hex(secret_key: &str) -> String {
    use sha2::Digest;
    let digest =
        Sha256::digest([b"runner/access-token/".as_slice(), secret_key.as_bytes()].concat());
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `_key_ring` (`:101-118`) built into the kernel ring the decode side
/// verifies against. A missing *or* empty (`if not keys` — note stock
/// settings ARE `[]`) list derives the single `"default"` key.
pub fn build_key_ring(configured: &[KeyConfig], secret_key: &str) -> KeyRing {
    if configured.is_empty() {
        return KeyRing::dev_from_secret(secret_key);
    }
    KeyRing::new(
        configured
            .iter()
            .map(|entry| KeyEntry {
                kid: entry.kid.clone(),
                // PyJWT receives the secret `str`; its HMAC key is the
                // UTF-8 bytes — the hex digest is NOT decoded.
                secret: entry.secret.as_bytes().to_vec(),
                active: is_active(entry.status.as_deref()),
            })
            .collect(),
    )
}

/// The key [`mint_access_token`] signs with: `_active_kid` (`:121-125`) +
/// its secret bytes from the ring. First `active` entry in list order wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningKey {
    pub kid: String,
    pub secret: Vec<u8>,
}

pub fn active_signing_key(
    configured: &[KeyConfig],
    secret_key: &str,
) -> Result<SigningKey, MintError> {
    if configured.is_empty() {
        return Ok(SigningKey {
            kid: "default".to_owned(),
            secret: derived_secret_hex(secret_key).into_bytes(),
        });
    }
    configured
        .iter()
        .find(|entry| is_active(entry.status.as_deref()))
        .map(|entry| SigningKey {
            kid: entry.kid.clone(),
            secret: entry.secret.as_bytes().to_vec(),
        })
        .ok_or(MintError::NoActiveKey)
}

// ---- access-token mint (`tokens.py:128-170`) --------------------------------

/// `AccessToken` (`:128-133`): raw JWT + expiry + signing `kid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessToken {
    pub raw: String,
    pub expires_at: DateTime<Utc>,
    pub kid: String,
}

/// PyJWT 2.12 header bytes: `{"alg","kid","typ"}` — sorted keys, compact
/// separators (`jwt/api_jws.py`, `sort_headers=True` default). Field order
/// below is the byte order.
#[derive(Debug, Clone, Serialize)]
struct JwtHeader<'a> {
    alg: &'a str,
    kid: &'a str,
    typ: &'a str,
}

/// Claim bytes in `mint_access_token` dict insertion order (`:154-162`):
/// `iss, sub, uid, wid, iat, exp, rtg` — the payload is NOT key-sorted.
#[derive(Debug, Clone, Serialize)]
struct AccessTokenPayload<'a> {
    iss: &'a str,
    sub: &'a str,
    uid: &'a str,
    wid: &'a str,
    iat: i64,
    exp: i64,
    rtg: i64,
}

/// `json.dumps(..., ensure_ascii=True)` over compact `serde_json` output:
/// `serde_json` already escapes `"`, `\` and control chars identically and
/// emits raw UTF-8 otherwise, so only DEL (`\u007f`) and non-ASCII
/// (lowercase `\uXXXX`, surrogate pairs above U+FFFF) need rewriting.
/// (Same helper as the invite-token mint; real claims are UUIDs/ints so
/// this is a no-op on every real input.)
fn ensure_ascii(compact_json: &str) -> String {
    let mut out = String::with_capacity(compact_json.len());
    for c in compact_json.chars() {
        if c.is_ascii() && c != '\u{7f}' {
            out.push(c);
        } else if c == '\u{7f}' {
            out.push_str("\\u007f");
        } else {
            let n = c as u32;
            if n < 0x1_0000 {
                out.push_str(&format!("\\u{n:04x}"));
            } else {
                let v = n - 0x1_0000;
                let (hi, lo) = (0xd800 + (v >> 10), 0xdc00 + (v & 0x3ff));
                out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
            }
        }
    }
    out
}

/// Inputs to [`mint_access_token`], mirroring its keyword arguments plus the
/// settings/clock/key values Python reads implicitly.
#[derive(Debug, Clone)]
pub struct MintParams<'a> {
    /// `runner_id` (`sub`, stringified).
    pub runner_id: &'a str,
    /// `user_id` (`uid`, trust-principal binding).
    pub user_id: &'a str,
    /// `workspace_id` (`wid`, trust-principal binding).
    pub workspace_id: &'a str,
    /// `rtg`: refresh-token generation that minted this token.
    pub rtg: i64,
    /// `ttl_secs` (`None` → `default_ttl_secs` below).
    pub ttl_secs: Option<i64>,
    /// `settings.ACCESS_TOKEN_TTL_SECS` (Django default 3600).
    pub default_ttl_secs: i64,
    /// `int(time.time())` at mint.
    pub now_unix: i64,
}

/// `datetime.fromtimestamp(now + ttl, tz=utc)` (`:168`): whole seconds.
/// Saturates only on absurd inputs (Python ints are unbounded).
fn expires_at_from_unix(exp_unix: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(exp_unix, 0).unwrap_or(if exp_unix < 0 {
        DateTime::<Utc>::MIN_UTC
    } else {
        DateTime::<Utc>::MAX_UTC
    })
}

/// `mint_access_token` (`:135-170`): HS256 JWT over the resolved [`SigningKey`],
/// byte-identical to PyJWT 2.12 (sorted compact header, insertion-order
/// compact payload, HMAC-SHA256, unpadded base64url). `rtg`/revocation
/// checks stay with the caller, as in Python.
pub fn mint_access_token(params: &MintParams<'_>, kid: &str, secret: &[u8]) -> AccessToken {
    let ttl = params.ttl_secs.unwrap_or(params.default_ttl_secs);
    let exp = params.now_unix.saturating_add(ttl);
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = ensure_ascii(
        &serde_json::to_string(&JwtHeader {
            alg: ACCESS_TOKEN_ALG,
            kid,
            typ: "JWT",
        })
        .expect("JWT header serializes"),
    );
    let payload = ensure_ascii(
        &serde_json::to_string(&AccessTokenPayload {
            iss: ACCESS_TOKEN_ISS,
            sub: params.runner_id,
            uid: params.user_id,
            wid: params.workspace_id,
            iat: params.now_unix,
            exp,
            rtg: params.rtg,
        })
        .expect("JWT payload serializes"),
    );
    let signing_input = format!(
        "{}.{}",
        engine.encode(header.as_bytes()),
        engine.encode(payload.as_bytes())
    );
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length");
    mac.update(signing_input.as_bytes());
    let raw = format!(
        "{signing_input}.{}",
        engine.encode(mac.finalize().into_bytes())
    );
    AccessToken {
        raw,
        expires_at: expires_at_from_unix(exp),
        kid: kid.to_owned(),
    }
}

/// [`mint_access_token`] with Python's settings-implicit key lookup
/// (`_active_kid` + `_key_ring` at `:163-164`).
pub fn mint_access_token_with_keys(
    params: &MintParams<'_>,
    configured: &[KeyConfig],
    secret_key: &str,
) -> Result<AccessToken, MintError> {
    let key = active_signing_key(configured, secret_key)?;
    Ok(mint_access_token(params, &key.kid, &key.secret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_auth::jwt::{decode_access_token, JwtError};
    use serde_json::Value;

    /// D13-F3, the Done-when fixture for this module.
    fn f3() -> Value {
        let path = format!(
            "{}/../../fixtures/runner_enroll/tokens/tokens.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("F3 fixture exists"))
            .expect("F3 fixture parses")
    }

    const FIXED_SECRET: &str = "d13-fixture-secret-key";

    fn mint_params() -> MintParams<'static> {
        MintParams {
            runner_id: "11111111-1111-1111-1111-111111111111",
            user_id: "22222222-2222-2222-2222-222222222222",
            workspace_id: "33333333-3333-3333-3333-333333333333",
            rtg: 3,
            ttl_secs: None,
            default_ttl_secs: DEFAULT_ACCESS_TOKEN_TTL_SECS,
            now_unix: 1_700_000_000,
        }
    }

    #[test]
    fn prefixes_and_ttls_match_f3() {
        let fx = f3()["prefixes_ttls"].clone();
        assert_eq!(ENROLLMENT_PREFIX, fx["ENROLLMENT_PREFIX"].as_str().unwrap());
        assert_eq!(
            REFRESH_TOKEN_PREFIX,
            fx["REFRESH_TOKEN_PREFIX"].as_str().unwrap()
        );
        assert_eq!(
            MACHINE_TOKEN_PREFIX,
            fx["MACHINE_TOKEN_PREFIX"].as_str().unwrap()
        );
        assert_eq!(
            CONNECTION_SECRET_PREFIX,
            fx["CONNECTION_SECRET_PREFIX"]
                .as_str()
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
        );
        // "timedelta(hours=1) = 3600s".
        assert_eq!(ENROLLMENT_TTL_SECS, 3600);
        assert_eq!(
            DEFAULT_ACCESS_TOKEN_TTL_SECS,
            fx["ACCESS_TOKEN_TTL_SECS_default"].as_i64().unwrap()
        );
        assert_eq!(f3()["fixed_secret_key"].as_str().unwrap(), FIXED_SECRET);
    }

    #[test]
    fn minters_match_shapes() {
        // `token_urlsafe(24)` renders 32 chars, `token_urlsafe(32)` 43.
        let cases = [
            (
                mint_enrollment_token(FIXED_SECRET, Utc::now()).raw,
                ENROLLMENT_PREFIX,
                7 + 32,
            ),
            (
                mint_refresh_token(FIXED_SECRET).raw,
                REFRESH_TOKEN_PREFIX,
                3 + 43,
            ),
            (
                mint_machine_token(FIXED_SECRET).raw,
                MACHINE_TOKEN_PREFIX,
                3 + 43,
            ),
            (
                mint_connection_secret(FIXED_SECRET).raw,
                CONNECTION_SECRET_PREFIX,
                7 + 43,
            ),
        ];
        for (raw, prefix, len) in cases {
            assert!(raw.starts_with(prefix), "{raw}");
            assert_eq!(raw.len(), len, "{raw}");
            assert!(
                raw[prefix.len()..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "{raw}"
            );
        }
        // Uniqueness + kernel wiring.
        let a = mint_machine_token(FIXED_SECRET);
        let b = mint_machine_token(FIXED_SECRET);
        assert_ne!(a.raw, b.raw);
        assert_eq!(a.hashed, hash_token(&a.raw, FIXED_SECRET));
        assert_eq!(a.fingerprint, fingerprint(&a.raw));
        assert_eq!(a.hashed.len(), 64);
        assert_eq!(a.fingerprint.len(), 12);
    }

    #[test]
    fn enrollment_expiry_keeps_subsecond_precision() {
        // `timezone.now()` carries microseconds into `expires_at`.
        let now = DateTime::from_timestamp(1_700_000_000, 123_456_789).expect("ts");
        let token = mint_enrollment_token(FIXED_SECRET, now);
        assert_eq!(
            token.expires_at,
            now.checked_add_signed(Duration::seconds(ENROLLMENT_TTL_SECS))
                .expect("in range")
        );
        assert_eq!(token.expires_at.timestamp_subsec_nanos(), 123_456_789);
    }

    #[test]
    fn hash_vectors_match_f3() {
        // Our minters hash through these exact kernel calls.
        for vector in f3()["hash_vectors"].as_array().unwrap() {
            let raw = vector["raw"].as_str().unwrap();
            assert_eq!(
                hash_token(raw, FIXED_SECRET),
                vector["hash"].as_str().unwrap(),
                "{raw}"
            );
            assert_eq!(
                fingerprint(raw),
                vector["fingerprint"].as_str().unwrap(),
                "{raw}"
            );
        }
    }

    #[test]
    fn derived_ring_matches_f3() {
        let fx = f3()["key_ring"].clone();
        let expected = fx["derived_secret_for_fixed_key"].as_str().unwrap();
        assert_eq!(derived_secret_hex(FIXED_SECRET), expected);
        // An empty list derives (`if not keys` — None and [] are
        // indistinguishable in Python, so the port takes a bare slice).
        let configured: Vec<KeyConfig> = vec![];
        let ring = build_key_ring(&configured, FIXED_SECRET);
        assert_eq!(ring.active_kid(), Some("default"));
        let key = active_signing_key(&configured, FIXED_SECRET).expect("derived");
        assert_eq!(key.kid, "default");
        assert_eq!(key.secret, expected.as_bytes());
        assert_eq!(
            fx["derived_shape"]["default"]["secret"].as_str().unwrap(),
            expected
        );
    }

    #[test]
    fn derived_secret_matches_kernel_test_vector() {
        // The kernel pins `dev_from_secret(b"f05-test-secret-key-12345")` to
        // this hex; our mirror must agree byte for byte.
        assert_eq!(
            derived_secret_hex("f05-test-secret-key-12345"),
            "3adc8ab2a89721ae46e257ebf3b30d1de843819ed36d02f55b8b151aa8f4c974"
        );
    }

    fn key(kid: &str, status: Option<&str>) -> KeyConfig {
        KeyConfig {
            kid: kid.to_owned(),
            secret: format!("secret-for-{kid}"),
            status: status.map(str::to_owned),
        }
    }

    #[test]
    fn list_form_status_and_order() {
        // Status defaults to active; first active in list order wins.
        let configured = vec![
            key("old", Some("verify_only")),
            key("new", None),
            key("third", Some("active")),
        ];
        assert_eq!(
            active_signing_key(&configured, FIXED_SECRET)
                .expect("active")
                .kid,
            "new"
        );
        let ring = build_key_ring(&configured, FIXED_SECRET);
        assert_eq!(ring.active_kid(), Some("new"));
        // Secrets are the UTF-8 bytes of the configured str.
        let key = active_signing_key(&configured, FIXED_SECRET).expect("active");
        assert_eq!(key.secret, b"secret-for-new");
    }

    #[test]
    fn no_active_key_errors_verbatim() {
        let configured = vec![key("old", Some("verify_only"))];
        let err = active_signing_key(&configured, FIXED_SECRET).unwrap_err();
        assert_eq!(err, MintError::NoActiveKey);
        assert_eq!(
            err.to_string(),
            "no active access-token signing key configured"
        );
        assert_eq!(build_key_ring(&configured, FIXED_SECRET).active_kid(), None);
        assert_eq!(
            mint_access_token_with_keys(&mint_params(), &configured, FIXED_SECRET).unwrap_err(),
            MintError::NoActiveKey
        );
    }

    fn segments(raw: &str) -> (String, String) {
        let parts: Vec<&str> = raw.split('.').collect();
        assert_eq!(parts.len(), 3, "{raw}");
        let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let decode = |s: &str| String::from_utf8(engine.decode(s).expect("b64")).expect("utf8");
        (decode(parts[0]), decode(parts[1]))
    }

    #[test]
    fn mint_claim_bytes_match_f3() {
        let params = mint_params();
        let token = mint_access_token_with_keys(&params, &[], FIXED_SECRET).expect("derived");
        assert_eq!(token.kid, "default");
        let (header, payload) = segments(&token.raw);
        assert_eq!(header, r#"{"alg":"HS256","kid":"default","typ":"JWT"}"#);
        assert_eq!(
            payload,
            r#"{"iss":"pi-dash-cloud","sub":"11111111-1111-1111-1111-111111111111","uid":"22222222-2222-2222-2222-222222222222","wid":"33333333-3333-3333-3333-333333333333","iat":1700000000,"exp":1700003600,"rtg":3}"#
        );
        assert_eq!(
            token.expires_at,
            DateTime::from_timestamp(1_700_003_600, 0).expect("ts")
        );
        // Every claim `decode_access_token` requires is present.
        let required = f3()["access_token"]["decode_require"].clone();
        let value: Value = serde_json::from_str(&payload).expect("json");
        for claim in required.as_array().unwrap() {
            let name = claim.as_str().unwrap();
            assert!(value.get(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn mint_is_byte_identical_to_pyjwt_vector() {
        // PyJWT-minted kernel vector (`pidash_auth::jwt` tests): same inputs
        // must produce the identical three segments.
        const JWT_VALID: &str = "eyJhbGciOiJIUzI1NiIsImtpZCI6ImRlZmF1bHQiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJwaS1kYXNoLWNsb3VkIiwic3ViIjoiMTExMTExMTEtMjIyMi0zMzMzLTQ0NDQtNTU1NTU1NTU1NTU1IiwidWlkIjoiN2M5ZTY2NzktNzQyNS00MGRlLTk0NGItZTI5YjVlNzM3OTAzIiwid2lkIjoiYWFhYWFhYWEtYmJiYi1jY2NjLWRkZGQtZWVlZWVlZWVlZWVlIiwiaWF0IjoyMDAwMDAwMDAwLCJleHAiOjIwMDAwMDM2MDAsInJ0ZyI6N30.d13__-h-ksMCqnA09HLhIrEbbHJluA4SGv-WaJhI5NU";
        let params = MintParams {
            runner_id: "11111111-2222-3333-4444-555555555555",
            user_id: "7c9e6679-7425-40de-944b-e29b5e737903",
            workspace_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            rtg: 7,
            ttl_secs: None,
            default_ttl_secs: 3600,
            now_unix: 2_000_000_000,
        };
        let token = mint_access_token_with_keys(&params, &[], "f05-test-secret-key-12345")
            .expect("derived");
        assert_eq!(token.raw, JWT_VALID);
    }

    #[test]
    fn mint_round_trips_through_kernel_decode() {
        // Independent oracle: the kernel decoder was written against PyJWT
        // and checks `exp` against the wall clock, so the valid mint uses a
        // 2033 timestamp while the expired mint uses a 2023 one.
        let params = MintParams {
            now_unix: 2_000_000_000,
            ..mint_params()
        };
        let ring = build_key_ring(&[], FIXED_SECRET);
        let token = mint_access_token_with_keys(&params, &[], FIXED_SECRET).expect("derived");
        let claims = decode_access_token(&token.raw, &ring).expect("decodes");
        assert_eq!(claims.sub, params.runner_id);
        assert_eq!(claims.uid, params.user_id);
        assert_eq!(claims.wid, params.workspace_id);
        assert_eq!(claims.iat, params.now_unix as u64);
        assert_eq!(claims.exp, (params.now_unix + 3600) as u64);
        assert_eq!(claims.rtg, params.rtg);
        // A past-expiry mint decodes as expired (F3's expired trigger).
        let past = MintParams {
            ttl_secs: Some(-10),
            ..mint_params()
        };
        let old = mint_access_token_with_keys(&past, &[], FIXED_SECRET).expect("derived");
        assert_eq!(
            decode_access_token(&old.raw, &ring).unwrap_err(),
            JwtError::Expired
        );
    }

    #[test]
    fn mint_ttl_default_and_override() {
        let with_default =
            mint_access_token_with_keys(&mint_params(), &[], FIXED_SECRET).expect("derived");
        assert_eq!(
            with_default.expires_at.timestamp() - mint_params().now_unix,
            3600
        );
        let params = MintParams {
            ttl_secs: Some(60),
            ..mint_params()
        };
        let overridden = mint_access_token_with_keys(&params, &[], FIXED_SECRET).expect("derived");
        assert_eq!(overridden.expires_at.timestamp() - params.now_unix, 60);
        let params = MintParams {
            default_ttl_secs: 7200,
            ..mint_params()
        };
        let custom = mint_access_token_with_keys(&params, &[], FIXED_SECRET).expect("derived");
        assert_eq!(custom.expires_at.timestamp() - params.now_unix, 7200);
    }

    #[test]
    fn mint_with_keys_matches_direct_mint() {
        let params = mint_params();
        let configured = vec![key("k1", None)];
        let via_keys =
            mint_access_token_with_keys(&params, &configured, FIXED_SECRET).expect("active");
        let direct = mint_access_token(&params, "k1", b"secret-for-k1");
        assert_eq!(via_keys, direct);
    }

    #[test]
    fn ensure_ascii_matches_pyjwt() {
        assert_eq!(ensure_ascii("plain"), "plain");
        assert_eq!(ensure_ascii("caf\u{e9}"), "caf\\u00e9");
        assert_eq!(ensure_ascii("\u{7f}"), "\\u007f");
        assert_eq!(ensure_ascii("\u{1f600}"), "\\ud83d\\ude00");
        // Non-ASCII claims escape in the minted bytes (PyJWT ensure_ascii).
        let params = MintParams {
            runner_id: "r\u{e9}",
            ..mint_params()
        };
        let token = mint_access_token_with_keys(&params, &[], FIXED_SECRET).expect("derived");
        let (_, payload) = segments(&token.raw);
        assert!(payload.contains(r#""sub":"r\u00e9""#), "{payload}");
    }
}
