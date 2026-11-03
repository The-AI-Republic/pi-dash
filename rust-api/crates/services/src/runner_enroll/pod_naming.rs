#![forbid(unsafe_code)]

//! Pod-name validation (`runner/services/pod_naming.py`, whole file `:18-84`).
//!
//! Default pods are `{identifier}_pod_<n>`; user pods are
//! `{identifier}_<custom_suffix>` with a server-enforced prefix, an ASCII
//! charset, length caps, and the `pod_<digits>` suffix range reserved for
//! auto-generation. Fixture: D13-F6 `pod_naming`
//! (`fixtures/runner_enroll/services/flows.golden.json`).

use std::sync::LazyLock;

/// Total name cap (`POD_NAME_MAX_LENGTH`): 128 *characters*.
pub const POD_NAME_MAX_LENGTH: usize = 128;
/// Suffix cap (`USER_SUFFIX_MAX_LENGTH`): 96 *characters*.
pub const USER_SUFFIX_MAX_LENGTH: usize = 96;

/// Reserved auto-default suffixes (`_RESERVED_USER_SUFFIX_RE`): `pod_<digits>`.
/// Python `\d` is exactly Unicode decimal (`Nd`), same as the `regex` crate's
/// — pinned by U+0663/U+00B2/U+FF11 tests. The trailing `\n?` replicates
/// Python `$`, which also matches just before ONE trailing newline
/// (`"WEB_pod_1\n"` is auto in Python); `\z` alone would diverge there.
static RESERVED_SUFFIX_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\Apod_\d+\n?\z").expect("reserved suffix regex compiles"));

/// User-suffix charset (`_USER_SUFFIX_CHARSET_RE`): ASCII `[A-Za-z0-9._-]`,
/// same trailing-newline note as above (`"WEB_abc\n"` is VALID in Python).
static CHARSET_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z0-9._-]+\n?\z").expect("charset regex compiles"));

/// `required_prefix` (`:33-35`): the mandatory `{identifier}_` prefix.
pub fn required_prefix(project_identifier: &str) -> String {
    format!("{project_identifier}_")
}

/// CPython `repr` for `str`, for the `{prefix!r}` interpolation (`:56`).
///
/// Single quotes unless the value holds a lone `'` (then double quotes);
/// backslash and `\n`/`\r`/`\t` take short escapes, other C0 controls and
/// DEL take lowercase `\xXX`, printable non-ASCII passes through. (Other
/// non-printable Unicode passing through unescaped is the documented
/// boundary, shared with the `assistant::mcp` precedent — unreachable from
/// real project identifiers.)
fn py_repr(value: &str) -> String {
    let use_double = value.contains('\'') && !value.contains('"');
    let (open, close) = if use_double { ('"', '"') } else { ('\'', '\'') };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(open);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\'' if !use_double => out.push_str("\\'"),
            '"' if use_double => out.push_str("\\\""),
            c if c < ' ' || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(close);
    out
}

/// `validate_user_pod_name` (`:38-75`): the error message, or `None` when
/// valid. Checks run in source order: required → prefix → total length →
/// empty suffix → suffix length → charset → reserved. Callers wrap the
/// message as 400 `{'error': msg}`.
///
/// Python also rejects non-`str` input with "name is required"; `&str` makes
/// that unrepresentable here, leaving only the empty check.
pub fn validate_user_pod_name(name: &str, project_identifier: &str) -> Option<String> {
    if name.is_empty() {
        return Some("name is required".to_owned());
    }
    let prefix = required_prefix(project_identifier);
    if !name.starts_with(&prefix) {
        return Some(format!("name must start with {}", py_repr(&prefix)));
    }
    // `len()` counts Unicode scalar values, not bytes.
    if name.chars().count() > POD_NAME_MAX_LENGTH {
        return Some(format!(
            "name must be at most {POD_NAME_MAX_LENGTH} characters"
        ));
    }
    // Byte slice: `starts_with` guarantees `prefix.len()` is a char boundary.
    let suffix = &name[prefix.len()..];
    if suffix.is_empty() {
        return Some("name suffix (after the project prefix) cannot be empty".to_owned());
    }
    if suffix.chars().count() > USER_SUFFIX_MAX_LENGTH {
        return Some(format!(
            "name suffix must be at most {USER_SUFFIX_MAX_LENGTH} characters"
        ));
    }
    if !CHARSET_RE.is_match(suffix) {
        return Some("name suffix may only contain letters, digits, '.', '_', '-'".to_owned());
    }
    if RESERVED_SUFFIX_RE.is_match(suffix) {
        return Some(
            "suffixes matching 'pod_<digits>' are reserved for auto-generated default pods"
                .to_owned(),
        );
    }
    None
}

