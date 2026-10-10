//! S3 bucket commands: `create_bucket`, `update_bucket`.
//!
//! Ports of `apps/api/pi_dash/db/management/commands/create_bucket.py:14-61`
//! and `update_bucket.py:16-187` (fixture F37-02). The command flows
//! ([`run_create_bucket`], [`run_update_bucket`]) run against the [`S3Ops`]
//! trait, so the branch matrix is unit-testable with a fake; [`ReqwestS3`]
//! is the live SigV4 client.
//!
//! Request shapes were captured from botocore (boto3 1.34.96, the pinned
//! version) against a logging stub: path-style URLs against an explicit
//! endpoint, `GET /{bucket}?list-type=2&encoding-type=url` for lists
//! (keys arrive URL-encoded and are decoded, like botocore does),
//! `PUT /{bucket}?policy` for policies, empty bodies on head/create, and
//! auto-sent `Content-MD5` on PutObject/PutBucketPolicy only. The signer
//! sends the real payload hash in `x-amz-content-sha256`, following the
//! `pidash-storage` precedent (botocore 1.34 sends real hashes too).
//!
//! Documented translations (also listed in the PR):
//!
//! * botocore retries 5xx and transport errors (the 5xx message gains a
//!   `(reached max retries: 4)` suffix); the Rust client attempts every
//!   call once and fails fast. Retry timing and the suffix are the only
//!   divergence — every status/code/message classification matches.
//! * Python tracebacks (unhandled `ValueError` from `int()`, uncaught
//!   `ClientError` from the permissions.json fallback, `NoRegionError` in
//!   `update_bucket`) become a one-line stderr message; the exit code (1)
//!   matches.
//! * Credentials resolve from the environment only. boto3 would fall
//!   through to shared files and instance metadata; the project's config
//!   registry classifies these keys as env-only, so that chain is out of
//!   contract.
//! * Ported bugs: the PutBucketPolicy permission probe really applies a
//!   public-read policy (F37-02 BUGS — the probe IS the mutation); a
//!   non-404 head error in `update_bucket` falls through to the success
//!   line; the GetObject probe silently skips empty buckets; `create_bucket`
//!   parses codes with `int()` while `update_bucket` string-compares them;
//!   `update_bucket`'s help is create_bucket's stale copy (kept in clap).

use std::time::Duration;

/// `create_bucket.py:31` / `update_bucket.py:146`, `style.NOTICE`.
pub const CHECKING_BUCKET: &str = "Checking bucket...";
/// `update_bucket.py:142`, `style.ERROR`.
pub const PLEASE_SET_BUCKET: &str = "Please set the AWS_S3_BUCKET_NAME environment variable.";
/// `update_bucket.py:76`, plain stdout (note: no error detail interpolated).
pub const COULD_NOT_DELETE_TEST_OBJECT: &str = "Couldn't delete test object";
/// `update_bucket.py:163`, `style.SUCCESS`.
pub const HAVE_PERMISSIONS: &str = "Access key has the required permissions.";
/// `update_bucket.py:129`, plain stdout from `make_objects_public`.
pub const PRIVATE_BUT_PUBLIC: &str = "Bucket is private, but existing objects remain public.";
/// `update_bucket.py:178`, `style.WARNING`.
pub const GENERATING_PERMISSIONS: &str =
    "Generating permissions.json for manual bucket policy update.";
/// `update_bucket.py:183`, `style.WARNING`.
pub const PERMISSIONS_WRITTEN: &str = "Permissions have been written to permissions.json.";
/// `update_bucket.py:42`.
pub const LIST_BUCKET_DENIED: &str = "ListBucket permission denied.";
/// `update_bucket.py:53`.
pub const GET_OBJECT_DENIED: &str = "GetObject permission denied.";
/// `update_bucket.py:68`.
pub const PUT_OBJECT_DENIED: &str = "PutObject permission denied.";
/// `update_bucket.py:93`.
pub const PUT_POLICY_DENIED: &str = "PutBucketPolicy permission denied.";
/// Name of the fallback file, written to the process working directory
/// (`update_bucket.py:181`).
pub const PERMISSIONS_FILE: &str = "permissions.json";
/// Probe object (`update_bucket.py:64,74`).
pub const TEST_OBJECT_KEY: &str = "test_permission_check.txt";
/// Probe body (`update_bucket.py:64`).
pub const TEST_OBJECT_BODY: &[u8] = b"Test";
/// CPython 3.11+ message for the unbound `permissions` local
/// (`update_bucket.py:171` raises it when `check_s3_permissions` raised;
/// the deployment runs Python 3.12). Ported verbatim per F37-02.
pub const UNBOUND_PERMISSIONS_MESSAGE: &str =
    "cannot access local variable 'permissions' where it is not associated with a value";

/// The five environment variables both bucket commands read with
/// `os.environ.get` (`create_bucket.py:21-28,32`, `update_bucket.py:18-27`).
/// `None` means unset; an empty string is `Some("")` and behaves exactly
/// like boto3 treats it (empty credentials are *used*, empty region is
/// passed through — only `None` triggers the missing-value paths).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct S3Env {
    pub endpoint_url: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
}

impl S3Env {
    /// Read the five variables from the process environment.
    pub fn from_env() -> Self {
        Self {
            endpoint_url: std::env::var("AWS_S3_ENDPOINT_URL").ok(),
            access_key_id: std::env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
            region: std::env::var("AWS_REGION").ok(),
            bucket: std::env::var("AWS_S3_BUCKET_NAME").ok(),
        }
    }
}

/// Client-construction failures, in the order Python raises them: region
/// first (`boto3.client` raises `NoRegionError`), then the `None` bucket
/// (which fails URL construction with a `TypeError`), and credentials last
/// (raised lazily per call as [`S3CallError::NoCredentials`]).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum S3SetupError {
    /// `NoRegionError`: `str()` is exactly this (pinned boto3 1.34.96).
    #[error("You must specify a region.")]
    NoRegion,
    /// Not a `ParamValidationError`: with `Bucket=None`, botocore fails
    /// URL construction with this CPython `TypeError` (verified with the
    /// pinned boto3, with and without credentials set).
    #[error("expected string or bytes-like object, got 'NoneType'")]
    BucketNone,
}

/// A resolved call target: everything except the credentials, which stay
/// lazy so the flows reproduce boto3's per-call `NoCredentialsError`.
#[derive(Debug, Clone, PartialEq)]
pub struct S3Target {
    pub endpoint_url: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: String,
    pub bucket: String,
}

/// `create_bucket` setup: build the client (region check), then resolve
/// the bucket (`create_bucket.py:20-32`).
pub fn resolve_create_target(env: &S3Env) -> Result<S3Target, S3SetupError> {
    Ok(S3Target {
        endpoint_url: env.endpoint_url.clone(),
        access_key_id: env.access_key_id.clone(),
        secret_access_key: env.secret_access_key.clone(),
        region: env.region.clone().ok_or(S3SetupError::NoRegion)?,
        bucket: env.bucket.clone().ok_or(S3SetupError::BucketNone)?,
    })
}

/// `update_bucket` setup outcome: either ready to run, or the `None`
/// bucket, which prints its own line and returns (`update_bucket.py:140`).
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateSetup {
    Ready(S3Target),
    MissingBucket,
}

