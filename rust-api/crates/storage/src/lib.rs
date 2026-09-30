//! Shared server-side S3 storage helpers (assets/storage domain, PIDASHCONV-480).
//!
//! Port of the S3 half of `apps/api/pi_dash/authentication/adapter/base.py`
//! (`download_and_upload_avatar:150-254`, `delete_old_avatar:285-287`) and
//! `apps/api/pi_dash/settings/storage.py` (`S3Storage`: `upload_file`,
//! `get_object_metadata`, `delete_files`), built on the D-02 offline-SigV4
//! precedent (`crates/api/src/space/assets.rs:1020-1078`, PIDASHCONV-178).
//!
//! This crate is the ONE shared home for SigV4 Authorization-header signing:
//! domain callers (notably the D-17 OAuth callback, PIDASHCONV-475) build a
//! [`s3::SignedRequest`] here and execute it with their own HTTP client
//! (`reqwest` already lives in `pidash-api`), instead of growing a parallel
//! signer. The crate itself never touches the network, so every constructor
//! below is pure and unit-testable.
//!
//! Python failure contract (translate, don't redesign): every S3 exception
//! path in `storage.py` is swallowed and the avatar caller falls back to the
//! provider URL — nothing 500s. Here every fallible outcome is a
//! [`s3::StorageError`] the caller maps to its fallback branch; builders that
//! cannot fail return values directly, and fallible pre-flight is explicit
//! via [`s3::validate_storage`].
//!
//! Deliberate deviations from botocore (documented, interop-safe):
//!
//! * `x-amz-content-sha256` carries the real payload hash. botocore's S3
//!   signer sends the literal `UNSIGNED-PAYLOAD`; S3 and MinIO accept either,
//!   and the real hash verifies on stores that reject unsigned payloads.
//! * The signed-header set is fixed and SigV4-sorted (`content-type`, `host`
//!   on PUT, `x-amz-content-sha256`, `x-amz-date`). The caller MUST send
//!   exactly the headers on [`s3::SignedRequest`] with exactly those values.
//!
//! Ported bugs (also listed in the PR):
//!
//! * BUG-content-type-exact-match (`adapter/base.py:165-175`): the extension
//!   allowlist is keyed on the raw `Content-Type` header value —
//!   `"image/png; charset=binary"` misses and falls back to the provider
//!   URL, exactly like Python's `extension_map.get(content_type)`.

#![forbid(unsafe_code)]

pub mod avatar;
pub mod s3;

pub use avatar::{
    avatar_asset_name, avatar_attributes, avatar_extension, avatar_object_key, chunk_fits,
    content_length_allowed, head_to_storage_metadata, ENTITY_TYPE_USER_AVATAR,
};
pub use s3::{
    resolve_server_endpoint, sha256_hex, sign_delete, sign_put, validate_storage, ResolvedEndpoint,
    SignedRequest, StorageError,
};
