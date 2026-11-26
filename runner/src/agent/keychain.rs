//! OS-keychain presence probe: does the keychain hold an item, without
//! ever reading its secret?
//!
//! The runner's credential gate ([`crate::agent::auth`]) must accept runs
//! from users whose sign-in lives in the OS keychain (Antigravity's `agy`
//! keeps its Google sign-in there). The probe below checks existence only:
//! it never returns secrets, never unlocks anything, and never triggers an
//! "allow access" prompt.
//!
//! Per platform:
//!
//! * Linux: Secret Service `SearchItems` over D-Bus, using both the
//!   unlocked and the locked result lists. `SearchItems` returns object
//!   paths only — no secrets — and does not need the keyring unlocked.
//! * macOS: `security find-generic-password -s <service>` as a
//!   subprocess. Stdout is discarded so secret bytes never enter this
//!   process — only the exit status is read (0 present, 44 absent). A
//!   service-only lookup does not trigger an access prompt.
//! * Windows: `cmdkey /list` as a subprocess, parsing target names only.
//!   The listing never prints credential blobs, so there is no secret to
//!   redact.
//!
//! Items are matched on the service alone: callers pass the service name
//! plus optional extra attributes. On Linux the `service` argument always
//! sets the `service` search attribute (the go-keyring convention `agy`
//! stores under); extra `attrs` narrow the search further. macOS and
//! Windows match on the service only, so per-account variants (for
//! example `antigravity` vs `antigravity-acp`) all count.
//!
//! Like the keychain probe in [`crate::config::secrets`], the search runs
//! on its own thread bounded by the same timeout: a stuck Secret Service
//! must not hang a CLI command or the daemon. Anything inconclusive — no
//! D-Bus session, keychain locked in a way the platform API reports as an
//! error, timeout — maps to [`KeychainPresence::Unavailable`], and callers
//! treat that as "let the run through", the same as OpenClaw today.

use crate::config::secrets::KEYRING_TIMEOUT;

/// What the keychain probe found. `Present` means at least one item
/// matched the service; `locked` is true when the item was only visible in
/// the locked set (Linux Secret Service reports locked and unlocked matches
/// separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainPresence {
    Present { locked: bool },
    Absent,
    Unavailable,
}

/// Pure mapping of a Secret Service `SearchItems` result onto
/// [`KeychainPresence`]: any unlocked match counts as unlocked, a match only
/// in the locked list counts as locked, no match at all is absent.
///
/// Only the Linux probe calls this; the `cfg` keeps it (and its tests)
/// without tripping `-D warnings` dead-code errors on other platforms.
#[cfg(any(test, target_os = "linux"))]
fn classify_search(unlocked: usize, locked: usize) -> KeychainPresence {
    if unlocked > 0 {
        KeychainPresence::Present { locked: false }
    } else if locked > 0 {
        KeychainPresence::Present { locked: true }
    } else {
        KeychainPresence::Absent
    }
}

/// Probe the OS keychain for an item with the given service.
///
/// Runs [`probe`] on its own thread and gives up after
/// [`KEYRING_TIMEOUT`], returning [`KeychainPresence::Unavailable`] on
/// timeout. Never reads, logs, or sends any secret.
pub fn keychain_item_present(service: &str, attrs: &[(&str, &str)]) -> KeychainPresence {
    let service = service.to_owned();
    let attrs: Vec<(String, String)> = attrs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("keychain-probe".to_string())
        .spawn(move || {
            let _ = tx.send(probe(&service, &attrs));
        });
    if spawned.is_err() {
        return KeychainPresence::Unavailable;
    }
    match rx.recv_timeout(KEYRING_TIMEOUT) {
        Ok(presence) => presence,
        Err(_) => KeychainPresence::Unavailable,
    }
}