/// `update_bucket` setup: build the client (region check, fatal —
/// `update_bucket.py:138` is outside any `try`), then the bucket check.
pub fn resolve_update_target(env: &S3Env) -> Result<UpdateSetup, S3SetupError> {
    let region = env.region.clone().ok_or(S3SetupError::NoRegion)?;
    let Some(bucket) = env.bucket.clone() else {
        return Ok(UpdateSetup::MissingBucket);
    };
    Ok(UpdateSetup::Ready(S3Target {
        endpoint_url: env.endpoint_url.clone(),
        access_key_id: env.access_key_id.clone(),
        secret_access_key: env.secret_access_key.clone(),
        region,
        bucket,
    }))
}

/// `botocore.exceptions.ClientError`, structurally: the S3 error `Code`
/// and `Message` plus the calling operation and the HTTP status.
#[derive(Debug, Clone, PartialEq)]
pub struct S3ServiceError {
    pub code: String,
    pub message: String,
    pub operation: &'static str,
    pub status: u16,
}

impl std::fmt::Display for S3ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "An error occurred ({}) when calling the {} operation: {}",
            self.code, self.operation, self.message
        )
    }
}

/// Every way one S3 call can fail: a service error (a `ClientError` in
/// Python — caught by the `except ClientError` arms), a transport failure
/// (an `EndpointConnectionError` — never a `ClientError`), or missing
/// credentials (a lazily-raised `NoCredentialsError`).
#[derive(Debug, Clone, PartialEq)]
pub enum S3CallError {
    Service(S3ServiceError),
    Transport(String),
    NoCredentials,
}

impl std::fmt::Display for S3CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service(error) => write!(f, "{error}"),
            Self::Transport(message) => write!(f, "{message}"),
            Self::NoCredentials => write!(f, "Unable to locate credentials"),
        }
    }
}

impl std::error::Error for S3CallError {}

/// The seven S3 calls the bucket commands make, in botocore terms:
/// `head_bucket`, `create_bucket`, `list_objects_v2` (returning the
/// decoded key list — `Contents` absent means empty), `get_object`,
/// `put_object`, `delete_object`, `put_bucket_policy`.
#[allow(async_fn_in_trait)]
pub trait S3Ops {
    /// `HEAD /{bucket}`.
    async fn head_bucket(&self) -> Result<(), S3CallError>;
    /// `PUT /{bucket}` with an empty body.
    async fn create_bucket(&self) -> Result<(), S3CallError>;
    /// `GET /{bucket}?list-type=2&encoding-type=url`, decoded keys.
    async fn list_keys(&self) -> Result<Vec<String>, S3CallError>;
    /// `GET /{bucket}/{key}` (body discarded).
    async fn get_object(&self, key: &str) -> Result<(), S3CallError>;
    /// `PUT /{bucket}/{key}` with `Content-MD5`.
    async fn put_object(&self, key: &str, body: &[u8]) -> Result<(), S3CallError>;
    /// `DELETE /{bucket}/{key}`.
    async fn delete_object(&self, key: &str) -> Result<(), S3CallError>;
    /// `PUT /{bucket}?policy` with `Content-MD5`.
    async fn put_bucket_policy(&self, policy_json: &str) -> Result<(), S3CallError>;
}

/// `create_bucket`'s `int(error_code)` failing on a symbolic code
/// (`create_bucket.py:37` raises `ValueError` out of `handle` — traceback,
/// exit 1).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("invalid literal for int() with base 10: {0:?}")]
pub struct NonNumericCode(pub String);

/// `create_bucket.py:19-61`: check-then-create with the 404/403/else
/// split on `int(Code)`. Transport failures take the outer
/// `except Exception` arm (`An error occurred`), including one from the
/// inner create call (the inner `except` catches `ClientError` only).
/// Returns `Err` only for a symbolic head code (Python's `ValueError`).
pub async fn run_create_bucket(
    ops: &impl S3Ops,
    bucket: &str,
    out: &mut dyn FnMut(&str),
) -> Result<(), NonNumericCode> {
    out(CHECKING_BUCKET);
    let head_error = match ops.head_bucket().await {
        Ok(()) => {
            out(&format!("Bucket '{bucket}' exists."));
            return Ok(());
        }
        Err(S3CallError::Service(error)) => error,
        Err(other) => {
            out(&format!("An error occurred: {other}"));
            return Ok(());
        }
    };
    // `int(e.response["Error"]["Code"])`: Python strips whitespace and
    // accepts a leading `+`; anything else raises out of `handle`.
    let code: i64 = head_error
        .code
        .trim()
        .parse()
        .map_err(|_| NonNumericCode(head_error.code.clone()))?;
    if code == 404 {
        out(&format!(
            "Bucket '{bucket}' does not exist. Creating bucket..."
        ));
        match ops.create_bucket().await {
            Ok(()) => out(&format!("Bucket '{bucket}' created successfully.")),
            Err(S3CallError::Service(error)) => {
                out(&format!("Failed to create bucket: {error}"));
            }
            // Not a `ClientError`: the inner `except` misses it and the
            // outer `except Exception` catches it.
            Err(other) => out(&format!("An error occurred: {other}")),
        }
    } else if code == 403 {
        out(&format!(
            "Access to the bucket '{bucket}' is forbidden. Check permissions."
        ));
    } else {
        out(&format!("Failed to check bucket: {head_error}"));
    }
    Ok(())
}

/// The four permission probes (`update_bucket.py:30-37` initial dict).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BucketPermissions {
    pub get_object: bool,
    pub list_bucket: bool,
    pub put_bucket_policy: bool,
    pub put_object: bool,
}

impl BucketPermissions {
    fn all(&self) -> bool {
        self.get_object && self.list_bucket && self.put_bucket_policy && self.put_object
    }
}

