//! Pure avatar-flow mapping for OAuth avatar persistence.
//!
//! Port of the offline half of `download_and_upload_avatar`
//! (`apps/api/pi_dash/authentication/adapter/base.py:150-254`): the
//! Content-Type allowlist, the size guards, the object-key / attribute
//! shapes, and the `get_object_metadata` JSON shape (`storage.py:156-174`).
//! The network halves (provider download over `requests`, S3 PUT/DELETE)
//! belong to the caller — this module never touches the network, so every
//! function here is pure and golden-testable.
//!
//! Python failure contract, mirrored: any mapping miss (unknown content
//! type, over-limit size) returns `None`/`false` so the caller stores the
//! provider URL directly (`base.py:236-254,330-345`) — never an error.

use serde_json::Value;

/// `FileAsset.EntityTypeContext.USER_AVATAR` (`db/models/asset.py:39`).
pub const ENTITY_TYPE_USER_AVATAR: &str = "USER_AVATAR";

/// Map a provider `Content-Type` response header to an avatar file extension.
///
/// Mirrors `base.py:160-175`: a missing header defaults to `"image/jpeg"`
/// before the lookup, and the lookup is an exact match on the raw header
/// value — `"image/png; charset=binary"` misses (see the crate-level
/// BUG-content-type-exact-match note). Returns `None` exactly when Python
/// returns `None` (caller takes the URL-fallback branch).
pub fn avatar_extension(content_type: Option<&str>) -> Option<&'static str> {
    match content_type.unwrap_or("image/jpeg") {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Object key for the uploaded avatar (`base.py:196`):
/// `f"{uuid4hex}-user-avatar.{extension}"`.
pub fn avatar_object_key(id_hex: &str, extension: &str) -> String {
    format!("{id_hex}-user-avatar.{extension}")
}

/// `FileAsset.attributes["name"]` for the avatar row (`base.py:213`):
/// `f"{provider}-avatar.{extension}"`.
pub fn avatar_asset_name(provider: &str, extension: &str) -> String {
    format!("{provider}-avatar.{extension}")
}

/// `FileAsset.attributes` for the avatar row (`base.py:213`):
/// `{"name", "type", "size"}` in Python insertion order.
pub fn avatar_attributes(provider: &str, extension: &str, content_type: &str, size: i64) -> Value {
    serde_json::json!({
        "name": avatar_asset_name(provider, extension),
        "type": content_type,
        "size": size,
    })
}

/// `FileAsset.storage_metadata` from an S3 HEAD response (`storage.py:156`).
///
/// Key order and null-on-missing mirror the Python dict: `response.get(...)`
/// yields `None` for absent fields, and `LastModified` is already an ISO-8601
/// string (`.isoformat()`) by the time it reaches this mapper. `metadata`
/// carries the HEAD `Metadata` map (`response.get("Metadata", {})`); pass
/// `None` when the caller has none (avatar uploads set no user metadata, so
/// the avatar flow always records `{}` exactly like Python).
pub fn head_to_storage_metadata(
    content_type: Option<&str>,
    content_length: Option<i64>,
    last_modified_iso: Option<&str>,
    etag: Option<&str>,
    metadata: Option<Value>,
) -> Value {
    serde_json::json!({
        "ContentType": content_type,
        "ContentLength": content_length,
        "LastModified": last_modified_iso,
        "ETag": etag,
        "Metadata": metadata.unwrap_or(serde_json::json!({})),
    })
}

/// `Content-Length` pre-check (`base.py:155-158`): a missing header imposes
/// no limit; a present value over `max_bytes` rejects the download.
///
/// Mirrors Python truthiness exactly: the header is only consulted when
/// present (`if content_length and ...`). A header that fails to parse as an
/// integer must be rejected by the caller — Python's `int()` raises into the
/// URL-fallback branch.
pub fn content_length_allowed(content_length: Option<i64>, max_bytes: i64) -> bool {
    match content_length {
        None => true,
        Some(n) => n <= max_bytes,
    }
}

/// Streaming size guard (`base.py:177-183`): the running `total_size` plus
/// the next chunk must stay within `max_bytes` (`DATA_UPLOAD_MAX_MEMORY_SIZE`,
/// i.e. `FILE_SIZE_LIMIT`, default `5242880`).
pub fn chunk_fits(total_so_far: u64, chunk_len: u64, max_bytes: u64) -> bool {
    total_so_far.saturating_add(chunk_len) <= max_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_allowlist_matches_python() {
        assert_eq!(avatar_extension(Some("image/jpeg")), Some("jpg"));
        assert_eq!(avatar_extension(Some("image/jpg")), Some("jpg"));
        assert_eq!(avatar_extension(Some("image/png")), Some("png"));
        assert_eq!(avatar_extension(Some("image/gif")), Some("gif"));
        assert_eq!(avatar_extension(Some("image/webp")), Some("webp"));
        // Missing header defaults to image/jpeg (`base.py:165`).
        assert_eq!(avatar_extension(None), Some("jpg"));
        // Unknown types fall back to the provider URL.
        assert_eq!(avatar_extension(Some("image/svg+xml")), None);
        assert_eq!(avatar_extension(Some("application/octet-stream")), None);
        assert_eq!(avatar_extension(Some("IMAGE/PNG")), None);
        // Exact-match parity: parameters miss, like Python's dict `.get`.
        assert_eq!(avatar_extension(Some("image/png; charset=binary")), None);
    }

    #[test]
    fn object_key_and_name_golden() {
        assert_eq!(
            avatar_object_key("ab12cd34ef56", "png"),
            "ab12cd34ef56-user-avatar.png"
        );
        assert_eq!(avatar_asset_name("google", "png"), "google-avatar.png");
        assert_eq!(
            ENTITY_TYPE_USER_AVATAR, "USER_AVATAR",
            "db/models/asset.py:39"
        );
    }

    #[test]
    fn attributes_json_golden() {
        // Golden: exact bytes Python's `attributes={...}` row carries
        // (`base.py:212-214`), key order included.
        let attrs = avatar_attributes("google", "png", "image/png", 12345);
        assert_eq!(
            serde_json::to_string(&attrs).expect("attrs serialize"),
            r#"{"name":"google-avatar.png","type":"image/png","size":12345}"#
        );
    }

    #[test]
    fn metadata_json_golden() {
        // Golden: `get_object_metadata` shape (`storage.py:165-171`).
        let meta = head_to_storage_metadata(
            Some("image/png"),
            Some(12345),
            Some("2026-09-30T06:00:00+00:00"),
            Some("\"abc123\""),
            None,
        );
        assert_eq!(
            serde_json::to_string(&meta).expect("meta serialize"),
            r#"{"ContentType":"image/png","ContentLength":12345,"LastModified":"2026-09-30T06:00:00+00:00","ETag":"\"abc123\"","Metadata":{}}"#
        );
        // Missing HEAD fields are null, like `response.get(...)` → None.
        let empty = head_to_storage_metadata(None, None, None, None, None);
        assert_eq!(
            serde_json::to_string(&empty).expect("empty serialize"),
            r#"{"ContentType":null,"ContentLength":null,"LastModified":null,"ETag":null,"Metadata":{}}"#
        );
        // A present Metadata map is recorded verbatim (`response.get`).
        let with_meta = head_to_storage_metadata(
            None,
            None,
            None,
            None,
            Some(serde_json::json!({"avatar": "true"})),
        );
        assert_eq!(
            serde_json::to_string(&with_meta).expect("meta serialize"),
            r#"{"ContentType":null,"ContentLength":null,"LastModified":null,"ETag":null,"Metadata":{"avatar":"true"}}"#
        );
    }

    #[test]
    fn size_guards_match_python() {
        // No header: no pre-check (`base.py:157`).
        assert!(content_length_allowed(None, 5242880));
        assert!(content_length_allowed(Some(5242880), 5242880));
        assert!(!content_length_allowed(Some(5242881), 5242880));
        // Streaming: `total_size > max` rejects (`base.py:180-182`).
        assert!(chunk_fits(0, 8192, 5242880));
        assert!(chunk_fits(5242879, 1, 5242880));
        assert!(!chunk_fits(5242880, 1, 5242880));
        assert!(!chunk_fits(u64::MAX, 1, 5242880));
    }
}
