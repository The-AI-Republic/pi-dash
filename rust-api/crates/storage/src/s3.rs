//! Offline SigV4 request signing for server-side S3 PUT and DELETE.
//!
//! Mirrors `S3Storage` (`settings/storage.py`) the way the D-02 presign
//! helpers do (`crates/api/src/space/assets.rs:1020-1078`): pure-HMAC SigV4,
//! no AWS SDK. Signing itself is pure; credential *resolution* may hit
//! IMDS (see [`crate::aws_credentials`]) but the signers only take the
//! resolved [`AwsCredentials`]. Endpoint/region/bucket come from
//! [`StorageSettings`](pidash_db::config::StorageSettings).
//!
//! Endpoint parity: [`resolve_server_endpoint`] resolves exactly like
//! `S3Storage(request)` with the default `is_server=False` (the constructor
//! the avatar flow uses in `adapter/base.py:198,290`) — MinIO mode signs
//! `{scheme}://{Host}` path-style, an explicit endpoint URL signs path-style
//! against it, otherwise the virtual-hosted AWS default. The caller passes
//! the scheme/host of the incoming request, same as the D-02 handlers.

use hmac::{Hmac, Mac};
use pidash_db::config::StorageSettings;
use sha2::{Digest, Sha256};

use crate::aws_credentials::{
    capped_presign_expires_secs, is_placeholder_credentials, AwsCredentials, CredentialSource,
};

type HmacSha256 = Hmac<Sha256>;

/// Caller-visible S3 failure.
///
/// Every variant maps to the OAuth caller's provider-URL fallback branch
/// (`adapter/base.py`: `upload_file` False / `get_object_metadata` None /
/// `delete_files` False are all swallowed) — never to a 500.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StorageError {
    /// The request never completed (timeout, DNS, connection reset).
    /// Python: `requests` raising inside `download_and_upload_avatar`
    /// (`base.py:150-152,236`) is caught by the blanket `except` and
    /// returns `None`.
    #[error("s3 transport failure: {0}")]
    Transport(String),
    /// S3 answered with an error status. Python: `ClientError` from
    /// `upload_fileobj` / `head_object` / `delete_objects` is logged and
    /// swallowed (`storage.py`, every method).
    #[error("s3 service error {status}: {message}")]
    Service { status: u16, message: String },
    /// The configured storage cannot be used (empty bucket or
    /// credentials, or a region that cannot be signed — invalid host
    /// label, or empty with no endpoint URL to derive from, which
    /// Python raises as `ValueError`/`InvalidRegionError` from the
    /// `S3Storage` constructor). Python surfaces these lazily as a
    /// `ClientError`/`ParamValidationError` on first use and swallows
    /// them the same way; this variant lets the caller pre-flight into
    /// the fallback branch without network I/O.
    #[error("s3 misconfigured: {0}")]
    Config(String),
}

