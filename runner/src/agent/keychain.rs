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
//! * macOS: generic-password search by service, loading attributes only
//!   (`kSecReturnAttributes`, never `kSecReturnData`). Attribute reads do
//!   not trigger an access prompt.
//! * Windows: `CredEnumerateW` with a target filter, ignoring the
//!   credential blob.
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

/// Unbounded platform probe. Searches generic passwords by service and
/// loads attributes only — no secret bytes cross into this process, so no
/// access prompt appears and nothing sensitive can leak into logs.
#[cfg(target_os = "macos")]
fn probe(service: &str, _attrs: &[(String, String)]) -> KeychainPresence {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    // Every `extern static` access and the copy call are unsafe; the query
    // itself only asks for attributes, never secret data.
    let class_key =
        unsafe { CFString::wrap_under_get_rule(security_framework_sys::item::kSecClass) };
    let class_value = unsafe {
        CFString::wrap_under_get_rule(security_framework_sys::item::kSecClassGenericPassword)
    };
    let service_key =
        unsafe { CFString::wrap_under_get_rule(security_framework_sys::item::kSecAttrService) };
    let return_key = unsafe {
        CFString::wrap_under_get_rule(security_framework_sys::item::kSecReturnAttributes)
    };
    let limit_key =
        unsafe { CFString::wrap_under_get_rule(security_framework_sys::item::kSecMatchLimit) };
    let limit_value =
        unsafe { CFString::wrap_under_get_rule(security_framework_sys::item::kSecMatchLimitAll) };
    let query = CFDictionary::from_CFType_pairs(&[
        (class_key.as_CFType(), class_value.as_CFType()),
        (service_key.as_CFType(), CFString::new(service).as_CFType()),
        (return_key.as_CFType(), CFBoolean::true_value().as_CFType()),
        (limit_key.as_CFType(), limit_value.as_CFType()),
    ]);
    let mut result: *const std::ffi::c_void = std::ptr::null();
    let status = unsafe {
        security_framework_sys::keychain_item::SecItemCopyMatching(
            query.as_concrete_TypeRef(),
            &mut result,
        )
    };
    if status == security_framework_sys::base::errSecSuccess {
        if result.is_null() {
            return KeychainPresence::Absent;
        }
        // `kSecMatchLimitAll` returns an array; take ownership (+1) so it
        // is released on drop, then count without touching secrets.
        let found: CFArray<core_foundation::base::CFType> =
            unsafe { CFArray::wrap_under_create_rule(result as CFArrayRef) };
        if found.is_empty() {
            KeychainPresence::Absent
        } else {
            KeychainPresence::Present { locked: false }
        }
    } else if status == security_framework_sys::base::errSecItemNotFound {
        KeychainPresence::Absent
    } else {
        // Locked keychain, denied interaction, no keychain: the item may
        // exist, so report inconclusive rather than absent.
        KeychainPresence::Unavailable
    }
}

/// Unbounded platform probe. Enumerates credentials whose target starts
/// with the service; the blob is never read.
#[cfg(target_os = "windows")]
fn probe(service: &str, _attrs: &[(String, String)]) -> KeychainPresence {
    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
    use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CredEnumerateW, CredFree};

    // `agy` stores under targets like `gemini:antigravity` and
    // `gemini/antigravity`; the prefix filter covers both spellings.
    let filter: Vec<u16> = format!("{service}*")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut count = 0u32;
    let mut creds: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: `filter` is NUL-terminated; `count`/`creds` are valid
    // out-pointers; the buffer is freed with `CredFree` on success.
    let ok = unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut creds) };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        if err == ERROR_NOT_FOUND {
            KeychainPresence::Absent
        } else {
            KeychainPresence::Unavailable
        }
    } else {
        unsafe { CredFree(creds as *mut std::ffi::c_void) };
        if count == 0 {
            KeychainPresence::Absent
        } else {
            KeychainPresence::Present { locked: false }
        }
    }
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
}
