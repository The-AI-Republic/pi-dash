//! Django `<uuid:>` path-converter compatibility.
//!
//! Django's `UUIDConverter` (`django/urls/converters.py`) matches only
//! lowercase hex, hyphenated, exactly-36-character UUIDs, case-sensitively.
//! Its regex runs at URL resolution, before middleware/authentication and
//! before the view, so every other spelling is a resolver 404.
//! `uuid::Uuid::parse_str` is far more permissive — it also accepts
//! uppercase, simple (unhyphenated), braced and `urn:uuid:` spellings — so a
//! handler that parses a `<uuid:...>` path segment directly will serve (and
//! write) spellings Django refuses to route.
//!
//! Handlers classify the raw segment with [`classify_uuid_segment`] before
//! auth: canonical segments are served locally, parseable-but-non-canonical
//! segments proxy to Django so its resolver 404 answers byte for byte, and
//! garbage keeps the local prod-correct resolver-404 body.
//!
//! Reference installations: `v1_cli_auth/runner.rs` (PIDASHCONV-1095),
//! `license/handlers_admin.rs` (PIDASHCONV-1132), `v1_assets`
//! (PIDASHCONV-1162).

use uuid::Uuid;

/// Whether a path segment is spelled the way Django's `UUIDConverter`
/// routes it: lowercase hex, hyphenated, exactly 36 characters.
pub fn is_canonical_uuid(raw: &str) -> bool {
    if raw.len() != 36 {
        return false;
    }
    raw.bytes().enumerate().all(|(index, byte)| {
        if index == 8 || index == 13 || index == 18 || index == 23 {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
        }
    })
}

/// How a `<uuid:...>` path segment classifies against Django's resolver.
pub enum UuidSegment {
    /// Routes in Django: serve here.
    Canonical(Uuid),
    /// Parses but is not the spelling Django routes (uppercase, simple,
    /// braced, `urn:uuid:`): proxy so Django's resolver 404 answers.
    NonCanonical,
    /// Not a UUID at all: the local prod-correct resolver-404 body.
    Garbage,
}

/// Classify a `<uuid:...>` segment (`v1_cli_auth` PIDASHCONV-1095 /
/// `license` PIDASHCONV-1132 precedent).
pub fn classify_uuid_segment(raw: &str) -> UuidSegment {
    match raw.parse::<Uuid>() {
        Ok(id) if is_canonical_uuid(raw) => UuidSegment::Canonical(id),
        Ok(_) => UuidSegment::NonCanonical,
        Err(_) => UuidSegment::Garbage,
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_uuid_segment, is_canonical_uuid, UuidSegment};

    const CANONICAL: &str = "12345678-1234-1234-1234-1234567890ab";

    #[test]
    fn canonical_uuid_accepts_lowercase_hyphenated_only() {
        assert!(is_canonical_uuid(CANONICAL));
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
            "not-a-uuid",
            "",
        ] {
            assert!(!is_canonical_uuid(raw), "{raw}");
        }
    }

    #[test]
    fn classify_separates_canonical_noncanonical_and_garbage() {
        assert!(matches!(
            classify_uuid_segment(CANONICAL),
            UuidSegment::Canonical(_)
        ));
        for raw in [
            "12345678-1234-1234-1234-1234567890AB",
            "123456781234123412341234567890ab",
            "{12345678-1234-1234-1234-1234567890ab}",
            "urn:uuid:12345678-1234-1234-1234-1234567890ab",
        ] {
            assert!(
                matches!(classify_uuid_segment(raw), UuidSegment::NonCanonical),
                "{raw}"
            );
        }
        for raw in ["not-a-uuid", "", "12345678-1234-1234-1234-1234567890a"] {
            assert!(
                matches!(classify_uuid_segment(raw), UuidSegment::Garbage),
                "{raw}"
            );
        }
    }
}
