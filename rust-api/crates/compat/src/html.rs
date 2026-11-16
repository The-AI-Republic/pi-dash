//! Minimal HTML escaping (PIDASHCONV-1623).
//!
//! Canonical home of the 5-char escaper every HTML-rendering module
//! used to keep privately. Moved from `api::auth_session::render`
//! (the CSRF-failure page's `{{ root_url }}` substitution); the OSS
//! caller imports it and the old path keeps a `pub use` shim.
//!
//! This is NOT the full-HTML sanitizer: `sanitize_html` (exported at
//! the SDK `compat` root from `api::space::sanitize`) strips tags and
//! attributes, while [`html_escape`] only escapes the five markup
//! chars. Keep both.

/// Django autoescape for a substituted value (`&<>"'`; Django renders
/// `'` as `&#x27;`). Moved from `api::auth_session::render`.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_covers_the_five_chars() {
        assert_eq!(html_escape(""), "");
        assert_eq!(html_escape("plain"), "plain");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(html_escape("\"q\""), "&quot;q&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
        assert_eq!(html_escape("&<>\"'"), "&amp;&lt;&gt;&quot;&#x27;");
        // Everything else passes through untouched.
        assert_eq!(html_escape("caf\u{e9} \u{4e2d}\n"), "caf\u{e9} \u{4e2d}\n");
    }
}