/// `check_s3_permissions` (`update_bucket.py:29-100`): the four probes in
/// order, with the always-attempted test-object cleanup between the put
/// and policy probes. A `Transport`/`NoCredentials` failure aborts the
/// check (Python lets non-`ClientError`s propagate); service errors print
/// their probe line and continue. The policy probe really applies the
/// public-read policy — ported, not fixed.
async fn check_permissions(
    ops: &impl S3Ops,
    bucket: &str,
    out: &mut dyn FnMut(&str),
) -> Result<BucketPermissions, S3CallError> {
    let mut permissions = BucketPermissions::default();
    // 1. `s3:ListBucket` (:39-47).
    match ops.list_keys().await {
        Ok(_) => permissions.list_bucket = true,
        Err(S3CallError::Service(error)) if error.code == "AccessDenied" => {
            out(LIST_BUCKET_DENIED);
        }
        Err(S3CallError::Service(error)) => out(&format!("Error in ListBucket: {error}")),
        Err(other) => return Err(other),
    }
    // 2. `s3:GetObject` (:49-60): a second list call, then a get of the
    // first key. A failure of either prints the GetObject line; an empty
    // bucket skips silently (no `Contents` key).
    match ops.list_keys().await {
        Ok(keys) => {
            if let Some(first) = keys.first() {
                match ops.get_object(first).await {
                    Ok(()) => permissions.get_object = true,
                    Err(S3CallError::Service(error)) if error.code == "AccessDenied" => {
                        out(GET_OBJECT_DENIED);
                    }
                    Err(S3CallError::Service(error)) => {
                        out(&format!("Error in GetObject: {error}"));
                    }
                    Err(other) => return Err(other),
                }
            }
        }
        Err(S3CallError::Service(error)) if error.code == "AccessDenied" => {
            out(GET_OBJECT_DENIED);
        }
        Err(S3CallError::Service(error)) => out(&format!("Error in GetObject: {error}")),
        Err(other) => return Err(other),
    }
    // 3. `s3:PutObject` (:62-71).
    match ops.put_object(TEST_OBJECT_KEY, TEST_OBJECT_BODY).await {
        Ok(()) => permissions.put_object = true,
        Err(S3CallError::Service(error)) if error.code == "AccessDenied" => {
            out(PUT_OBJECT_DENIED);
        }
        Err(S3CallError::Service(error)) => out(&format!("Error in PutObject: {error}")),
        Err(other) => return Err(other),
    }
    // Cleanup (:73-77): always attempted, even when the put failed; only
    // `ClientError` prints (without the error itself).
    match ops.delete_object(TEST_OBJECT_KEY).await {
        Ok(()) => {}
        Err(S3CallError::Service(_)) => out(COULD_NOT_DELETE_TEST_OBJECT),
        Err(other) => return Err(other),
    }
    // 4. `s3:PutBucketPolicy` (:79-98): the probe payload is a real
    // public-read policy, applied by the probe itself (ported bug).
    match ops.put_bucket_policy(&probe_policy_json(bucket)).await {
        Ok(()) => permissions.put_bucket_policy = true,
        Err(S3CallError::Service(error)) if error.code == "AccessDenied" => {
            out(PUT_POLICY_DENIED);
        }
        Err(S3CallError::Service(error)) => {
            out(&format!("Error in PutBucketPolicy: {error}"));
        }
        Err(other) => return Err(other),
    }
    Ok(permissions)
}

/// `generate_bucket_policy` (`update_bucket.py:102-121`): one ARN per
/// listed key, in response order; an empty bucket yields `"Resource": []`.
fn generate_bucket_policy(bucket: &str, keys: &[String]) -> String {
    bucket_policy_json(bucket, keys)
}

/// `make_objects_public` (`update_bucket.py:123-132`): apply the
/// generated policy. The caller prints the success line.
async fn make_objects_public(ops: &impl S3Ops, bucket: &str) -> Result<(), S3CallError> {
    let keys = ops.list_keys().await?;
    ops.put_bucket_policy(&generate_bucket_policy(bucket, &keys))
        .await
}

/// Format an `IOError` exactly like CPython's `str(exc)` for file errors:
/// `[Errno 13] Permission denied: 'permissions.json'`.
fn io_error_line(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => {
            let message = std::io::Error::from_raw_os_error(code).to_string();
            let message = message
                .strip_suffix(&format!(" (os error {code})"))
                .unwrap_or(&message)
                .to_string();
            format!(
                "Error writing permissions.json: [Errno {code}] {message}: '{PERMISSIONS_FILE}'"
            )
        }
        None => format!("Error writing permissions.json: {error}"),
    }
}

/// `update_bucket.py:134-187`. `write_file` writes the fallback policy to
/// the working directory (injected for tests; production passes
/// `std::fs::write`). Returns `Err` exactly where Python lets an error
/// escape `handle` (traceback, exit 1): a transport/credentials failure on
/// the head call, or a failed list inside the permissions.json fallback.
pub async fn run_update_bucket(
    ops: &impl S3Ops,
    bucket: &str,
    write_file: &dyn Fn(&str, &str) -> std::io::Result<()>,
    out: &mut dyn FnMut(&str),
) -> Result<(), S3CallError> {
    out(CHECKING_BUCKET);
    match ops.head_bucket().await {
        Ok(()) => out(&format!("Bucket '{bucket}' exists.")),
        // String comparison (`== "404"`), unlike create_bucket's `int()`.
        Err(S3CallError::Service(error)) if error.code == "404" => {
            out(&format!("Bucket '{bucket}' does not exist."));
            return Ok(());
        }
        Err(S3CallError::Service(error)) => {
            // Ported bug: any other head error prints `Error:` and falls
            // through to the success line.
            out(&format!("Error: {error}"));
            out(&format!("Bucket '{bucket}' exists."));
        }
        // The head `try` catches `ClientError` only — anything else
        // escapes `handle`.
        Err(other) => return Err(other),
    }

    // `check_s3_permissions` raising (non-`ClientError`) is caught at
    // :166-169, and then `all(permissions.values())` raises `NameError`
    // (:171, caught at :174-175) because `permissions` was never bound —
    // two `Error:` lines, then the fallback. Ported per F37-02.
    let permissions = match check_permissions(ops, bucket, out).await {
        Ok(permissions) => Some(permissions),
        Err(error) => {
            out(&format!("Error: {error}"));
            out(&format!("Error: {UNBOUND_PERMISSIONS_MESSAGE}"));
            None
        }
    };
    if let Some(permissions) = permissions {
        if permissions.all() {
            out(HAVE_PERMISSIONS);
            match make_objects_public(ops, bucket).await {
                Ok(()) => {
                    out(PRIVATE_BUT_PUBLIC);
                    return Ok(());
                }
                // :170-175 catches everything from the make-public call
                // and falls through to the fallback.
                Err(error) => out(&format!("Error: {error}")),
            }
        }
    }

    out(GENERATING_PERMISSIONS);
    // The fallback list is outside any `ClientError` handling — only the
    // file write catches `IOError`.
    let keys = ops.list_keys().await?;
    let policy = generate_bucket_policy(bucket, &keys);
    match write_file(PERMISSIONS_FILE, &policy) {
        Ok(()) => out(PERMISSIONS_WRITTEN),
        Err(error) => out(&io_error_line(&error)),
    }
    Ok(())
}

/// Escape a string exactly like CPython `json.dumps` with the default
/// `ensure_ascii=True`: `"`, `\` and the short escapes, `\u00xx` for other
/// C0 controls and DEL, `\uXXXX` above that (surrogate pairs past the BMP),
/// `/` untouched.
pub fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                escaped.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch if (ch as u32) < 0x80 => escaped.push(ch),
            ch if (ch as u32) < 0x10000 => {
                escaped.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => {
                let code = ch as u32 - 0x10000;
                let high = 0xd800 + (code >> 10);
                let low = 0xdc00 + (code & 0x3ff);
                escaped.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    escaped
}

/// The PutBucketPolicy probe payload (`update_bucket.py:81-89`):
/// `json.dumps` with default separators (`", "`, `": "`), insertion order
/// `Version`, `Statement` / `Effect`, `Principal`, `Action`, `Resource`.
pub fn probe_policy_json(bucket: &str) -> String {
    format!(
        "{{\"Version\": \"2012-10-17\", \"Statement\": [{{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": \"{}\"}}]}}",
        json_escape(&format!("arn:aws:s3:::{bucket}/*"))
    )
}

/// The generated bucket policy (`update_bucket.py:112-120`): same shape,
/// with one ARN per key in response order.
pub fn bucket_policy_json(bucket: &str, keys: &[String]) -> String {
    let resources = keys
        .iter()
        .map(|key| {
            format!(
                "\"{}\"",
                json_escape(&format!("arn:aws:s3:::{bucket}/{key}"))
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"Version\": \"2012-10-17\", \"Statement\": [{{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": [{resources}]}}]}}"
    )
}