/// Unbounded platform probe. Every error maps to
/// [`KeychainPresence::Unavailable`]: callers must never block a run on a
/// guess.
#[cfg(target_os = "linux")]
fn probe(service: &str, attrs: &[(String, String)]) -> KeychainPresence {
    use std::collections::HashMap;
    use zbus::blocking::{Connection, Proxy};

    let outcome: Result<KeychainPresence, zbus::Error> = (|| {
        let conn = Connection::session()?;
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.secrets",
            "/org/freedesktop/secrets",
            "org.freedesktop.Secret.Service",
        )?;
        // The service argument always sets the `service` attribute, so a
        // bare-service call can never match another application's item.
        let mut query: HashMap<&str, &str> = attrs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        query.insert("service", service);
        let (unlocked, locked): (
            Vec<zbus::zvariant::OwnedObjectPath>,
            Vec<zbus::zvariant::OwnedObjectPath>,
        ) = proxy.call("SearchItems", &(query,))?;
        Ok(classify_search(unlocked.len(), locked.len()))
    })();
    outcome.unwrap_or(KeychainPresence::Unavailable)
}

/// Unbounded platform probe. Runs `security find-generic-password` as a
/// subprocess with stdout discarded, so secret bytes never enter this
/// process — only the exit status is read. No `unsafe`, no FFI: the crate
/// forbids unsafe code outright. A service-only lookup does not trigger
/// an "allow access" prompt; `Command` passes the service as an argv
/// entry, never through a shell, so service names cannot inject.
///
/// Exit statuses are the low byte of the OSStatus: 0 is success, 44 is
/// `errSecItemNotFound` (-25300). Anything else — locked keychain,
/// denied interaction, missing helper — is inconclusive, so it maps to
/// [`KeychainPresence::Unavailable`] and callers let the run through
/// rather than block on a guess.
#[cfg(target_os = "macos")]
fn probe(service: &str, _attrs: &[(String, String)]) -> KeychainPresence {
    use std::process::{Command, Stdio};

    let code = Command::new("security")
        .args(["find-generic-password", "-s", service])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .and_then(|status| status.code());
    match code {
        Some(0) => KeychainPresence::Present { locked: false },
        Some(44) => KeychainPresence::Absent,
        _ => KeychainPresence::Unavailable,
    }
}

/// Unbounded platform probe. Runs `cmdkey /list` as a subprocess and
/// parses target names only. The listing never prints credential blobs,
/// so capturing it cannot leak a secret. No `unsafe`, no FFI: the crate
/// forbids unsafe code outright.
#[cfg(target_os = "windows")]
fn probe(service: &str, _attrs: &[(String, String)]) -> KeychainPresence {
    use std::process::{Command, Stdio};

    let text = Command::new("cmdkey")
        .arg("/list")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    match text {
        Some(text) if cmdkey_output_has_service(&text, service) => {
            KeychainPresence::Present { locked: false }
        }
        Some(_) => KeychainPresence::Absent,
        None => KeychainPresence::Unavailable,
    }
}