/// `is_auto_default_name` (`:78-84`): true iff the name carries the reserved
/// `pod_<digits>` suffix under the project prefix.
pub fn is_auto_default_name(name: &str, project_identifier: &str) -> bool {
    let prefix = required_prefix(project_identifier);
    if !name.starts_with(&prefix) {
        return false;
    }
    RESERVED_SUFFIX_RE.is_match(&name[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// D13-F6 `pod_naming`, the Done-when fixture for this module.
    fn f6() -> Value {
        let path = format!(
            "{}/../../fixtures/runner_enroll/services/flows.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("F6 fixture exists"))
            .expect("F6 fixture parses")
    }

    fn naming() -> Value {
        f6()["pod_naming"].clone()
    }

    /// Expand the fixture's `<N a's>` / annotation shorthands to literal names.
    fn expand(shorthand: &str) -> String {
        if shorthand == "WEB_<97 a's>" {
            return format!("WEB_{}", "a".repeat(97));
        }
        if shorthand.starts_with("X_<126 a's>") {
            return format!("X_{}", "a".repeat(126));
        }
        if shorthand.starts_with("X_<127 a's>") {
            return format!("X_{}", "a".repeat(127));
        }
        if shorthand == "web_beefy (case-sensitive prefix)" {
            return "web_beefy".to_owned();
        }
        shorthand.to_owned()
    }

    #[test]
    fn consts_match_f6() {
        let fx = naming();
        assert_eq!(
            POD_NAME_MAX_LENGTH,
            fx["POD_NAME_MAX_LENGTH"].as_u64().unwrap() as usize
        );
        assert_eq!(
            USER_SUFFIX_MAX_LENGTH,
            fx["USER_SUFFIX_MAX_LENGTH"].as_u64().unwrap() as usize
        );
    }

    #[test]
    fn validate_cases_match_f6() {
        let fx = naming();
        let cases = fx["validate_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 12);
        for case in cases {
            let project = case["project"].as_str().unwrap();
            let name = expand(case["name"].as_str().unwrap());
            let expected = case["error"].as_str().map(str::to_owned);
            assert_eq!(validate_user_pod_name(&name, project), expected, "{name:?}");
        }
        // The expansions hit the documented boundary totals.
        assert_eq!(
            expand("X_<126 a's> (total 128: passes total check, fails suffix check)")
                .chars()
                .count(),
            128
        );
        assert_eq!(
            expand("X_<127 a's> (total 129: fails total check first)")
                .chars()
                .count(),
            129
        );
    }

    #[test]
    fn auto_cases_match_f6() {
        let fx = naming();
        let cases = fx["is_auto_default_name"].as_array().unwrap();
        assert_eq!(cases.len(), 6);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let project = case["project"].as_str().unwrap();
            assert_eq!(
                is_auto_default_name(name, project),
                case["result"].as_bool().unwrap(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn trailing_newline_matches_python() {
        // Probed against real `pod_naming.py`: Python `$` tolerates exactly
        // one trailing newline.
        assert_eq!(validate_user_pod_name("WEB_abc\n", "WEB"), None);
        assert_eq!(
            validate_user_pod_name("WEB_pod_1\n", "WEB").as_deref(),
            Some("suffixes matching 'pod_<digits>' are reserved for auto-generated default pods")
        );
        assert!(is_auto_default_name("WEB_pod_1\n", "WEB"));
        assert!(!is_auto_default_name("WEB_abc\n", "WEB"));
        for name in ["WEB_abc\n\n", "WEB_pod_1\n\n", "WEB_abc\r\n"] {
            assert_eq!(
                validate_user_pod_name(name, "WEB").as_deref(),
                Some("name suffix may only contain letters, digits, '.', '_', '-'"),
                "{name:?}"
            );
            assert!(!is_auto_default_name(name, "WEB"), "{name:?}");
        }
    }

    #[test]
    fn unicode_digits_match_python() {
        // `\d` is Unicode decimal (Nd) on both sides, but non-ASCII suffixes
        // fail the ASCII charset first — so in `validate` only the charset
        // error is reachable, while `is_auto_default_name` sees `\d` raw.
        let charset_err = "name suffix may only contain letters, digits, '.', '_', '-'";
        // U+0663 ARABIC-INDIC DIGIT THREE (Nd).
        assert_eq!(
            validate_user_pod_name("WEB_pod_\u{663}", "WEB").as_deref(),
            Some(charset_err)
        );
        assert!(is_auto_default_name("WEB_pod_\u{663}", "WEB"));
        // U+FF11 FULLWIDTH DIGIT ONE (Nd).
        assert!(is_auto_default_name("WEB_pod_\u{ff11}", "WEB"));
        // U+00B2 SUPERSCRIPT TWO (No — not decimal).
        assert_eq!(
            validate_user_pod_name("WEB_pod_\u{b2}", "WEB").as_deref(),
            Some(charset_err)
        );
        assert!(!is_auto_default_name("WEB_pod_\u{b2}", "WEB"));
    }

    #[test]
    fn lengths_count_chars_not_bytes() {
        // 128 multibyte chars: passes the total check, fails the suffix one —
        // byte counting would report the total error instead.
        let name = format!("X_{}", "\u{e9}".repeat(126));
        assert_eq!(name.chars().count(), 128);
        assert_eq!(
            validate_user_pod_name(&name, "X").as_deref(),
            Some("name suffix must be at most 96 characters")
        );
        assert_eq!(
            validate_user_pod_name("WEB_caf\u{e9}", "WEB").as_deref(),
            Some("name suffix may only contain letters, digits, '.', '_', '-'")
        );
    }

    #[test]
    fn prefix_message_matches_python_repr() {
        // Probed against real `pod_naming.py`: `{prefix!r}` is full CPython
        // repr (interior newlines/backslashes survive identifier validation).
        assert_eq!(
            validate_user_pod_name("x", "WE\nB").as_deref(),
            Some("name must start with 'WE\\nB_'")
        );
        assert_eq!(
            validate_user_pod_name("x", "WE'B").as_deref(),
            Some("name must start with \"WE'B_\"")
        );
        assert_eq!(py_repr("WE\\B_"), "'WE\\\\B_'");
        assert_eq!(py_repr("a\x07b"), "'a\\x07b'");
        assert_eq!(py_repr("a\x7fb"), "'a\\x7fb'");
        assert_eq!(py_repr("caf\u{e9}_"), "'caf\u{e9}_'");
        assert_eq!(py_repr("both\"and'quotes"), "'both\"and\\'quotes'");
    }

    #[test]
    fn empty_project_identifier() {
        assert_eq!(
            validate_user_pod_name("_", "").as_deref(),
            Some("name suffix (after the project prefix) cannot be empty")
        );
        assert_eq!(
            validate_user_pod_name("", "").as_deref(),
            Some("name is required")
        );
        assert!(!is_auto_default_name("_", ""));
        assert!(!is_auto_default_name("", ""));
    }
}