/// Percent-encode an S3 path the way SigV4 canonical URIs require:
/// unreserved bytes plus `/` stay literal, everything else becomes
/// uppercase `%XX` over the UTF-8 bytes.
pub fn uri_encode_path(path: &str) -> String {
    const UNRESERVED: &[u8] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~/";
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if UNRESERVED.contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Decode `%XX` sequences (list keys arrive URL-encoded with
/// `encoding-type=url`; `+` is a literal plus, not a space).
pub fn uri_decode_key(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let mut advanced = false;
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Some(high) = hex_value(bytes[index + 1]) {
                if let Some(low) = hex_value(bytes[index + 2]) {
                    decoded.push(high * 16 + low);
                    index += 3;
                    advanced = true;
                }
            }
        }
        if !advanced {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Canonicalize a query string for signing: split on `&`, sort, render
/// each pair as `name=value` (a bare name gains `=`), join with `&`.
/// `?policy` therefore signs as `policy=`, like botocore.
pub fn canonical_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }
    let mut pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => format!("{name}={value}"),
            None => format!("{pair}="),
        })
        .collect();
    pairs.sort();
    pairs.join("&")
}

/// Sign one S3 request (SigV4 `s3`, header auth), mirroring the
/// `pidash-storage` canonical-request construction (`crates/storage` is
/// read-only and its fixed-shape helpers cannot sign query-bearing
/// requests). Signed headers are exactly
/// `host;x-amz-content-sha256;x-amz-date`, like botocore sends for these
/// operations; `Content-MD5` (where sent) stays unsigned. Returns the
/// `Authorization` header value.
// Ten positional inputs mirror the D-02 presign-helper shape (like the
// storage crate's own `sign_*`); grouping them would only rename the tuple.
#[allow(clippy::too_many_arguments)]
pub fn sign_s3_request(
    access_key: &str,
    secret_key: &str,
    region: &str,
    method: &str,
    canonical_uri: &str,
    query: &str,
    signed_host: &str,
    payload_hash: &str,
    amz_datetime: &str,
    date_stamp: &str,
) -> String {
    let canonical_query = canonical_query_string(query);
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\nhost:{signed_host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\n\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_datetime}\n{scope}\n{}",
        pidash_storage::sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex_hmac(
        &signing_key(secret_key, date_stamp, region),
        string_to_sign.as_bytes(),
    );
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

fn signing_key(secret_key: &str, date_stamp: &str, region: &str) -> Vec<u8> {
    use hmac::{KeyInit, Mac};
    let mut date_hmac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(format!("AWS4{secret_key}").as_bytes())
            .expect("HMAC accepts any key length");
    date_hmac.update(date_stamp.as_bytes());
    let date_key = date_hmac.finalize().into_bytes();
    let mut region_hmac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(&date_key).expect("HMAC accepts any key length");
    region_hmac.update(region.as_bytes());
    let region_key = region_hmac.finalize().into_bytes();
    let mut service_hmac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&region_key)
        .expect("HMAC accepts any key length");
    service_hmac.update(b"s3");
    let service_key = service_hmac.finalize().into_bytes();
    let mut signing_hmac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&service_key)
        .expect("HMAC accepts any key length");
    signing_hmac.update(b"aws4_request");
    signing_hmac.finalize().into_bytes().to_vec()
}

fn hex_hmac(key: &[u8], message: &[u8]) -> String {
    use hmac::{KeyInit, Mac};
    let mut hmac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    hmac.update(message);
    let bytes = hmac.finalize().into_bytes();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Current UTC time as (`amz_datetime`, `date_stamp`).
fn now_stamps() -> (String, String) {
    let now = chrono::Utc::now();
    (
        now.format("%Y%m%dT%H%M%SZ").to_string(),
        now.format("%Y%m%d").to_string(),
    )
}

/// Parse an S3 error body: the first `<Code>`/`<Message>` pair. Returns
/// `None` for empty or non-XML bodies, in which case the caller falls back
/// to the status-derived code/message.
pub fn parse_error_xml(body: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(body).ok()?;
    let code = xml_tag(text, "Code")?;
    let message = xml_tag(text, "Message").unwrap_or_default();
    Some((xml_unescape(&code), xml_unescape(&message)))
}

fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(text[start..end].to_string())
}

/// Collect every `<Key>` value of a ListBucketResult, URL-decoded.
pub fn parse_list_keys(body: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(body) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<Key>") {
        let content = &rest[start + "<Key>".len()..];
        let Some(end) = content.find("</Key>") else {
            break;
        };
        keys.push(uri_decode_key(&xml_unescape(&content[..end])));
        rest = &content[end + "</Key>".len()..];
    }
    keys
}

