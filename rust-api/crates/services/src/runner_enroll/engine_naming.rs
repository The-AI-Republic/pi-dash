#![forbid(unsafe_code)]

//! Engine runner naming (`runner/services/machine_identity.py`).
//!
//! Pure naming kernels: `slug`, machine slug, base name with the 128-char
//! sha256 truncation, and the unique-name suffix loop. The DB-backed
//! `unique_runner_name` / `live_engine_runner_for` live in `queries`
//! (they need the `taken names` seam); this module owns the string
//! rules byte-for-byte.

use regex::Regex;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

/// `RUNNER_NAME_MAX` (`machine_identity.py:48`).
pub const RUNNER_NAME_MAX: usize = 128;
/// `_HASH_LEN` (`:49`).
pub const HASH_LEN: usize = 8;
/// `ENGINE_NAME_PREFIX` (`:46`).
pub const ENGINE_NAME_PREFIX: &str = "builtin";

static SLUG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

/// `_slug` (`:64-65`): lowercase, every non-alphanumeric run to `-`, strip `-`.
pub fn slug(value: &str) -> String {
    SLUG_RE
        .replace_all(&value.to_lowercase(), "-")
        .trim_matches('-')
        .to_string()
}

/// `machine_slug` (`:68-70`): `slug(host_label)`, else `slug(label)`, else
/// the first segment of the machine UUID.
pub fn machine_slug(machine_id: uuid::Uuid, host_label: &str, label: &str) -> String {
    let from_host = slug(host_label);
    if !from_host.is_empty() {
        return from_host;
    }
    let from_label = slug(label);
    if !from_label.is_empty() {
        return from_label;
    }
    machine_id
        .to_string()
        .split('-')
        .next()
        .unwrap_or("")
        .to_string()
}

fn hex_of(hash: &[u8]) -> String {
    let mut out = String::with_capacity(hash.len() * 2);
    for byte in hash {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `engine_runner_base_name` (`:73-83`): join non-empty `builtin`, machine
/// slug, workspace slug, project identifier; over 128 chars, cut to 119,
/// rstrip `-`, plus `-` and the first 8 hex of sha256(project.id).
pub fn engine_runner_base_name(
    machine_id: uuid::Uuid,
    host_label: &str,
    label: &str,
    workspace_slug: &str,
    project_id: uuid::Uuid,
    project_identifier: &str,
) -> String {
    let parts = [
        ENGINE_NAME_PREFIX.to_string(),
        machine_slug(machine_id, host_label, label),
        slug(workspace_slug),
        slug(project_identifier),
    ];
    let base = parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("-");
    if base.len() <= RUNNER_NAME_MAX {
        return base;
    }
    let mut hasher = Sha256::new();
    hasher.update(project_id.to_string().as_bytes());
    let digest = hex_of(&hasher.finalize());
    let code = &digest[..HASH_LEN.min(digest.len())];
    let cut = base[..RUNNER_NAME_MAX - HASH_LEN - 1].trim_end_matches('-');
    format!("{cut}-{code}")
}

/// `_with_suffix` (`:86-88`): cut base to 128 minus suffix len, rstrip `-`,
/// append `-n`.
pub fn with_suffix(base: &str, n: u32) -> String {
    let suffix = format!("-{n}");
    let cut = if base.len() > RUNNER_NAME_MAX - suffix.len() {
        &base[..RUNNER_NAME_MAX - suffix.len()]
    } else {
        base
    };
    format!("{}{}", cut.trim_end_matches('-'), suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_python() {
        assert_eq!(slug("Rich's MBP"), "rich-s-mbp");
        assert_eq!(slug("Rich's Laptop"), "rich-s-laptop");
        assert_eq!(slug("  --Hello__World--  "), "hello-world");
        assert_eq!(slug(""), "");
        assert_eq!(slug("ABC123"), "abc123");
    }

    #[test]
    fn machine_slug_fallbacks() {
        let id = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        assert_eq!(machine_slug(id, "MyHost", ""), "myhost");
        assert_eq!(machine_slug(id, "", "My Label"), "my-label");
        assert_eq!(machine_slug(id, "", ""), "12345678");
    }

    #[test]
    fn base_name_joins_parts() {
        let mid = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let pid = uuid::Uuid::parse_str("87654321-1234-1234-1234-123456789abc").unwrap();
        assert_eq!(
            engine_runner_base_name(mid, "rich-mbp", "", "test-workspace", pid, "DEF"),
            "builtin-rich-mbp-test-workspace-def"
        );
    }

    #[test]
    fn base_name_truncates_with_hash() {
        let mid = uuid::Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let pid = uuid::Uuid::parse_str("87654321-1234-1234-1234-123456789abc").unwrap();
        let host = "h".repeat(200);
        let name = engine_runner_base_name(mid, &host, "", "ws", pid, "DEF");
        assert_eq!(name.len(), RUNNER_NAME_MAX);
        assert!(name.starts_with("builtin-hhh"));
        let mut hasher = Sha256::new();
        hasher.update(pid.to_string().as_bytes());
        let digest = hex_of(&hasher.finalize());
        assert!(name.ends_with(&digest[..8]));
    }

    #[test]
    fn with_suffix_cuts_to_128() {
        let base = "a".repeat(128);
        let next = with_suffix(&base, 2);
        assert_eq!(next.len(), 128);
        assert!(next.ends_with("-2"));
        assert_eq!(with_suffix("short", 2), "short-2");
    }
}
