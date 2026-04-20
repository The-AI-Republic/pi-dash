//! `runner.name` rules: charset validation + default-name generator.
//!
//! Mirror of the cloud-side regex in `apps/api/pi_dash/runner/serializers.py`
//! (`RUNNER_NAME_CHARSET`). Charset is `[A-Za-z0-9_-]`, length 1..=128.
//!
//! Defaults follow the scheme `pidash_runner_<3 random chars from [A-Za-z0-9]>`.
//! The collision domain is per-workspace (enforced by the cloud DB's
//! `UNIQUE(workspace_id, name)` constraint), and 62³ ≈ 238k suffixes is ample
//! at that scope — `configure` retries up to 5 times on `runner_name_taken`
//! when the name was auto-generated, so a realistic collision rate is near
//! zero even at thousands of runners per workspace.
//!
//! See `.ai_design/runner_install_ux/cli-restructure-and-install-flow.md` §5.

use rand::Rng;

const MAX_LEN: usize = 128;

/// The 62-char alphabet used for the random suffix. No underscore or dash
/// because the suffix is cosmetic — keeping it purely alphanumeric avoids
/// awkward names like `pidash_runner_---`.
const SUFFIX_CHARSET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

const SUFFIX_LEN: usize = 3;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NameValidationError {
    #[error("runner name cannot be empty")]
    Empty,
    #[error("runner name is longer than {MAX_LEN} characters")]
    TooLong,
    #[error(
        "runner name may only contain letters, digits, underscore, and dash (no spaces or other characters)"
    )]
    BadChar,
}

/// Validate a candidate `runner.name`. Applied to user-supplied `--name`
/// input; auto-generated names are charset-safe by construction but are
/// still cheap to re-validate in tests.
pub fn validate(name: &str) -> Result<(), NameValidationError> {
    if name.is_empty() {
        return Err(NameValidationError::Empty);
    }
    if name.len() > MAX_LEN {
        return Err(NameValidationError::TooLong);
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(NameValidationError::BadChar);
    }
    Ok(())
}

/// Generate `pidash_runner_<3 random chars>`. Uses the thread-local RNG so
/// callers don't need to thread one through.
pub fn generate_default() -> String {
    let mut rng = rand::thread_rng();
    let suffix: String = (0..SUFFIX_LEN)
        .map(|_| SUFFIX_CHARSET[rng.gen_range(0..SUFFIX_CHARSET.len())] as char)
        .collect();
    format!("pidash_runner_{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names_pass() {
        for name in [
            "a",
            "laptop",
            "my-laptop",
            "my_laptop",
            "pidash_runner_aB3",
            "CAPS",
            "abc123",
            "a-b_c-1",
            &"a".repeat(128),
        ] {
            assert!(validate(name).is_ok(), "expected ok: {name:?}");
        }
    }

    #[test]
    fn empty_name_is_rejected() {
        assert_eq!(validate(""), Err(NameValidationError::Empty));
    }

    #[test]
    fn overlong_name_is_rejected() {
        let too_long = "a".repeat(129);
        assert_eq!(validate(&too_long), Err(NameValidationError::TooLong));
    }

    #[test]
    fn names_with_disallowed_chars_are_rejected() {
        for bad in [
            "has space",
            "dot.separated",
            "slash/separator",
            "emoji-💥",
            "semicolon;",
            "plus+sign",
            "at@host",
            "paren(s)",
        ] {
            assert_eq!(validate(bad), Err(NameValidationError::BadChar), "{bad:?}");
        }
    }

    #[test]
    fn generated_default_has_expected_shape() {
        for _ in 0..100 {
            let name = generate_default();
            assert!(
                name.starts_with("pidash_runner_"),
                "missing prefix: {name}"
            );
            assert_eq!(
                name.len(),
                "pidash_runner_".len() + SUFFIX_LEN,
                "unexpected length: {name}"
            );
            // Every generated default must pass the validator — otherwise the
            // cloud would reject our own output.
            assert!(validate(&name).is_ok(), "generated name failed validation: {name}");
            let suffix = &name["pidash_runner_".len()..];
            assert!(
                suffix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric()),
                "suffix must be alphanumeric only: {suffix}"
            );
        }
    }
}