/// Unescape the XML entities S3 uses in error and list bodies: the five
/// named entities plus decimal and hex character references.
pub fn xml_unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';') else {
            out.push_str(&rest[start..]);
            break;
        };
        let entity = &after[..end];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if let Some(number) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X")) =>
            {
                match u32::from_str_radix(number, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(ch) => out.push(ch),
                    None => out.push_str(&rest[start..start + 1 + end + 1]),
                }
            }
            _ if let Some(number) = entity.strip_prefix("#") => {
                match number.parse::<u32>().ok().and_then(char::from_u32) {
                    Some(ch) => out.push(ch),
                    None => out.push_str(&rest[start..start + 1 + end + 1]),
                }
            }
            _ => out.push_str(&rest[start..start + 1 + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Resolve the request base exactly like boto3 with these settings: an
/// explicit endpoint URL is used path-style (`{endpoint}/{bucket}...`);
/// otherwise the virtual-hosted AWS default. Returns
/// (`url_base`, `signed_host`, `path_prefix`).
pub fn resolve_request_base(
    endpoint_url: Option<&str>,
    region: &str,
    bucket: &str,
) -> (String, String, String) {
    if let Some(endpoint) = endpoint_url.filter(|endpoint| !endpoint.is_empty()) {
        let base = endpoint.trim_end_matches('/').to_string();
        let signed_host = base
            .rsplit("://")
            .next()
            .unwrap_or(base.as_str())
            .split('/')
            .next()
            .unwrap_or(base.as_str())
            .to_string();
        // Buckets are DNS-safe in the entrypoint contract; boto3 would
        // percent-encode exotic names, which S3 rejects anyway.
        (base, signed_host, format!("/{bucket}"))
    } else if region.is_empty() {
        // botocore derives `https://s3..amazonaws.com` and rejects it;
        // unreachable here (empty region is passed through to signing and
        // the request fails at the server like Python's).
        (
            format!("https://{bucket}.s3.amazonaws.com"),
            format!("{bucket}.s3.amazonaws.com"),
            String::new(),
        )
    } else {
        let host = if region == "us-east-1" {
            "s3.amazonaws.com".to_string()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        (
            format!("https://{bucket}.{host}"),
            format!("{bucket}.{host}"),
            String::new(),
        )
    }
}

/// The live [`S3Ops`] client: reqwest plus the [`sign_s3_request`] signer.
/// Missing credentials fail every call without I/O, like boto3's lazy
/// `NoCredentialsError`.
pub struct ReqwestS3 {
    client: reqwest::Client,
    url_base: String,
    signed_host: String,
    path_prefix: String,
    access_key: Option<String>,
    secret_key: Option<String>,
    region: String,
}

impl ReqwestS3 {
    /// Build from a resolved [`S3Target`]. Never fails — even an empty
    /// region passes through to signing (botocore accepts `region_name=""`
    /// and fails at the server instead).
    pub fn new(target: &S3Target) -> Self {
        let (url_base, signed_host, path_prefix) = resolve_request_base(
            target.endpoint_url.as_deref(),
            &target.region,
            &target.bucket,
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client builds with a stock TLS backend");
        Self {
            client,
            url_base,
            signed_host,
            path_prefix,
            access_key: target.access_key_id.clone(),
            secret_key: target.secret_access_key.clone(),
            region: target.region.clone(),
        }
    }

    /// Send one signed request. `operation` is the botocore operation name
    /// used in the `ClientError` text; `send_md5` mirrors botocore's
    /// auto-sent `Content-MD5` (PutObject/PutBucketPolicy only).
    async fn send(
        &self,
        operation: &'static str,
        method: reqwest::Method,
        path: &str,
        query: &str,
        body: Vec<u8>,
        send_md5: bool,
    ) -> Result<Vec<u8>, S3CallError> {
        let (Some(access_key), Some(secret_key)) =
            (self.access_key.as_deref(), self.secret_key.as_deref())
        else {
            return Err(S3CallError::NoCredentials);
        };
        let payload_hash = pidash_storage::sha256_hex(&body);
        let (amz_datetime, date_stamp) = now_stamps();
        let authorization = sign_s3_request(
            access_key,
            secret_key,
            &self.region,
            method.as_str(),
            path,
            query,
            &self.signed_host,
            &payload_hash,
            &amz_datetime,
            &date_stamp,
        );
        let mut url = format!("{}{path}", self.url_base);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        let mut request = self
            .client
            .request(method, &url)
            .header("Authorization", authorization)
            .header("x-amz-date", amz_datetime)
            .header("x-amz-content-sha256", payload_hash);
        if send_md5 {
            use base64::Engine as _;
            use md5::Digest;
            let digest = md5::Md5::digest(&body);
            request = request.header(
                "Content-MD5",
                base64::engine::general_purpose::STANDARD.encode(digest),
            );
        }
        let response = request.body(body).send().await.map_err(|error| {
            // botocore reports every transport failure as
            // `EndpointConnectionError: Could not connect to the endpoint
            // URL: "{url}"` (DNS failures included); timeouts are the
            // `Connect timeout` variant.
            if error.is_timeout() {
                S3CallError::Transport(format!("Connect timeout on endpoint URL: \"{url}\""))
            } else {
                S3CallError::Transport(format!("Could not connect to the endpoint URL: \"{url}\""))
            }
        })?;
        let status = response.status();
        if status.is_success() {
            return response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|error| {
                    S3CallError::Transport(format!(
                        "Could not connect to the endpoint URL: \"{url}\" ({error})"
                    ))
                });
        }
        let body = response.bytes().await.unwrap_or_default();
        Err(S3CallError::Service(
            self.service_error(operation, status, &body),
        ))
    }

    /// Classify a failed response: XML `Code`/`Message` when the body
    /// parses, otherwise the status-derived pair botocore synthesizes for
    /// empty/unparseable bodies (status string + reason phrase).
    fn service_error(
        &self,
        operation: &'static str,
        status: reqwest::StatusCode,
        body: &[u8],
    ) -> S3ServiceError {
        if let Some((code, message)) = parse_error_xml(body) {
            return S3ServiceError {
                code,
                message,
                operation,
                status: status.as_u16(),
            };
        }
        S3ServiceError {
            code: status.as_u16().to_string(),
            message: status.canonical_reason().unwrap_or_default().to_string(),
            operation,
            status: status.as_u16(),
        }
    }

    /// Canonical path of the bucket itself: `/{bucket}` path-style,
    /// `/` virtual-hosted.
    fn bucket_path(&self) -> String {
        if self.path_prefix.is_empty() {
            "/".to_string()
        } else {
            self.path_prefix.clone()
        }
    }

    /// Canonical path of an object: `/{bucket}/{encoded_key}`
    /// path-style, `/{encoded_key}` virtual-hosted.
    fn object_path(&self, key: &str) -> String {
        format!(
            "{}/{}",
            self.bucket_path().trim_end_matches('/'),
            uri_encode_path(key)
        )
    }
}

impl S3Ops for ReqwestS3 {
    async fn head_bucket(&self) -> Result<(), S3CallError> {
        let bucket = self.bucket_path();
        self.send(
            "HeadBucket",
            reqwest::Method::HEAD,
            &bucket,
            "",
            Vec::new(),
            false,
        )
        .await
        .map(|_| ())
    }

    async fn create_bucket(&self) -> Result<(), S3CallError> {
        let bucket = self.bucket_path();
        self.send(
            "CreateBucket",
            reqwest::Method::PUT,
            &bucket,
            "",
            Vec::new(),
            false,
        )
        .await
        .map(|_| ())
    }

    async fn list_keys(&self) -> Result<Vec<String>, S3CallError> {
        // Query order is botocore's serialization
        // (`list-type=2&encoding-type=url`); signing sorts it anyway.
        let bucket = self.bucket_path();
        let body = self
            .send(
                "ListObjectsV2",
                reqwest::Method::GET,
                &bucket,
                "list-type=2&encoding-type=url",
                Vec::new(),
                false,
            )
            .await?;
        Ok(parse_list_keys(&body))
    }

    async fn get_object(&self, key: &str) -> Result<(), S3CallError> {
        let path = self.object_path(key);
        self.send(
            "GetObject",
            reqwest::Method::GET,
            &path,
            "",
            Vec::new(),
            false,
        )
        .await
        .map(|_| ())
    }

    async fn put_object(&self, key: &str, body: &[u8]) -> Result<(), S3CallError> {
        let path = self.object_path(key);
        self.send(
            "PutObject",
            reqwest::Method::PUT,
            &path,
            "",
            body.to_vec(),
            true,
        )
        .await
        .map(|_| ())
    }

    async fn delete_object(&self, key: &str) -> Result<(), S3CallError> {
        let path = self.object_path(key);
        self.send(
            "DeleteObject",
            reqwest::Method::DELETE,
            &path,
            "",
            Vec::new(),
            false,
        )
        .await
        .map(|_| ())
    }

    async fn put_bucket_policy(&self, policy_json: &str) -> Result<(), S3CallError> {
        let bucket = self.bucket_path();
        self.send(
            "PutBucketPolicy",
            reqwest::Method::PUT,
            &bucket,
            "policy",
            policy_json.as_bytes().to_vec(),
            true,
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    fn service(code: &str, message: &str, operation: &'static str, status: u16) -> S3CallError {
        S3CallError::Service(S3ServiceError {
            code: code.to_string(),
            message: message.to_string(),
            operation,
            status,
        })
    }

    /// Scripted [`S3Ops`]: each method replays its queue in order and
    /// records every call (arguments included) for sequence assertions.
    #[derive(Default)]
    struct FakeS3 {
        head: RefCell<Vec<Result<(), S3CallError>>>,
        create: RefCell<Vec<Result<(), S3CallError>>>,
        lists: RefCell<Vec<Result<Vec<String>, S3CallError>>>,
        gets: RefCell<Vec<Result<(), S3CallError>>>,
        puts: RefCell<Vec<Result<(), S3CallError>>>,
        deletes: RefCell<Vec<Result<(), S3CallError>>>,
        policies: RefCell<Vec<Result<(), S3CallError>>>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeS3 {
        fn next<T>(queue: &RefCell<Vec<T>>) -> T {
            queue.borrow_mut().remove(0)
        }
    }

    impl S3Ops for FakeS3 {
        async fn head_bucket(&self) -> Result<(), S3CallError> {
            self.calls.borrow_mut().push("head".to_string());
            Self::next(&self.head)
        }
        async fn create_bucket(&self) -> Result<(), S3CallError> {
            self.calls.borrow_mut().push("create".to_string());
            Self::next(&self.create)
        }
        async fn list_keys(&self) -> Result<Vec<String>, S3CallError> {
            self.calls.borrow_mut().push("list".to_string());
            Self::next(&self.lists)
        }
        async fn get_object(&self, key: &str) -> Result<(), S3CallError> {
            self.calls.borrow_mut().push(format!("get:{key}"));
            Self::next(&self.gets)
        }
        async fn put_object(&self, key: &str, body: &[u8]) -> Result<(), S3CallError> {
            self.calls
                .borrow_mut()
                .push(format!("put:{key}:{}", body.len()));
            Self::next(&self.puts)
        }
        async fn delete_object(&self, key: &str) -> Result<(), S3CallError> {
            self.calls.borrow_mut().push(format!("delete:{key}"));
            Self::next(&self.deletes)
        }
        async fn put_bucket_policy(&self, policy_json: &str) -> Result<(), S3CallError> {
            self.calls
                .borrow_mut()
                .push(format!("policy:{policy_json}"));
            Self::next(&self.policies)
        }
    }

    /// A line sink plus its collected lines.
    fn sink() -> (Rc<RefCell<Vec<String>>>, impl FnMut(&str)) {
        let lines = Rc::new(RefCell::new(Vec::new()));
        let collected = Rc::clone(&lines);
        let out = move |line: &str| collected.borrow_mut().push(line.to_string());
        (lines, out)
    }

    /// An in-memory `write_file` plus its written files.
    #[allow(clippy::type_complexity)]
    fn memory_fs() -> (
        Rc<RefCell<HashMap<String, String>>>,
        impl Fn(&str, &str) -> std::io::Result<()>,
    ) {
        let files = Rc::new(RefCell::new(HashMap::new()));
        let stored = Rc::clone(&files);
        let write = move |path: &str, content: &str| {
            stored
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        };
        (files, write)
    }

    #[test]
    fn client_error_text_matches_botocore() {
        let error = S3ServiceError {
            code: "AccessDenied".to_string(),
            message: "Access Denied".to_string(),
            operation: "ListObjectsV2",
            status: 403,
        };
        assert_eq!(
            error.to_string(),
            "An error occurred (AccessDenied) when calling the ListObjectsV2 operation: Access Denied"
        );
        assert_eq!(
            S3CallError::NoCredentials.to_string(),
            "Unable to locate credentials"
        );
        assert_eq!(
            S3SetupError::NoRegion.to_string(),
            "You must specify a region."
        );
        assert_eq!(
            S3SetupError::BucketNone.to_string(),
            "expected string or bytes-like object, got 'NoneType'"
        );
    }

    #[test]
    fn setup_precedence_region_then_bucket() {
        let mut env = S3Env::default();
        assert_eq!(resolve_create_target(&env), Err(S3SetupError::NoRegion));
        env.region = Some("us-east-1".to_string());
        assert_eq!(resolve_create_target(&env), Err(S3SetupError::BucketNone));
        env.bucket = Some("b".to_string());
        let target = resolve_create_target(&env).expect("resolved");
        assert_eq!(target.region, "us-east-1");
        // Empty strings are passed through, like boto3 (verified: empty
        // region builds the client, empty credentials are used).
        env.region = Some(String::new());
        assert!(resolve_create_target(&env).is_ok());
        // update_bucket distinguishes the missing bucket (its own line)
        // from the missing region (fatal).
        let mut env = S3Env::default();
        assert_eq!(resolve_update_target(&env), Err(S3SetupError::NoRegion));
        env.region = Some("r".to_string());
        assert_eq!(resolve_update_target(&env), Ok(UpdateSetup::MissingBucket));
    }

    /// F37-02 create_bucket matrix, every branch.
    #[tokio::test]
    async fn create_bucket_branch_matrix() {
        // Exists.
        let fake = FakeS3 {
            head: vec![Ok(())].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            ["Checking bucket...", "Bucket 'bkt' exists."]
        );

        // 404 -> create -> created.
        let fake = FakeS3 {
            head: vec![Err(service("404", "Not Found", "HeadBucket", 404))].into(),
            create: vec![Ok(())].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' does not exist. Creating bucket...",
                "Bucket 'bkt' created successfully.",
            ]
        );

        // 404 -> create fails with a service error.
        let fake = FakeS3 {
            head: vec![Err(service("404", "Not Found", "HeadBucket", 404))].into(),
            create: vec![Err(service(
                "BucketAlreadyExists",
                "The requested bucket name is not available.",
                "CreateBucket",
                409,
            ))]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' does not exist. Creating bucket...",
                "Failed to create bucket: An error occurred (BucketAlreadyExists) when calling the CreateBucket operation: The requested bucket name is not available.",
            ]
        );

        // 404 -> create fails with a transport error: the inner `except
        // ClientError` misses it, the outer `except Exception` catches it.
        let fake = FakeS3 {
            head: vec![Err(service("404", "Not Found", "HeadBucket", 404))].into(),
            create: vec![Err(S3CallError::Transport(
                "Could not connect to the endpoint URL: \"http://x/bkt\"".to_string(),
            ))]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' does not exist. Creating bucket...",
                "An error occurred: Could not connect to the endpoint URL: \"http://x/bkt\"",
            ]
        );

        // 403.
        let fake = FakeS3 {
            head: vec![Err(service("403", "Forbidden", "HeadBucket", 403))].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Access to the bucket 'bkt' is forbidden. Check permissions.",
            ]
        );

        // Any other numeric code.
        let fake = FakeS3 {
            head: vec![Err(service("405", "Method Not Allowed", "HeadBucket", 405))].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Failed to check bucket: An error occurred (405) when calling the HeadBucket operation: Method Not Allowed",
            ]
        );

        // Symbolic code: `int()` raises out of `handle` (exit 1).
        let fake = FakeS3 {
            head: vec![Err(service(
                "NoSuchBucket",
                "The specified bucket does not exist",
                "HeadBucket",
                404,
            ))]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        let result = run_create_bucket(&fake, "bkt", &mut out).await;
        assert_eq!(result, Err(NonNumericCode("NoSuchBucket".to_string())));
        assert_eq!(lines.borrow().as_slice(), ["Checking bucket..."]);

        // Transport on head: outer `except Exception`.
        let fake = FakeS3 {
            head: vec![Err(S3CallError::Transport(
                "Could not connect to the endpoint URL: \"http://x/bkt\"".to_string(),
            ))]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "An error occurred: Could not connect to the endpoint URL: \"http://x/bkt\"",
            ]
        );

        // Missing credentials on head: same outer arm.
        let fake = FakeS3 {
            head: vec![Err(S3CallError::NoCredentials)].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_create_bucket(&fake, "bkt", &mut out).await.expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "An error occurred: Unable to locate credentials",
            ]
        );
    }

    /// F37-02 update_bucket success path: probe call order, the policy
    /// probe side effect, the make-public policy shape, cleanup.
    #[tokio::test]
    async fn update_bucket_success_path() {
        let fake = FakeS3 {
            head: vec![Ok(())].into(),
            // Probe 1 list, probe 2 list, make-public list.
            lists: vec![
                Ok(vec!["a.txt".to_string()]),
                Ok(vec!["a.txt".to_string()]),
                Ok(vec!["a.txt".to_string(), "b.txt".to_string()]),
            ]
            .into(),
            gets: vec![Ok(())].into(),
            puts: vec![Ok(())].into(),
            deletes: vec![Ok(())].into(),
            policies: vec![Ok(()), Ok(())].into(),
            ..Default::default()
        };
        let (files, write_file) = memory_fs();
        let (lines, mut out) = sink();
        run_update_bucket(&fake, "bkt", &write_file, &mut out)
            .await
            .expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' exists.",
                "Access key has the required permissions.",
                "Bucket is private, but existing objects remain public.",
            ]
        );
        let calls = fake.calls.borrow();
        // head, list, list, get, put, delete, policy-probe, list,
        // policy-apply.
        assert_eq!(calls[0], "head");
        assert_eq!(calls[1], "list");
        assert_eq!(calls[2], "list");
        assert_eq!(calls[3], "get:a.txt");
        assert_eq!(calls[4], "put:test_permission_check.txt:4");
        assert_eq!(calls[5], "delete:test_permission_check.txt");
        assert!(calls[6].starts_with("policy:"));
        // The probe policy is the public-read wildcard (the side effect).
        assert!(calls[6].contains("\"Resource\": \"arn:aws:s3:::bkt/*\""));
        assert_eq!(calls[7], "list");
        // The applied policy names each existing object.
        assert!(calls[8].contains("arn:aws:s3:::bkt/a.txt"));
        assert!(calls[8].contains("arn:aws:s3:::bkt/b.txt"));
        assert_eq!(calls.len(), 9);
        // No fallback file on the success path.
        assert!(files.borrow().is_empty());
    }

    /// F37-02 update_bucket: 404 head, non-404 head fallthrough, denied
    /// probes with fallback, quirk paths, failed fallback write.
    #[tokio::test]
    async fn update_bucket_error_and_fallback_paths() {
        // 404 head: the error line, then return.
        let fake = FakeS3 {
            head: vec![Err(service("404", "Not Found", "HeadBucket", 404))].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_update_bucket(&fake, "bkt", &|_, _| Ok(()), &mut out)
            .await
            .expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            ["Checking bucket...", "Bucket 'bkt' does not exist.",]
        );

        // Non-404 head error: `Error:` then the fallthrough success line;
        // denied probes then take the permissions.json fallback.
        let fake = FakeS3 {
            head: vec![Err(service("403", "Forbidden", "HeadBucket", 403))].into(),
            lists: vec![Ok(vec![]), Ok(vec![]), Ok(vec![])].into(),
            puts: vec![Err(service(
                "AccessDenied",
                "Access Denied",
                "PutObject",
                403,
            ))]
            .into(),
            deletes: vec![Err(service(
                "AccessDenied",
                "Access Denied",
                "DeleteObject",
                403,
            ))]
            .into(),
            policies: vec![Err(service(
                "AccessDenied",
                "Access Denied",
                "PutBucketPolicy",
                403,
            ))]
            .into(),
            ..Default::default()
        };
        let (files, write_file) = memory_fs();
        let (lines, mut out) = sink();
        run_update_bucket(&fake, "bkt", &write_file, &mut out)
            .await
            .expect("ok");
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Error: An error occurred (403) when calling the HeadBucket operation: Forbidden",
                "Bucket 'bkt' exists.",
                // Probe 2's list succeeds empty: GetObject silently skips.
                "PutObject permission denied.",
                "Couldn't delete test object",
                "PutBucketPolicy permission denied.",
                "Generating permissions.json for manual bucket policy update.",
                "Permissions have been written to permissions.json.",
            ]
        );
        assert_eq!(
            files
                .borrow()
                .get("permissions.json")
                .map(String::as_str),
            Some("{\"Version\": \"2012-10-17\", \"Statement\": [{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": []}]}")
        );

        // Transport on head: escapes `handle` (only `ClientError` is
        // caught there).
        let fake = FakeS3 {
            head: vec![Err(S3CallError::Transport("down".to_string()))].into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        let result = run_update_bucket(&fake, "bkt", &|_, _| Ok(()), &mut out).await;
        assert_eq!(result, Err(S3CallError::Transport("down".to_string())));
        assert_eq!(lines.borrow().as_slice(), ["Checking bucket..."]);

        // Transport inside the check: `Error:`, the unbound-`permissions`
        // line, then the fallback list fails too (escapes).
        let fake = FakeS3 {
            head: vec![Ok(())].into(),
            lists: vec![
                Err(S3CallError::Transport("down".to_string())),
                Err(S3CallError::Transport("down".to_string())),
            ]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        let result = run_update_bucket(&fake, "bkt", &|_, _| Ok(()), &mut out).await;
        assert_eq!(result, Err(S3CallError::Transport("down".to_string())));
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' exists.",
                "Error: down",
                "Error: cannot access local variable 'permissions' where it is not associated with a value",
                "Generating permissions.json for manual bucket policy update.",
            ]
        );

        // Missing credentials inside the check: same quirk shape.
        let fake = FakeS3 {
            head: vec![Ok(())].into(),
            lists: vec![
                Err(S3CallError::NoCredentials),
                Err(S3CallError::NoCredentials),
            ]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        let result = run_update_bucket(&fake, "bkt", &|_, _| Ok(()), &mut out).await;
        assert_eq!(result, Err(S3CallError::NoCredentials));
        assert_eq!(
            lines.borrow().as_slice(),
            [
                "Checking bucket...",
                "Bucket 'bkt' exists.",
                "Error: Unable to locate credentials",
                "Error: cannot access local variable 'permissions' where it is not associated with a value",
                "Generating permissions.json for manual bucket policy update.",
            ]
        );

        // Failed fallback file write: the `IOError` line (CPython text).
        let fake = FakeS3 {
            head: vec![Ok(())].into(),
            lists: vec![Ok(vec![]), Ok(vec![]), Ok(vec![])].into(),
            puts: vec![Err(service(
                "AccessDenied",
                "Access Denied",
                "PutObject",
                403,
            ))]
            .into(),
            deletes: vec![Ok(())].into(),
            policies: vec![Err(service(
                "AccessDenied",
                "Access Denied",
                "PutBucketPolicy",
                403,
            ))]
            .into(),
            ..Default::default()
        };
        let (lines, mut out) = sink();
        run_update_bucket(
            &fake,
            "bkt",
            &|_, _| Err(std::io::Error::from_raw_os_error(13)),
            &mut out,
        )
        .await
        .expect("ok");
        assert!(lines.borrow().iter().any(|line| line
            == "Error writing permissions.json: [Errno 13] Permission denied: 'permissions.json'"));
    }

    /// Policy JSON is CPython `json.dumps` byte for byte (goldens
    /// generated with the deployment interpreter).
    #[test]
    fn policy_json_matches_cpython_dumps() {
        assert_eq!(
            probe_policy_json("b"),
            "{\"Version\": \"2012-10-17\", \"Statement\": [{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": \"arn:aws:s3:::b/*\"}]}"
        );
        assert_eq!(
            bucket_policy_json("b", &[]),
            "{\"Version\": \"2012-10-17\", \"Statement\": [{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": []}]}"
        );
        assert_eq!(
            bucket_policy_json("b", &["a.txt".to_string(), "b.txt".to_string()]),
            "{\"Version\": \"2012-10-17\", \"Statement\": [{\"Effect\": \"Allow\", \"Principal\": \"*\", \"Action\": \"s3:GetObject\", \"Resource\": [\"arn:aws:s3:::b/a.txt\", \"arn:aws:s3:::b/b.txt\"]}]}"
        );
        // `json.dumps({"k": "a\"b\\c\nd\x01\x7f\xe9\U0001F600/e"})` inner value.
        assert_eq!(
            json_escape("a\"b\\c\nd\x01\x7f\u{e9}\u{1F600}/e"),
            "a\\\"b\\\\c\\nd\\u0001\\u007f\\u00e9\\ud83d\\ude00/e"
        );
        assert_eq!(json_escape(" "), " ");
        assert_eq!(json_escape("\u{0}"), "\\u0000");
        assert_eq!(json_escape("~"), "~");
    }

    /// SigV4 vectors generated with botocore's own signer (frozen
    /// 2026-10-10T03:00:00Z, AKIDEXAMPLE/SECRETEXAMPLE, us-east-1).
    #[test]
    fn signer_matches_botocore_vectors() {
        let empty = pidash_storage::sha256_hex(&[]);
        // GET /my-bucket?list-type=2&encoding-type=url
        assert_eq!(
            sign_s3_request(
                "AKIDEXAMPLE",
                "SECRETEXAMPLE",
                "us-east-1",
                "GET",
                "/my-bucket",
                "list-type=2&encoding-type=url",
                "localhost:4566",
                &empty,
                "20261010T030000Z",
                "20261010",
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261010/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=da1275281aeaec035a90d8ab08cdb2cc8dda5fad94324d02d7d09dcc55a296fc"
        );
        // PUT /my-bucket/test_permission_check.txt, body "Test".
        let test_hash = pidash_storage::sha256_hex(b"Test");
        assert_eq!(
            sign_s3_request(
                "AKIDEXAMPLE",
                "SECRETEXAMPLE",
                "us-east-1",
                "PUT",
                "/my-bucket/test_permission_check.txt",
                "",
                "localhost:4566",
                &test_hash,
                "20261010T030000Z",
                "20261010",
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261010/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=10bb4b8c5ea0dbea55cd3bd41d03e72fb73978d90de323c3fb5359254d1edf76"
        );
        // PUT /my-bucket?policy — the valueless query signs as `policy=`.
        assert_eq!(canonical_query_string("policy"), "policy=");
        let policy_hash = pidash_storage::sha256_hex(b"{\"a\":1}");
        assert_eq!(
            sign_s3_request(
                "AKIDEXAMPLE",
                "SECRETEXAMPLE",
                "us-east-1",
                "PUT",
                "/my-bucket",
                "policy",
                "localhost:4566",
                &policy_hash,
                "20261010T030000Z",
                "20261010",
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261010/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=d63ef1651acc0e0434f166b7a713ff90fd64817e0b72e2010f80e4d59ac6c033"
        );
        // HEAD /my-bucket.
        assert_eq!(
            sign_s3_request(
                "AKIDEXAMPLE",
                "SECRETEXAMPLE",
                "us-east-1",
                "HEAD",
                "/my-bucket",
                "",
                "localhost:4566",
                &empty,
                "20261010T030000Z",
                "20261010",
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261010/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=9bed2418ff88a8921c7757f9d199bf5bfc8133e8461498495dc11d6841741f95"
        );
    }

    #[test]
    fn path_encoding_and_key_decoding() {
        assert_eq!(uri_encode_path("a.txt"), "a.txt");
        assert_eq!(uri_encode_path("odd key+&.txt"), "odd%20key%2B%26.txt");
        assert_eq!(uri_encode_path("a/b"), "a/b");
        assert_eq!(uri_encode_path("caf\u{e9}.txt"), "caf%C3%A9.txt");
        assert_eq!(uri_decode_key("odd%20key%2B%26.txt"), "odd key+&.txt");
        assert_eq!(uri_decode_key("a+b"), "a+b");
        assert_eq!(uri_decode_key("%zz"), "%zz");
    }

    #[test]
    fn error_xml_and_list_parsing() {
        let (code, message) = parse_error_xml(
            br#"<?xml version="1.0" ?><Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>"#,
        )
        .expect("parses");
        assert_eq!(
            (code.as_str(), message.as_str()),
            ("AccessDenied", "Access Denied")
        );
        assert_eq!(parse_error_xml(b""), None);
        assert_eq!(parse_error_xml(b"<html>nope</html>"), None);
        assert_eq!(
            parse_list_keys(
                br#"<?xml version="1.0" ?><ListBucketResult><KeyCount>2</KeyCount><Contents><Key>a.txt</Key></Contents><Contents><Key>odd%20key.txt</Key></Contents></ListBucketResult>"#
            ),
            vec!["a.txt".to_string(), "odd key.txt".to_string()]
        );
        assert!(parse_list_keys(
            br#"<?xml version="1.0" ?><ListBucketResult><KeyCount>0</KeyCount></ListBucketResult>"#
        )
        .is_empty());
        assert_eq!(xml_unescape("a &amp; b &#65; &#x42;"), "a & b A B");
    }

    #[test]
    fn request_base_resolution() {
        // Explicit endpoint: path-style against it.
        assert_eq!(
            resolve_request_base(Some("http://localhost:4566/"), "us-east-1", "bkt"),
            (
                "http://localhost:4566".to_string(),
                "localhost:4566".to_string(),
                "/bkt".to_string(),
            )
        );
        // AWS default: virtual-hosted, with the us-east-1 global host.
        assert_eq!(
            resolve_request_base(None, "us-east-1", "bkt"),
            (
                "https://bkt.s3.amazonaws.com".to_string(),
                "bkt.s3.amazonaws.com".to_string(),
                String::new(),
            )
        );
        assert_eq!(
            resolve_request_base(None, "eu-west-1", "bkt"),
            (
                "https://bkt.s3.eu-west-1.amazonaws.com".to_string(),
                "bkt.s3.eu-west-1.amazonaws.com".to_string(),
                String::new(),
            )
        );
    }
}