/// Pre-flight check: bucket and credentials are present.
///
/// Not a Python behavior port (Python fails lazily on first use) — a
/// convenience so the caller can take the fallback branch before any I/O.
///
/// The placeholder keys `access-key` / `secret-key` are local-MinIO only
/// (`USE_MINIO=1`); outside MinIO they are treated as "not configured" and
/// fail here with a clear message instead of a misleading
/// `InvalidAccessKeyId` from AWS (NEWPIDASH-204).
pub fn validate_storage(storage: &StorageSettings) -> Result<(), StorageError> {
    if storage.bucket_name.is_empty() {
        return Err(StorageError::Config("bucket name is empty".to_owned()));
    }
    if is_placeholder_credentials(&storage.access_key_id, &storage.secret_access_key)
        && !storage.use_minio
    {
        return Err(StorageError::Config(
            "access credentials are not configured (placeholder keys are only valid with USE_MINIO=1)"
                .to_owned(),
        ));
    }
    if storage.access_key_id.is_empty() || storage.secret_access_key.is_empty() {
        return Err(StorageError::Config(
            "access credentials are not configured (AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY are unset and no instance role is available)"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Resolve the credentials to sign with for `storage`, applying boto3's
/// chain (`AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY` first, then the EC2
/// instance role) and keeping the placeholder keys only for `USE_MINIO=1`.
///
/// Should be called per request: role credentials rotate about hourly, so
/// they must not be cached in `StorageSettings`. The static path returns the
/// boot-time keys (plus `AWS_SESSION_TOKEN`); the instance-role path hits
/// the process-wide IMDS cache (`pidash_storage::aws_credentials`).
pub async fn resolve_storage_credentials(
    storage: &StorageSettings,
) -> Result<AwsCredentials, StorageError> {
    resolve_storage_credentials_with_source(storage, CredentialSource::from_env()).await
}

/// [`resolve_storage_credentials`] over an explicit [`CredentialSource`].
///
/// Useful for tests that must not mutate the process environment or share the
/// process-wide IMDS cache: construct a per-test
/// `CredentialSource::InstanceRole(Arc::new(InstanceRoleProvider::new(base)))`
/// against a fake IMDS and pass it here.
pub async fn resolve_storage_credentials_with_source(
    storage: &StorageSettings,
    source: CredentialSource,
) -> Result<AwsCredentials, StorageError> {
    let is_placeholder =
        is_placeholder_credentials(&storage.access_key_id, &storage.secret_access_key);
    let has_static = !storage.access_key_id.is_empty()
        && !storage.secret_access_key.is_empty()
        && !(is_placeholder && !storage.use_minio);
    if has_static {
        return Ok(AwsCredentials {
            access_key: storage.access_key_id.clone(),
            secret_key: storage.secret_access_key.clone(),
            session_token: storage.session_token.clone(),
            expires_at: None,
        });
    }
    if storage.use_minio && is_placeholder {
        return Ok(AwsCredentials {
            access_key: storage.access_key_id.clone(),
            secret_key: storage.secret_access_key.clone(),
            session_token: storage.session_token.clone(),
            expires_at: None,
        });
    }
    // No usable static keys: fall through to the credential chain.
    source.check_static().map_err(StorageError::Config)?;
    source.resolve().await.map_err(StorageError::Config)
}

/// Resolve `storage` into a signable copy: fills `access_key_id`,
/// `secret_access_key` and `session_token` from the per-request resolution
/// and caps `signed_url_expiration_secs` so a presigned URL does not outlive
/// temporary (role) credentials. Static keys leave the expiry unchanged.
pub async fn resolve_storage_settings(
    storage: &StorageSettings,
) -> Result<StorageSettings, StorageError> {
    resolve_storage_settings_with_source(storage, CredentialSource::from_env()).await
}

/// [`resolve_storage_settings`] over an explicit [`CredentialSource`].
pub async fn resolve_storage_settings_with_source(
    storage: &StorageSettings,
    source: CredentialSource,
) -> Result<StorageSettings, StorageError> {
    let creds = resolve_storage_credentials_with_source(storage, source).await?;
    let mut resolved = storage.clone();
    resolved.access_key_id = creds.access_key.clone();
    resolved.secret_access_key = creds.secret_key.clone();
    resolved.session_token = creds.session_token.clone();
    if let Some(expires_at) = creds.expires_at {
        let now = chrono::Utc::now();
        let remaining = (expires_at - now).num_seconds().max(0);
        resolved.signed_url_expiration_secs = resolved.signed_url_expiration_secs.min(remaining);
    }
    Ok(resolved)
}

/// Resolve credentials without a `StorageSettings` (export uploader and
/// bucket commands read their env directly): the same boto3 chain as
/// [`resolve_storage_credentials`].
pub async fn resolve_default_credentials() -> Result<AwsCredentials, StorageError> {
    let source = CredentialSource::from_env();
    source.check_static().map_err(StorageError::Config)?;
    source.resolve().await.map_err(StorageError::Config)
}

/// Cap a presigned-URL expiry so it does not outlive temporary (role)
/// credentials. Re-exported for callers that presign with role creds:
/// `requested` is the caller-requested expiry, `creds` the resolved creds,
/// `now` the signing instant (as `DateTime<Utc>`).
pub fn capped_expires_secs(
    requested: u64,
    creds: &AwsCredentials,
    now: chrono::DateTime<chrono::Utc>,
) -> u64 {
    capped_presign_expires_secs(requested, creds, now)
}

/// Resolved endpoint for signing: base URL, signed `host`, path style.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedEndpoint {
    /// Base URL the object path is appended to (no trailing slash).
    pub url_base: String,
    /// Value signed as the `host` header.
    pub signed_host: String,
    /// Path-style (`/{bucket}/{key}`) when true, virtual-hosted (`/{key}`)
    /// when false.
    pub path_style: bool,
}

/// Endpoint + host split mirroring `S3Storage.__init__` with a request
/// (`is_server=False`) and the D-02 `endpoint_parts` helper. Server-side
/// signing is header auth, so regional clients keep the regional host
/// (no `use_global_endpoint` rewrite — that is presigning only).
pub fn resolve_server_endpoint(
    storage: &StorageSettings,
    scheme: &str,
    host: &str,
) -> Result<ResolvedEndpoint, StorageError> {
    if !pidash_db::config::is_valid_region_name(&storage.region) {
        return Err(StorageError::Config(
            "region is not a valid host label".to_owned(),
        ));
    }
    // `MINIO_ENDPOINT_SSL=1` signs https in MinIO mode (`storage.py:46-51`).
    let scheme = storage.endpoint_protocol(scheme);
    if storage.use_minio {
        Ok(ResolvedEndpoint {
            url_base: format!("{scheme}://{host}"),
            signed_host: host.to_owned(),
            path_style: true,
        })
    } else if let Some(endpoint) = storage.endpoint_url.as_deref().filter(|e| !e.is_empty()) {
        let endpoint = endpoint.trim_end_matches('/');
        let signed_host = endpoint
            .rsplit("://")
            .next()
            .unwrap_or(endpoint)
            .split('/')
            .next()
            .unwrap_or(endpoint);
        Ok(ResolvedEndpoint {
            url_base: endpoint.to_owned(),
            signed_host: signed_host.to_owned(),
            path_style: true,
        })
    } else {
        let region = storage.region.as_str();
        if region.is_empty() {
            // botocore derives `https://s3..amazonaws.com` and rejects
            // it (`ValueError: Invalid endpoint`).
            return Err(StorageError::Config(
                "empty region with no endpoint URL to derive from".to_owned(),
            ));
        }
        // botocore's s3 endpoint table serves us-east-1 from the global
        // endpoint (`s3.amazonaws.com`, no region infix); every other
        // region is virtual-hosted regional.
        let base = if region == "us-east-1" {
            "s3.amazonaws.com".to_owned()
        } else {
            format!("s3.{region}.amazonaws.com")
        };
        Ok(ResolvedEndpoint {
            url_base: format!("https://{}.{base}", storage.bucket_name),
            signed_host: format!("{}.{base}", storage.bucket_name),
            path_style: false,
        })
    }
}

/// A signed server-side request, ready to execute.
///
/// Send with the HTTP client as `method url` carrying exactly these headers:
/// `authorization`, `x-amz-date`, `x-amz-content-sha256`, plus
/// `content-type` when present, `x-amz-security-token` when
/// [`SignedRequest::security_token`] is `Some`, and every
/// [`SignedRequest::extra_headers`] entry. Any added, dropped, or re-cased
/// signed header invalidates the signature.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedRequest {
    /// `PUT` or `DELETE`.
    pub method: &'static str,
    /// Full object URL (`{url_base}/{bucket}/{key}` path-style).
    pub url: String,
    /// Value for the `Authorization` header.
    pub authorization: String,
    /// Value for the `x-amz-date` header (`%Y%m%dT%H%M%SZ`).
    pub amz_date: String,
    /// Value for the `x-amz-content-sha256` header (real payload hash).
    pub content_sha256: String,
    /// Value for the `Content-Type` header (PUT only).
    pub content_type: Option<String>,
    /// `X-Amz-Security-Token` header value for temporary (role) credentials;
    /// `None` for long-lived keys. Must be sent as `x-amz-security-token` and
    /// is included in the SigV4 canonical request when present.
    pub security_token: Option<String>,
    /// Extra signed headers to send verbatim (`(lowercase-name, value)`).
    pub extra_headers: Vec<(String, String)>,
}

/// Sign a server-side PUT of `payload` bytes as `object_key`.
///
/// Mirrors `S3Storage.upload_file` (`storage.py:186-207`): the object is
/// stored with `ContentType` and addressed by key. Returns the object key's
/// URL and headers — the caller executes the PUT and treats any failure as
/// [`StorageError`]. Credentials come from `storage` (static env keys);
/// for per-request role credentials use [`sign_put_with_creds`] with
/// [`resolve_storage_credentials`].
// Eight positional inputs mirror the D-02 presign-helper shape; grouping
// them would only rename the tuple.
#[allow(clippy::too_many_arguments)]
pub fn sign_put(
    storage: &StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    content_type: &str,
    payload: &[u8],
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let creds = AwsCredentials {
        access_key: storage.access_key_id.clone(),
        secret_key: storage.secret_access_key.clone(),
        session_token: storage.session_token.clone(),
        expires_at: None,
    };
    sign_put_with_creds(
        storage,
        &creds,
        scheme,
        host,
        object_key,
        content_type,
        payload,
        amz_datetime,
        date_stamp,
    )
}

/// Sign a PUT with explicit credentials (per-request role creds). The
/// `x-amz-security-token` header is signed and returned on the
/// [`SignedRequest`] when `creds.session_token` is non-empty.
#[allow(clippy::too_many_arguments)]
pub fn sign_put_with_creds(
    storage: &StorageSettings,
    creds: &AwsCredentials,
    scheme: &str,
    host: &str,
    object_key: &str,
    content_type: &str,
    payload: &[u8],
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let payload_hash = sha256_hex(payload);
    let endpoint = resolve_server_endpoint(storage, scheme, host)?;
    let path = canonical_path(&endpoint, &storage.bucket_name, object_key);
    let token = creds.session_token.as_str();
    let has_token = !token.is_empty();
    // SigV4 requires canonical headers sorted by name (botocore sorts too).
    let (canonical, signed_headers) = if has_token {
        let signed = "content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token";
        let canonical = format!(
            "PUT\n{path}\n\ncontent-type:{content_type}\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\nx-amz-security-token:{token}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    } else {
        let signed = "content-type;host;x-amz-content-sha256;x-amz-date";
        let canonical = format!(
            "PUT\n{path}\n\ncontent-type:{content_type}\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    };
    let authorization = authorization_with_creds(
        creds,
        &storage.region,
        &canonical,
        amz_datetime,
        date_stamp,
        &signed_headers,
    );
    Ok(SignedRequest {
        method: "PUT",
        url: format!("{}{path}", endpoint.url_base),
        authorization,
        amz_date: amz_datetime.to_owned(),
        content_sha256: payload_hash,
        content_type: Some(content_type.to_owned()),
        security_token: if has_token {
            Some(token.to_owned())
        } else {
            None
        },
        extra_headers: Vec::new(),
    })
}

/// Sign a server-side PUT with extra signed headers (the MinIO export
/// upload, `export_task.py:57-62`, signs `x-amz-acl: public-read` along
/// the `ContentType`). `extra` entries are `(lowercase-name, value)`;
/// the canonical headers sort by name (botocore sorts too) and the
/// returned [`SignedRequest::extra_headers`] carries the same pairs for
/// the caller to send. Otherwise identical to [`sign_put`].
#[allow(clippy::too_many_arguments)]
pub fn sign_put_extra(
    storage: &StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    content_type: &str,
    payload: &[u8],
    extra: &[(&str, &str)],
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let creds = AwsCredentials {
        access_key: storage.access_key_id.clone(),
        secret_key: storage.secret_access_key.clone(),
        session_token: storage.session_token.clone(),
        expires_at: None,
    };
    sign_put_extra_with_creds(
        storage,
        &creds,
        scheme,
        host,
        object_key,
        content_type,
        payload,
        extra,
        amz_datetime,
        date_stamp,
    )
}

/// Sign a PUT with extra headers and explicit credentials.
#[allow(clippy::too_many_arguments)]
pub fn sign_put_extra_with_creds(
    storage: &StorageSettings,
    creds: &AwsCredentials,
    scheme: &str,
    host: &str,
    object_key: &str,
    content_type: &str,
    payload: &[u8],
    extra: &[(&str, &str)],
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let payload_hash = sha256_hex(payload);
    let endpoint = resolve_server_endpoint(storage, scheme, host)?;
    let path = canonical_path(&endpoint, &storage.bucket_name, object_key);
    let token = creds.session_token.clone();
    let has_token = !token.is_empty();
    let mut names: Vec<&str> = vec!["content-type", "host", "x-amz-content-sha256", "x-amz-date"];
    names.extend(extra.iter().map(|(name, _)| *name));
    if has_token {
        names.push("x-amz-security-token");
    }
    names.sort_unstable();
    let value_of = |name: &str| match name {
        "content-type" => content_type.to_owned(),
        "host" => endpoint.signed_host.clone(),
        "x-amz-content-sha256" => payload_hash.clone(),
        "x-amz-date" => amz_datetime.to_owned(),
        "x-amz-security-token" => token.clone(),
        other => extra
            .iter()
            .find(|(key, _)| *key == other)
            .map(|(_, value)| (*value).to_owned())
            .unwrap_or_default(),
    };
    let canonical_headers = names
        .iter()
        .map(|name| format!("{name}:{}\n", value_of(name)))
        .collect::<String>();
    let signed_headers = names.join(";");
    let canonical = format!("PUT\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let authorization = authorization_with_creds(
        creds,
        &storage.region,
        &canonical,
        amz_datetime,
        date_stamp,
        &signed_headers,
    );
    Ok(SignedRequest {
        method: "PUT",
        url: format!("{}{path}", endpoint.url_base),
        authorization,
        amz_date: amz_datetime.to_owned(),
        content_sha256: payload_hash,
        content_type: Some(content_type.to_owned()),
        security_token: if has_token { Some(token) } else { None },
        extra_headers: extra
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    })
}

/// Sign a server-side HEAD of `object_key`.
///
/// Mirrors `S3Storage.get_object_metadata` (`storage.py:156-174`): one signed
/// `HEAD` per key, answered headers projected through
/// [`crate::avatar::head_to_storage_metadata`]. The caller executes it and
/// treats any failure as [`StorageError`] (Python logs and records `None`,
/// so the row is still created with NULL metadata).
pub fn sign_head(
    storage: &StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let creds = AwsCredentials {
        access_key: storage.access_key_id.clone(),
        secret_key: storage.secret_access_key.clone(),
        session_token: storage.session_token.clone(),
        expires_at: None,
    };
    sign_head_with_creds(
        storage,
        &creds,
        scheme,
        host,
        object_key,
        amz_datetime,
        date_stamp,
    )
}

/// Sign HEAD with explicit credentials.
pub fn sign_head_with_creds(
    storage: &StorageSettings,
    creds: &AwsCredentials,
    scheme: &str,
    host: &str,
    object_key: &str,
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let payload_hash = sha256_hex(&[]);
    let endpoint = resolve_server_endpoint(storage, scheme, host)?;
    let path = canonical_path(&endpoint, &storage.bucket_name, object_key);
    let token = creds.session_token.as_str();
    let has_token = !token.is_empty();
    let (canonical, signed_headers) = if has_token {
        let signed = "host;x-amz-content-sha256;x-amz-date;x-amz-security-token";
        let canonical = format!(
            "HEAD\n{path}\n\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\nx-amz-security-token:{token}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    } else {
        let signed = "host;x-amz-content-sha256;x-amz-date";
        let canonical = format!(
            "HEAD\n{path}\n\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    };
    let authorization = authorization_with_creds(
        creds,
        &storage.region,
        &canonical,
        amz_datetime,
        date_stamp,
        &signed_headers,
    );
    Ok(SignedRequest {
        method: "HEAD",
        url: format!("{}{path}", endpoint.url_base),
        authorization,
        amz_date: amz_datetime.to_owned(),
        content_sha256: payload_hash,
        content_type: None,
        security_token: if has_token {
            Some(token.to_owned())
        } else {
            None
        },
        extra_headers: Vec::new(),
    })
}

/// Sign a server-side DELETE of `object_key`.
///
/// Mirrors `S3Storage.delete_files` (`storage.py:209-220`) for the single-key
/// avatar case (`adapter/base.py:290`): one signed `DELETE` per key. The
/// caller executes it and treats any failure as [`StorageError`].
pub fn sign_delete(
    storage: &StorageSettings,
    scheme: &str,
    host: &str,
    object_key: &str,
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let creds = AwsCredentials {
        access_key: storage.access_key_id.clone(),
        secret_key: storage.secret_access_key.clone(),
        session_token: storage.session_token.clone(),
        expires_at: None,
    };
    sign_delete_with_creds(
        storage,
        &creds,
        scheme,
        host,
        object_key,
        amz_datetime,
        date_stamp,
    )
}

/// Sign DELETE with explicit credentials.
pub fn sign_delete_with_creds(
    storage: &StorageSettings,
    creds: &AwsCredentials,
    scheme: &str,
    host: &str,
    object_key: &str,
    amz_datetime: &str,
    date_stamp: &str,
) -> Result<SignedRequest, StorageError> {
    let payload_hash = sha256_hex(&[]);
    let endpoint = resolve_server_endpoint(storage, scheme, host)?;
    let path = canonical_path(&endpoint, &storage.bucket_name, object_key);
    let token = creds.session_token.as_str();
    let has_token = !token.is_empty();
    let (canonical, signed_headers) = if has_token {
        let signed = "host;x-amz-content-sha256;x-amz-date;x-amz-security-token";
        let canonical = format!(
            "DELETE\n{path}\n\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\nx-amz-security-token:{token}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    } else {
        let signed = "host;x-amz-content-sha256;x-amz-date";
        let canonical = format!(
            "DELETE\n{path}\n\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_datetime}\n\n{signed}\n{payload_hash}",
            endpoint.signed_host,
        );
        (canonical, signed.to_owned())
    };
    let authorization = authorization_with_creds(
        creds,
        &storage.region,
        &canonical,
        amz_datetime,
        date_stamp,
        &signed_headers,
    );
    Ok(SignedRequest {
        method: "DELETE",
        url: format!("{}{path}", endpoint.url_base),
        authorization,
        amz_date: amz_datetime.to_owned(),
        content_sha256: payload_hash,
        content_type: None,
        security_token: if has_token {
            Some(token.to_owned())
        } else {
            None
        },
        extra_headers: Vec::new(),
    })
}

/// A presigned-URL endpoint: the base the signed path hangs under, the
/// host the signature covers, and the addressing style. Unlike
/// [`ResolvedEndpoint`] this is caller-assembled per export branch
/// (the MinIO presign endpoint is the custom-domain URL, not the upload
/// endpoint).
#[derive(Debug, Clone, PartialEq)]
pub struct PresignEndpoint {
    /// Base URL without trailing slash (`http://localhost:8123`,
    /// `https://uploads.s3.us-west-2.amazonaws.com`).
    pub url_base: String,
    /// `host` header value the signature covers (with port when present).
    pub signed_host: String,
    /// Path-style (`/{bucket}/{key}`) vs virtual-host (`/{key}`).
    pub path_style: bool,
}

/// Mint a SigV4 query-auth presigned GET URL (`generate_presigned_url(
/// "get_object", ExpiresIn=...)`, `export_task.py:63-67,87-91,108-112`):
/// canonical query (`X-Amz-Algorithm`, `X-Amz-Credential`,
/// `X-Amz-Date`, `X-Amz-Expires`, `X-Amz-SignedHeaders=host`), the
/// canonical request (whose payload hash is the empty-body SHA256 —
/// botocore's `SigV4QueryAuth` signs `e3b0c44...`, not
/// `UNSIGNED-PAYLOAD`), and the hex signature appended as
/// `X-Amz-Signature`. Verified byte-for-byte against botocore vectors
/// (see tests). For temporary (role) credentials use
/// [`presign_get_url_with_token`] or [`presign_get_url_with_creds`] so
/// `X-Amz-Security-Token` is carried on the URL.
#[allow(clippy::too_many_arguments)]
pub fn presign_get_url(
    endpoint: &PresignEndpoint,
    bucket: &str,
    object_key: &str,
    access_key: &str,
    secret_key: &str,
    region: &str,
    amz_datetime: &str,
    date_stamp: &str,
    expires_secs: u64,
) -> String {
    presign_get_url_with_token(
        endpoint,
        bucket,
        object_key,
        access_key,
        secret_key,
        "",
        region,
        amz_datetime,
        date_stamp,
        expires_secs,
    )
}

/// Mint a presigned GET URL carrying a session token (`X-Amz-Security-Token`)
/// for temporary credentials. The token is part of the canonical query and
/// therefore covered by the signature, mirroring botocore's query auth with
/// `aws_session_token`.
#[allow(clippy::too_many_arguments)]
pub fn presign_get_url_with_token(
    endpoint: &PresignEndpoint,
    bucket: &str,
    object_key: &str,
    access_key: &str,
    secret_key: &str,
    session_token: &str,
    region: &str,
    amz_datetime: &str,
    date_stamp: &str,
    expires_secs: u64,
) -> String {
    let scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let credential = format!("{access_key}/{scope}");
    let base_query = format!(
        "X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential={}&X-Amz-Date={amz_datetime}&X-Amz-Expires={expires_secs}",
        uri_encode(&credential),
    );
    // When a session token is present it sorts between Expires and
    // SignedHeaders (`X-Amz-Security-Token` < `X-Amz-SignedHeaders`).
    let query = if session_token.is_empty() {
        format!("{base_query}&X-Amz-SignedHeaders=host")
    } else {
        format!(
            "{base_query}&X-Amz-Security-Token={}&X-Amz-SignedHeaders=host",
            uri_encode(session_token),
        )
    };
    let path = if endpoint.path_style {
        format!("/{bucket}/{}", uri_encode_path(object_key))
    } else {
        format!("/{}", uri_encode_path(object_key))
    };
    let canonical = format!(
        "GET\n{path}\n{query}\nhost:{}\n\nhost\n{}",
        endpoint.signed_host,
        sha256_hex(&[]),
    );
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_datetime}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(secret_key, date_stamp, region),
        to_sign.as_bytes(),
    ));
    format!(
        "{}{path}?{query}&X-Amz-Signature={signature}",
        endpoint.url_base,
    )
}

/// Mint a presigned GET URL with credentials resolved per request
/// (role creds rotate; expiry is capped so the URL does not outlive them).
/// `requested_expires_secs` is capped against the credential expiry via
/// [`capped_expires_secs`]; pass `now` as the signing instant.
#[allow(clippy::too_many_arguments)]
pub fn presign_get_url_with_creds(
    endpoint: &PresignEndpoint,
    bucket: &str,
    object_key: &str,
    creds: &AwsCredentials,
    region: &str,
    amz_datetime: &str,
    date_stamp: &str,
    requested_expires_secs: u64,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let expires_secs = capped_presign_expires_secs(requested_expires_secs, creds, now);
    presign_get_url_with_token(
        endpoint,
        bucket,
        object_key,
        &creds.access_key,
        &creds.secret_key,
        &creds.session_token,
        region,
        amz_datetime,
        date_stamp,
        expires_secs,
    )
}

fn canonical_path(endpoint: &ResolvedEndpoint, bucket: &str, object_key: &str) -> String {
    if endpoint.path_style {
        format!("/{bucket}/{}", uri_encode_path(object_key))
    } else {
        format!("/{}", uri_encode_path(object_key))
    }
}

#[allow(dead_code)]
fn authorization(
    storage: &StorageSettings,
    canonical_request: &str,
    amz_datetime: &str,
    date_stamp: &str,
    signed_headers: &str,
) -> String {
    let creds = AwsCredentials {
        access_key: storage.access_key_id.clone(),
        secret_key: storage.secret_access_key.clone(),
        session_token: storage.session_token.clone(),
        expires_at: None,
    };
    authorization_with_creds(
        &creds,
        &storage.region,
        canonical_request,
        amz_datetime,
        date_stamp,
        signed_headers,
    )
}

fn authorization_with_creds(
    creds: &AwsCredentials,
    region: &str,
    canonical_request: &str,
    amz_datetime: &str,
    date_stamp: &str,
    signed_headers: &str,
) -> String {
    let scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_datetime}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex(&hmac_sha256(
        &signing_key(&creds.secret_key, date_stamp, region),
        string_to_sign.as_bytes(),
    ));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        creds.access_key,
    )
}

/// SHA-256 hex digest (also lets the caller hash provider bytes
/// independently of [`sign_put`]).
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// RFC 3986 percent-encoding for SigV4 (unreserved marks stay bare,
/// everything else `%XX` uppercase — botocore's `quote(..., safe='-_.~')`).
fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(
                    char::from_digit((b >> 4) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit((b & 0x0f) as u32, 16)
                        .expect("hex")
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

/// Path encoding for the canonical URI: slashes survive, every segment is
/// RFC 3986-encoded (botocore `quote(path, safe='/~')` with the same
/// unreserved set as [`uri_encode`]).
fn uri_encode_path(path: &str) -> String {
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// SigV4 signing key: `kDate/kRegion/kService/kSigning`
/// (`storage.py` always signs `s3`).
fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    hmac_sha256(&k_service, b"aws4_request")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_storage() -> StorageSettings {
        StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "AKIDEXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
            session_token: String::new(),
            bucket_name: "examplebucket".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some("https://s3.us-east-1.amazonaws.com".to_owned()),
            signed_url_expiration_secs: 3600,
        }
    }

    /// Independent oracle: recompute the signature by hand from the pinned
    /// canonical request, without touching [`sign_put`]/[`sign_delete`].
    fn oracle_signature(canonical: &str, amz_datetime: &str, date: &str) -> String {
        let scope = format!("{date}/us-east-1/s3/aws4_request");
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_datetime}\n{scope}\n{}",
            sha256_hex(canonical.as_bytes())
        );
        hex(&hmac_sha256(
            &signing_key(
                "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
                date,
                "us-east-1",
            ),
            to_sign.as_bytes(),
        ))
    }

    #[test]
    fn put_signing_vector() {
        // `sha256("hello")` is the well-known
        // 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824.
        let req = sign_put(
            &test_storage(),
            "https",
            "unused.example",
            "ab12cd34-user-avatar.png",
            "image/png",
            b"hello",
            "20260928T120000Z",
            "20260928",
        )
        .expect("custom endpoint signs");
        assert_eq!(req.method, "PUT");
        assert_eq!(
            req.url,
            "https://s3.us-east-1.amazonaws.com/examplebucket/ab12cd34-user-avatar.png"
        );
        assert_eq!(
            req.content_sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(req.content_type.as_deref(), Some("image/png"));
        assert_eq!(req.amz_date, "20260928T120000Z");
        // Golden canonical request (AWS header-auth shape, headers sorted —
        // botocore emits `content-type` before `host` for the same input).
        let canonical = "PUT\n/examplebucket/ab12cd34-user-avatar.png\n\ncontent-type:image/png\nhost:s3.us-east-1.amazonaws.com\nx-amz-content-sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824\nx-amz-date:20260928T120000Z\n\ncontent-type;host;x-amz-content-sha256;x-amz-date\n2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let expected = format!(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260928/us-east-1/s3/aws4_request, SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date, Signature={}",
            oracle_signature(canonical, "20260928T120000Z", "20260928")
        );
        assert_eq!(req.authorization, expected);
    }

    #[test]
    fn delete_signing_vector() {
        // `sha256("")` is the well-known
        // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.
        let req = sign_delete(
            &test_storage(),
            "https",
            "unused.example",
            "ab12cd34-user-avatar.png",
            "20260928T120000Z",
            "20260928",
        )
        .expect("custom endpoint signs");
        assert_eq!(req.method, "DELETE");
        assert_eq!(
            req.url,
            "https://s3.us-east-1.amazonaws.com/examplebucket/ab12cd34-user-avatar.png"
        );
        assert_eq!(
            req.content_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(req.content_type, None);
        let canonical = "DELETE\n/examplebucket/ab12cd34-user-avatar.png\n\nhost:s3.us-east-1.amazonaws.com\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20260928T120000Z\n\nhost;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let expected = format!(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260928/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",
            oracle_signature(canonical, "20260928T120000Z", "20260928")
        );
        assert_eq!(req.authorization, expected);
    }

    #[test]
    fn head_signing_vector() {
        // Same empty-payload hash as DELETE (`sha256("")`), same URL, only
        // the method line differs — `get_object_metadata` heads the key
        // the PUT just stored (`storage.py:156-174`).
        let req = sign_head(
            &test_storage(),
            "https",
            "unused.example",
            "ab12cd34-user-avatar.png",
            "20260928T120000Z",
            "20260928",
        )
        .expect("custom endpoint signs");
        assert_eq!(req.method, "HEAD");
        assert_eq!(
            req.url,
            "https://s3.us-east-1.amazonaws.com/examplebucket/ab12cd34-user-avatar.png"
        );
        assert_eq!(
            req.content_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(req.content_type, None);
        let canonical = "HEAD\n/examplebucket/ab12cd34-user-avatar.png\n\nhost:s3.us-east-1.amazonaws.com\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20260928T120000Z\n\nhost;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let expected = format!(
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20260928/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={}",
            oracle_signature(canonical, "20260928T120000Z", "20260928")
        );
        assert_eq!(req.authorization, expected);
    }

    #[test]
    fn endpoint_resolution_modes() {
        // MinIO mode signs `{scheme}://{Host}` path-style (D-02 parity).
        let minio = StorageSettings {
            use_minio: true,
            ..test_storage()
        };
        assert_eq!(
            resolve_server_endpoint(&minio, "http", "127.0.0.1:9000").expect("minio"),
            ResolvedEndpoint {
                url_base: "http://127.0.0.1:9000".to_owned(),
                signed_host: "127.0.0.1:9000".to_owned(),
                path_style: true,
            }
        );
        // `MINIO_ENDPOINT_SSL=1` forces https in MinIO mode (`storage.py:46-51`).
        let minio_ssl = StorageSettings {
            use_minio: true,
            minio_endpoint_ssl: true,
            ..test_storage()
        };
        assert_eq!(
            resolve_server_endpoint(&minio_ssl, "http", "127.0.0.1:9000").expect("minio"),
            ResolvedEndpoint {
                url_base: "https://127.0.0.1:9000".to_owned(),
                signed_host: "127.0.0.1:9000".to_owned(),
                path_style: true,
            }
        );
        // Explicit endpoint: trailing slash trimmed, host keeps its port.
        let custom = StorageSettings {
            use_minio: false,
            endpoint_url: Some("http://pi-dash-minio:9000/".to_owned()),
            ..test_storage()
        };
        let resolved = resolve_server_endpoint(&custom, "https", "public.example").expect("custom");
        assert_eq!(resolved.url_base, "http://pi-dash-minio:9000");
        assert_eq!(resolved.signed_host, "pi-dash-minio:9000");
        assert!(resolved.path_style);
        // No endpoint: virtual-hosted AWS default. us-east-1 resolves to
        // the global endpoint (botocore probe A/L), not a regional infix.
        let aws = StorageSettings {
            use_minio: false,
            endpoint_url: None,
            ..test_storage()
        };
        let resolved = resolve_server_endpoint(&aws, "https", "public.example").expect("aws");
        assert_eq!(resolved.url_base, "https://examplebucket.s3.amazonaws.com");
        assert_eq!(resolved.signed_host, "examplebucket.s3.amazonaws.com");
        assert!(!resolved.path_style);
        // Other regions stay virtual-hosted regional (probe J HEAD).
        let regional = StorageSettings {
            region: "eu-west-1".to_owned(),
            ..aws.clone()
        };
        let resolved =
            resolve_server_endpoint(&regional, "https", "public.example").expect("regional");
        assert_eq!(
            resolved.url_base,
            "https://examplebucket.s3.eu-west-1.amazonaws.com"
        );
        // Empty region with a derived endpoint fails (`ValueError`,
        // probe D); garbage regions fail everywhere
        // (`InvalidRegionError`). The caller maps both into its
        // provider-URL fallback branch, never a 500.
        let no_region = StorageSettings {
            region: String::new(),
            ..aws.clone()
        };
        assert!(resolve_server_endpoint(&no_region, "https", "public.example").is_err());
        let garbage = StorageSettings {
            region: "!!".to_owned(),
            ..aws
        };
        assert!(resolve_server_endpoint(&garbage, "https", "public.example").is_err());
        // Empty region against a custom endpoint still signs (probe F).
        let custom_empty = StorageSettings {
            region: String::new(),
            ..custom
        };
        let resolved =
            resolve_server_endpoint(&custom_empty, "https", "public.example").expect("custom");
        assert_eq!(resolved.url_base, "http://pi-dash-minio:9000");
    }

    #[test]
    fn object_key_path_is_segment_encoded() {
        let req = sign_delete(
            &test_storage(),
            "https",
            "unused.example",
            "a b/c+d.png",
            "20260928T120000Z",
            "20260928",
        )
        .expect("custom endpoint signs");
        assert!(
            req.url.ends_with("/examplebucket/a%20b/c%2Bd.png"),
            "{}",
            req.url
        );
    }

    #[test]
    fn storage_error_contract() {
        // Every variant renders a caller-usable message for the fallback
        // branch; service errors carry the status for logging.
        let err = StorageError::Transport("timeout after 10s".to_owned());
        assert_eq!(format!("{err}"), "s3 transport failure: timeout after 10s");
        let err = StorageError::Service {
            status: 403,
            message: "Forbidden".to_owned(),
        };
        assert_eq!(format!("{err}"), "s3 service error 403: Forbidden");
        assert_eq!(
            validate_storage(&test_storage()),
            Ok(()),
            "complete settings pre-flight clean"
        );
        assert!(
            matches!(
                validate_storage(&StorageSettings {
                    bucket_name: String::new(),
                    ..test_storage()
                }),
                Err(StorageError::Config(_))
            ),
            "empty bucket fails pre-flight, never a 500"
        );
        assert!(
            matches!(
                validate_storage(&StorageSettings {
                    secret_access_key: String::new(),
                    ..test_storage()
                }),
                Err(StorageError::Config(_))
            ),
            "empty secret fails pre-flight, never a 500"
        );
    }

    /// Byte-for-byte against the frozen botocore vectors: MinIO
    /// custom-domain presign (path-style) and region presign
    /// (virtual-host). Same inputs, same signature.
    #[test]
    fn presign_matches_botocore_vectors() {
        let key = "ws-1/export-an-abcdef-2026-09-28.zip";
        let minio = presign_get_url(
            &PresignEndpoint {
                url_base: "http://localhost:8123".to_owned(),
                signed_host: "localhost:8123".to_owned(),
                path_style: true,
            },
            "uploads",
            key,
            "ak",
            "sk",
            "us-east-1",
            "20261017T065234Z",
            "20261017",
            604800,
        );
        assert_eq!(
            minio,
            "http://localhost:8123/uploads/ws-1/export-an-abcdef-2026-09-28.zip\
             ?X-Amz-Algorithm=AWS4-HMAC-SHA256\
             &X-Amz-Credential=ak%2F20261017%2Fus-east-1%2Fs3%2Faws4_request\
             &X-Amz-Date=20261017T065234Z&X-Amz-Expires=604800\
             &X-Amz-SignedHeaders=host\
             &X-Amz-Signature=dd69daec777bff84091463170f51df2a430e3c43d0e9648f8019abb1b4f12e00"
        );
        let region = presign_get_url(
            &PresignEndpoint {
                url_base: "https://uploads.s3.us-west-2.amazonaws.com".to_owned(),
                signed_host: "uploads.s3.us-west-2.amazonaws.com".to_owned(),
                path_style: false,
            },
            "uploads",
            key,
            "ak",
            "sk",
            "us-west-2",
            "20261017T065234Z",
            "20261017",
            604800,
        );
        assert_eq!(
            region,
            "https://uploads.s3.us-west-2.amazonaws.com/ws-1/export-an-abcdef-2026-09-28.zip\
             ?X-Amz-Algorithm=AWS4-HMAC-SHA256\
             &X-Amz-Credential=ak%2F20261017%2Fus-west-2%2Fs3%2Faws4_request\
             &X-Amz-Date=20261017T065234Z&X-Amz-Expires=604800\
             &X-Amz-SignedHeaders=host\
             &X-Amz-Signature=3c92d5d720cc0697d7877b107caba731f39e31708e1274b7009e81649500db2b"
        );
    }

    /// The MinIO upload signs `x-amz-acl` alongside the PUT headers:
    /// same frozen vector as botocore's header auth.
    #[test]
    fn put_extra_signs_acl_like_botocore() {
        let storage = StorageSettings {
            use_minio: false,
            minio_endpoint_ssl: false,
            access_key_id: "ak".to_owned(),
            secret_access_key: "sk".to_owned(),
            session_token: String::new(),
            bucket_name: "uploads".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: Some("http://127.0.0.1:9000".to_owned()),
            signed_url_expiration_secs: 3600,
        };
        let req = sign_put_extra(
            &storage,
            "http",
            "unused.example",
            "ws-1/export-an-abcdef-2026-09-28.zip",
            "application/zip",
            b"PK-test-bytes",
            &[("x-amz-acl", "public-read")],
            "20261017T065234Z",
            "20261017",
        )
        .expect("signs");
        assert_eq!(
            req.url,
            "http://127.0.0.1:9000/uploads/ws-1/export-an-abcdef-2026-09-28.zip"
        );
        assert_eq!(
            req.authorization,
            "AWS4-HMAC-SHA256 Credential=ak/20261017/us-east-1/s3/aws4_request, \
             SignedHeaders=content-type;host;x-amz-acl;x-amz-content-sha256;x-amz-date, \
             Signature=46d8292fa9c02f8bd6f7a42be2cb625c5d777101cad5b717e6e06dc87460cbb7"
        );
        assert_eq!(
            req.extra_headers,
            vec![("x-amz-acl".to_owned(), "public-read".to_owned())]
        );
    }

    #[test]
    fn sign_put_with_session_token_includes_header() {
        let storage = test_storage();
        // Static temporary keys (e.g. STS): token is signed and returned.
        let creds = crate::aws_credentials::AwsCredentials {
            access_key: storage.access_key_id.clone(),
            secret_key: storage.secret_access_key.clone(),
            session_token: "tok-123".to_owned(),
            expires_at: None,
        };
        let req = sign_put_with_creds(
            &storage,
            &creds,
            "https",
            "unused.example",
            "ab12cd34-user-avatar.png",
            "image/png",
            b"hello",
            "20260928T120000Z",
            "20260928",
        )
        .expect("signs with token");
        assert_eq!(req.security_token.as_deref(), Some("tok-123"));
        assert!(
            req.authorization.contains("x-amz-security-token"),
            "{}",
            req.authorization
        );
        // Without token the header is absent and the vector is unchanged.
        let creds_no_token = crate::aws_credentials::AwsCredentials {
            access_key: storage.access_key_id.clone(),
            secret_key: storage.secret_access_key.clone(),
            session_token: String::new(),
            expires_at: None,
        };
        let req2 = sign_put_with_creds(
            &storage,
            &creds_no_token,
            "https",
            "unused.example",
            "ab12cd34-user-avatar.png",
            "image/png",
            b"hello",
            "20260928T120000Z",
            "20260928",
        )
        .expect("signs without token");
        assert_eq!(req2.security_token, None);
        assert!(!req2.authorization.contains("x-amz-security-token"));
    }

    #[test]
    fn presign_get_url_carries_session_token() {
        let url = presign_get_url_with_token(
            &PresignEndpoint {
                url_base: "https://uploads.s3.us-west-2.amazonaws.com".to_owned(),
                signed_host: "uploads.s3.us-west-2.amazonaws.com".to_owned(),
                path_style: false,
            },
            "uploads",
            "ws-1/export.zip",
            "ak",
            "sk",
            "tok-xyz",
            "us-west-2",
            "20261017T065234Z",
            "20261017",
            3600,
        );
        assert!(url.contains("X-Amz-Security-Token=tok-xyz"), "{url}");
        // Verify the token is part of the canonical query (signed).
        // The signature must differ from the no-token signature.
        let url_no_token = presign_get_url(
            &PresignEndpoint {
                url_base: "https://uploads.s3.us-west-2.amazonaws.com".to_owned(),
                signed_host: "uploads.s3.us-west-2.amazonaws.com".to_owned(),
                path_style: false,
            },
            "uploads",
            "ws-1/export.zip",
            "ak",
            "sk",
            "us-west-2",
            "20261017T065234Z",
            "20261017",
            3600,
        );
        assert_ne!(url, url_no_token);
    }

    #[test]
    fn validate_storage_rejects_placeholder_outside_minio() {
        let placeholder = StorageSettings {
            use_minio: false,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            ..test_storage()
        };
        assert!(validate_storage(&placeholder).is_err());
        let minio_placeholder = StorageSettings {
            use_minio: true,
            access_key_id: "access-key".to_owned(),
            secret_access_key: "secret-key".to_owned(),
            ..test_storage()
        };
        assert!(validate_storage(&minio_placeholder).is_ok());
        // Real keys pass even outside MinIO.
        let real = StorageSettings {
            use_minio: false,
            access_key_id: "AKIAREAL".to_owned(),
            secret_access_key: "secret".to_owned(),
            ..test_storage()
        };
        assert!(validate_storage(&real).is_ok());
    }

    #[tokio::test]
    async fn resolve_storage_credentials_falls_back_to_instance_role() {
        use crate::aws_credentials::tests::{imds_router, serve, FakeImds};
        use crate::aws_credentials::{CredentialSource, InstanceRoleProvider};
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        use std::sync::Arc;

        // Fake IMDS with a role; no process-env mutation. A per-test
        // provider keeps this test parallel-safe (the previous version
        // raced on `AWS_EC2_METADATA_SERVICE_ENDPOINT` + the shared
        // `OnceLock` IMDS cache).
        let state = FakeImds {
            fetches: Arc::new(AtomicUsize::new(0)),
            tokens: Arc::new(AtomicUsize::new(0)),
            lifetime_secs: 3600,
            with_role: true,
        };
        let base = serve(imds_router(state.clone())).await;
        let provider = Arc::new(InstanceRoleProvider::new(&base));
        let source = CredentialSource::InstanceRole(provider.clone());

        let storage = StorageSettings {
            use_minio: false,
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: String::new(),
            ..test_storage()
        };
        // The creds are fetched at most once and carry the token.
        let creds = resolve_storage_credentials_with_source(&storage, source.clone())
            .await
            .expect("role creds");
        assert!(creds.access_key.starts_with("ASIAFAKE"));
        assert!(!creds.session_token.is_empty());
        assert!(creds.expires_at.is_some());
        // Signing with them sends the token header.
        let signed = sign_put_with_creds(
            &storage,
            &creds,
            "https",
            "unused.example",
            "k",
            "text/plain",
            b"hi",
            "20260928T120000Z",
            "20260928",
        )
        .expect("signs");
        assert_eq!(signed.security_token, Some(creds.session_token.clone()));
        // Presigned URL carries the token and capped expiry.
        let now = chrono::Utc::now();
        let url = presign_get_url_with_creds(
            &PresignEndpoint {
                url_base: "https://b.s3.amazonaws.com".to_owned(),
                signed_host: "b.s3.amazonaws.com".to_owned(),
                path_style: false,
            },
            "b",
            "k",
            &creds,
            "us-east-1",
            "20260928T120000Z",
            "20260928",
            7 * 24 * 60 * 60,
            now,
        );
        assert!(url.contains("X-Amz-Security-Token="), "{url}");
        // Capped expiry: role creds live 3600s, so the 7-day request is capped.
        assert!(
            url.contains("X-Amz-Expires=3600") || url.contains("X-Amz-Expires=3599"),
            "{url}"
        );

        // Cache: second resolve does not refetch.
        let _ = resolve_storage_credentials_with_source(
            &storage,
            CredentialSource::InstanceRole(provider),
        )
        .await
        .expect("cached");
        assert_eq!(state.fetches.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_storage_settings_caps_expiry_for_role_creds() {
        use crate::aws_credentials::tests::{imds_router, serve, FakeImds};
        use crate::aws_credentials::{CredentialSource, InstanceRoleProvider};
        use std::sync::Arc;

        let state = FakeImds {
            fetches: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            tokens: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            lifetime_secs: 1800,
            with_role: true,
        };
        let base = serve(imds_router(state)).await;
        let provider = Arc::new(InstanceRoleProvider::new(&base));
        let source = CredentialSource::InstanceRole(provider);
        let storage = StorageSettings {
            use_minio: false,
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: String::new(),
            bucket_name: "b".to_owned(),
            region: "us-east-1".to_owned(),
            endpoint_url: None,
            signed_url_expiration_secs: 604800,
            ..test_storage()
        };
        let resolved = resolve_storage_settings_with_source(&storage, source)
            .await
            .expect("resolves");
        assert!(resolved.access_key_id.starts_with("ASIAFAKE"));
        assert!(!resolved.session_token.is_empty());
        assert!(
            resolved.signed_url_expiration_secs < 604800 && resolved.signed_url_expiration_secs > 0,
            "{}",
            resolved.signed_url_expiration_secs
        );
        assert!(
            resolved.signed_url_expiration_secs == 1800
                || (resolved.signed_url_expiration_secs >= 1798
                    && resolved.signed_url_expiration_secs <= 1800),
            "{}",
            resolved.signed_url_expiration_secs
        );
    }
}
