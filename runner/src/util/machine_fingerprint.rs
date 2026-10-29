// Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Hardware/OS fingerprint for this computer.
//!
//! The cloud recognises a dev machine by `dev_machine_id`, a random UUID kept
//! in `config.toml`. Anything that loses that file (`pidash remove`, a
//! reinstall, a second config dir) used to mint a second record for the same
//! computer. The fingerprint is a stable, privacy-preserving identifier the
//! cloud uses to fold those records back into one: lowercase hex SHA-256 of
//! `"pidash-machine-v1\0" + raw_id`, where `raw_id` is the OS machine id
//! (trimmed, lowercased). The raw id never leaves the machine.
//!
//! Sources:
//! - macOS: `IOPlatformUUID` from `ioreg -rd1 -c IOPlatformExpertDevice`.
//! - Linux: the first non-empty of `/etc/machine-id`, `/var/lib/dbus/machine-id`.
//! - Windows: `HKLM\SOFTWARE\Microsoft\Cryptography` value `MachineGuid`.
//!
//! When none is available (some containers) the fingerprint is `None` and
//! callers omit the field entirely.

use std::sync::OnceLock;

const DOMAIN_PREFIX: &[u8] = b"pidash-machine-v1\0";

/// This computer's fingerprint, computed once per process.
pub fn machine_fingerprint() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| read_raw_machine_id().and_then(|raw| fingerprint_from_raw(&raw)))
        .clone()
}

/// Hash a raw OS machine id into the wire fingerprint. `None` for an empty
/// (or whitespace-only) id, so a blank source never collapses every machine
/// onto the same hash.
pub fn fingerprint_from_raw(raw_id: &str) -> Option<String> {
    let normalized = raw_id.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let mut input = Vec::with_capacity(DOMAIN_PREFIX.len() + normalized.len());
    input.extend_from_slice(DOMAIN_PREFIX);
    input.extend_from_slice(normalized.as_bytes());
    let digest = ring::digest::digest(&ring::digest::SHA256, &input);
    Some(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

/// Extract `IOPlatformUUID` from `ioreg -rd1 -c IOPlatformExpertDevice` output.
/// The relevant line looks like `"IOPlatformUUID" = "E13BB741-…"`.
pub fn parse_ioreg_platform_uuid(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if key.trim().trim_matches('"') != "IOPlatformUUID" {
            return None;
        }
        let value = value.trim().trim_matches('"').trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// Extract `MachineGuid` from `reg query … /v MachineGuid` output, whose
/// value line looks like `    MachineGuid    REG_SZ    3f2c…`.
pub fn parse_reg_machine_guid(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if !parts.next()?.eq_ignore_ascii_case("MachineGuid") {
            return None;
        }
        let _kind = parts.next()?;
        let value = parts.next()?.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// First non-empty machine id among `candidates` (Linux lookup order).
pub fn first_non_empty(candidates: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
fn read_raw_machine_id() -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_ioreg_platform_uuid(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(target_os = "linux")]
fn read_raw_machine_id() -> Option<String> {
    first_non_empty(
        ["/etc/machine-id", "/var/lib/dbus/machine-id"]
            .into_iter()
            .map(|p| std::fs::read_to_string(p).ok()),
    )
}

#[cfg(windows)]
fn read_raw_machine_id() -> Option<String> {
    // `/reg:64` reads the native view even from a 32-bit process, where
    // WOW64 redirection would otherwise hide the key.
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
            "/reg:64",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reg_machine_guid(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn read_raw_machine_id() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_domain_separated_sha256_hex() {
        // sha256("pidash-machine-v1\0abc")
        let fp = fingerprint_from_raw("abc").unwrap();
        assert_eq!(fp.len(), 64);
        assert!(
            fp.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        let expected = ring::digest::digest(&ring::digest::SHA256, b"pidash-machine-v1\0abc");
        let expected: String = expected
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(fp, expected);
        // Not the bare hash of the id: the prefix is part of the input.
        let bare = ring::digest::digest(&ring::digest::SHA256, b"abc");
        let bare: String = bare.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_ne!(fp, bare);
    }

    #[test]
    fn known_vector() {
        // Pinned so the server/desktop implementations can check against it.
        assert_eq!(
            fingerprint_from_raw("e13bb741-adad-5ac5-a910-590b3c7ddb34").unwrap(),
            "1079429538dca4891278de44ad4743539491f9d7b93a750dee9a274a7f7f1459"
        );
    }

    #[test]
    fn raw_id_is_trimmed_and_lowercased() {
        let a = fingerprint_from_raw("E13BB741-ADAD-5AC5-A910-590B3C7DDB34\n").unwrap();
        let b = fingerprint_from_raw("  e13bb741-adad-5ac5-a910-590b3c7ddb34").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn empty_raw_id_has_no_fingerprint() {
        assert_eq!(fingerprint_from_raw(""), None);
        assert_eq!(fingerprint_from_raw(" \n\t"), None);
    }

    #[test]
    fn parses_ioreg_output() {
        let out = r#"+-o J316sAP  <class IOPlatformExpertDevice, id 0x100000255, registered>
    {
      "IOPolledInterface" = "AppleARMWatchdogTimerHibernateHandler is not serializable"
      "IOPlatformSerialNumber" = "XXXXXXXXXX"
      "IOPlatformUUID" = "E13BB741-ADAD-5AC5-A910-590B3C7DDB34"
      "IOPlatformUUIDSuffix" = "nope"
    }
"#;
        assert_eq!(
            parse_ioreg_platform_uuid(out).as_deref(),
            Some("E13BB741-ADAD-5AC5-A910-590B3C7DDB34")
        );
    }

    #[test]
    fn ioreg_output_without_uuid_or_with_empty_uuid_is_none() {
        assert_eq!(parse_ioreg_platform_uuid(""), None);
        assert_eq!(
            parse_ioreg_platform_uuid(r#"  "IOPlatformSerialNumber" = "X""#),
            None
        );
        assert_eq!(
            parse_ioreg_platform_uuid(r#"  "IOPlatformUUID" = """#),
            None
        );
    }

    #[test]
    fn parses_reg_query_output() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Cryptography\r\n    MachineGuid    REG_SZ    3f2c9a1e-1111-2222-3333-444455556666\r\n\r\n";
        assert_eq!(
            parse_reg_machine_guid(out).as_deref(),
            Some("3f2c9a1e-1111-2222-3333-444455556666")
        );
        assert_eq!(parse_reg_machine_guid("ERROR: not found"), None);
    }

    #[test]
    fn linux_order_takes_first_non_empty() {
        assert_eq!(
            first_non_empty([Some("\n".into()), Some("abc\n".into())]).as_deref(),
            Some("abc")
        );
        assert_eq!(
            first_non_empty([None, Some("def".into())]).as_deref(),
            Some("def")
        );
        assert_eq!(first_non_empty([None, Some("  ".into())]), None);
    }

    #[test]
    fn this_machine_fingerprint_is_well_formed_when_present() {
        if let Some(fp) = machine_fingerprint() {
            assert_eq!(fp.len(), 64);
            assert!(fp.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        }
    }
}