/// True when `cmdkey /list` output names a credential for `service`.
///
/// `agy` stores under targets like `gemini:antigravity` and
/// `gemini/antigravity`, displayed as `LegacyGeneric:target=…`; the
/// match is the value after `target=` (that marker is part of the stored
/// name, so it survives non-English output), falling back to the value
/// after the line's label (`Target: …`) so bare target forms match too.
/// The match is the service alone (`gemini`), an exact hit, or the
/// service followed by `:` or `/` — so per-account variants all count
/// while `gemini2`-style neighbours do not. Case-insensitive: target
/// names compare that way on Windows.
#[cfg(any(test, target_os = "windows"))]
fn cmdkey_output_has_service(output: &str, service: &str) -> bool {
    let service = service.to_ascii_lowercase();
    let exact = service.as_str();
    let colon = format!("{service}:");
    let slash = format!("{service}/");
    output.lines().any(|line| {
        let hay = line.to_ascii_lowercase();
        let mut targets: Vec<&str> = hay
            .split("target=")
            .skip(1)
            .map(|rest| rest.split_whitespace().next().unwrap_or(""))
            .collect();
        if targets.is_empty() {
            let after_label = hay
                .split_once(':')
                .map(|(_, value)| value.trim())
                .unwrap_or(hay.trim());
            targets.push(after_label);
        }
        targets.iter().any(|target| {
            *target == exact || target.starts_with(&colon) || target.starts_with(&slash)
        })
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn probe(_service: &str, _attrs: &[(String, String)]) -> KeychainPresence {
    KeychainPresence::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocked_match_is_present_unlocked() {
        assert_eq!(
            classify_search(1, 0),
            KeychainPresence::Present { locked: false }
        );
        assert_eq!(
            classify_search(2, 3),
            KeychainPresence::Present { locked: false }
        );
    }

    #[test]
    fn locked_only_match_is_present_locked() {
        assert_eq!(
            classify_search(0, 1),
            KeychainPresence::Present { locked: true }
        );
    }

    #[test]
    fn no_match_is_absent() {
        assert_eq!(classify_search(0, 0), KeychainPresence::Absent);
    }

    /// Touches the real OS keychain (search only: no reads, no writes, no
    /// unlock) and must stay ignored in normal runs.
    #[test]
    #[ignore = "needs a real answering keychain; run explicitly with -- --ignored"]
    #[cfg(target_os = "linux")]
    fn live_keychain_search_reports_canary_absent() {
        let canary = "pidash-antigravity-canary-8d2f";
        assert_eq!(
            keychain_item_present(canary, &[("service", canary)]),
            KeychainPresence::Absent
        );
    }

    /// Same canary search via the `security` CLI: exit 44 means absent.
    /// Needs a logged-in Mac; stays ignored in normal runs.
    #[test]
    #[ignore = "needs a real answering keychain; run explicitly with -- --ignored"]
    #[cfg(target_os = "macos")]
    fn live_security_search_reports_canary_absent() {
        let canary = "pidash-antigravity-canary-8d2f";
        assert_eq!(keychain_item_present(canary, &[]), KeychainPresence::Absent);
    }

    /// Same canary search via `cmdkey /list`. Needs a Windows host;
    /// stays ignored in normal runs.
    #[test]
    #[ignore = "needs a real answering keychain; run explicitly with -- --ignored"]
    #[cfg(target_os = "windows")]
    fn live_cmdkey_search_reports_canary_absent() {
        let canary = "pidash-antigravity-canary-8d2f";
        assert_eq!(keychain_item_present(canary, &[]), KeychainPresence::Absent);
    }

    const CMDKEY_COLON: &str = "\
Currently stored credentials:

    Target: LegacyGeneric:target=gemini:antigravity
    Type: Generic
    User: someone@example.com
";

    const CMDKEY_SLASH: &str = "\
Currently stored credentials:

    Target: LegacyGeneric:target=gemini/antigravity
    Type: Generic
";

    #[test]
    fn cmdkey_colon_target_matches() {
        assert!(cmdkey_output_has_service(CMDKEY_COLON, "gemini"));
    }

    #[test]
    fn cmdkey_slash_target_matches() {
        assert!(cmdkey_output_has_service(CMDKEY_SLASH, "gemini"));
    }

    #[test]
    fn cmdkey_bare_target_matches() {
        let output = "    Target: gemini:antigravity\n    Type: Generic\n";
        assert!(cmdkey_output_has_service(output, "gemini"));
        let output = "    Target: gemini\n";
        assert!(cmdkey_output_has_service(output, "gemini"));
    }

    #[test]
    fn cmdkey_match_is_case_insensitive() {
        let output = "    Target: LegacyGeneric:target=GEMINI:Antigravity\n";
        assert!(cmdkey_output_has_service(output, "gemini"));
        assert!(cmdkey_output_has_service(CMDKEY_COLON, "GEMINI"));
    }

    #[test]
    fn cmdkey_other_services_do_not_match() {
        let output = "    Target: LegacyGeneric:target=gemini2:antigravity\n";
        assert!(!cmdkey_output_has_service(output, "gemini"));
        let output = "    Target: LegacyGeneric:target=mygemini:antigravity\n";
        assert!(!cmdkey_output_has_service(output, "gemini"));
        let output = "    Target: LegacyGeneric:target=other:service\n";
        assert!(!cmdkey_output_has_service(output, "gemini"));
    }

    #[test]
    fn cmdkey_empty_store_is_absent() {
        assert!(!cmdkey_output_has_service("", "gemini"));
        assert!(!cmdkey_output_has_service(
            "Currently stored credentials:\n\n* NONE *\n",
            "gemini"
        ));
    }
}
