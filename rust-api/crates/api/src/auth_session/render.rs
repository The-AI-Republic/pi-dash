#![forbid(unsafe_code)]

//! D-16 authentication HTTP rendering (stage 5, PIDASHCONV-340).
//!
//! Thin HTTP shell over the `pidash_services::auth_session` kernel:
//! JSON 400 bodies, 302 redirect locations, and the `csrf_failure`
//! page (`authentication/views/common.py:37-45`). No logic lives here
//! beyond status codes and byte layout; sibling D-16 handler issues
//! call these from their routes.
//!
//! The CSRF template below is a byte copy of
//! `apps/api/templates/csrf_failure.html` (embedded because the
//! quarantine forbids reading outside `rust-api/` at build time;
//! template drift is owned by the domain gate). Only `{{ root_url }}`
//! is substituted; `reason` never renders (ported quirk).

use pidash_services::auth_session::{error_dict_json, get_safe_redirect_url, ParamValue};
use serde_json::{Map, Value};

/// `Response(exc.get_error_dict(), 400)` status.
pub const JSON_400_STATUS: u16 = 400;
/// `HttpResponseRedirect(url)` status.
pub const REDIRECT_302_STATUS: u16 = 302;
/// Throttle-denial status (`rate_limit.py`, `exception.py:26-31`).
pub const THROTTLE_429_STATUS: u16 = 429;
/// `NotAuthenticated` passthrough status (`exception.py:22-24`).
pub const UNAUTHENTICATED_401_STATUS: u16 = 401;

/// JSON 400 body as a `Value`, key order preserved (this crate's
/// `serde_json` carries `preserve_order`, so insertion order — the
/// `get_error_dict` order — survives rendering).
pub fn json_error_body(pairs: &[(String, Value)]) -> Value {
    let mut map = Map::with_capacity(pairs.len());
    for (k, v) in pairs {
        map.insert(k.clone(), v.clone());
    }
    Value::Object(map)
}

/// Byte-exact JSON 400 body string (DRF key order).
pub fn json_error_string(pairs: &[(String, Value)]) -> String {
    error_dict_json(pairs)
}

/// 302 `Location` for an error redirect: `get_safe_redirect_url`
/// over the error-dict pairs in order.
pub fn redirect_location(
    base_url: &str,
    next_path: &str,
    params: &[(&str, ParamValue)],
    allowed_hosts: &[&str],
) -> String {
    get_safe_redirect_url(base_url, next_path, params, allowed_hosts)
}

/// Django autoescape for the single substituted value (`&<>"'`;
/// Django renders `'` as `&#x27;`).
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

/// `apps/api/templates/csrf_failure.html`, verbatim except the
/// `{{ root_url }}` marker (replaced by [`render_csrf_failure`]).
pub const CSRF_FAILURE_TEMPLATE: &str = r#"<!-- templates/csrf_failure.html -->
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>CSRF Verification Failed</title>
    <style>
      body {
        font-family: Arial, sans-serif;
        background-color: #f8f9fa;
        padding: 50px;
        display: flex;
        justify-content: center;
        align-items: center;
        height: 100vh;
        margin: 0;
      }
      .card {
        max-width: 400px;
        padding: 30px;
        background-color: #ffffff;
        border-radius: 8px;
        box-shadow: 0 4px 8px rgba(0, 0, 0, 0.1);
      }
      .card-header {
        text-align: center;
        margin-bottom: 20px;
      }
      .btn-primary {
        display: block;
        width: 100%;
        padding: 10px;
        background-color: #007bff;
        color: #fff;
        text-align: center;
        text-decoration: none;
        border: none;
        border-radius: 4px;
        cursor: pointer;
      }
      .btn-primary:hover {
        background-color: #0056b3;
      }
    </style>
  </head>
  <body>
    <div class="card">
      <div class="card-header">
        <h3>CSRF Verification Failed</h3>
      </div>
      <div class="card-body">
        <p>
          It looks like your form submission has expired or there was a problem
          with your request.
        </p>
        <p>Please try the following:</p>
        <ul>
          <li>Refresh the page and try submitting the form again.</li>
          <li>Ensure that cookies are enabled in your browser.</li>
        </ul>
        <a href="{{ root_url }}" class="btn-primary">Go to Home Page</a>
      </div>
    </div>
  </body>
</html>
"#;

/// `csrf_failure` page bytes: the template with the autoescaped
/// `root_url` substituted (Django `render()` semantics for this
/// single-variable template).
pub fn render_csrf_failure(root_url: &str) -> String {
    CSRF_FAILURE_TEMPLATE.replace("{{ root_url }}", &html_escape(root_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pidash_services::auth_session::{csrf_context, error_pairs};

    #[test]
    fn json_body_preserves_error_dict_order() {
        let pairs = error_pairs(
            5065,
            "AUTHENTICATION_FAILED_SIGN_IN",
            &[("email", ParamValue::Str("u@x.com".to_owned()))],
        );
        let body = json_error_body(&pairs);
        assert_eq!(
            serde_json::to_string(&body).expect("serializes"),
            "{\"error_code\":5065,\"error_message\":\"AUTHENTICATION_FAILED_SIGN_IN\",\"email\":\"u@x.com\"}"
        );
        assert_eq!(
            json_error_string(&pairs),
            serde_json::to_string(&body).expect("serializes")
        );
    }

    #[test]
    fn redirect_location_matches_kernel_golden() {
        let location = redirect_location(
            "http://localhost:8000/spaces/",
            "https://evil.com/x",
            &[
                ("error_code", ParamValue::Int(5090)),
                (
                    "error_message",
                    ParamValue::Str("INVALID_MAGIC_CODE_SIGN_IN".to_owned()),
                ),
                ("email", ParamValue::Str("u@x.com".to_owned())),
            ],
            &["localhost:8000"],
        );
        assert_eq!(
            location,
            "http://localhost:8000/spaces/?next_path=/x&error_code=5090&error_message=INVALID_MAGIC_CODE_SIGN_IN&email=u%40x.com"
        );
    }

    #[test]
    fn csrf_page_renders_root_url_only() {
        let ctx = csrf_context("REASON_XYZ_123", "http://localhost:8000");
        let root = ctx
            .iter()
            .find(|(k, _)| k == "root_url")
            .expect("root_url")
            .1
            .as_str()
            .expect("str")
            .to_owned();
        let page = render_csrf_failure(&root);
        assert!(page.starts_with("<!-- templates/csrf_failure.html -->"));
        assert!(page.contains(
            "<a href=\"http://localhost:8000\" class=\"btn-primary\">Go to Home Page</a>"
        ));
        // `reason` is passed in the context but never renders.
        assert!(!page.contains("REASON_XYZ_123"));
        assert!(render_csrf_failure("http://h/?a=1&b=2").contains("http://h/?a=1&amp;b=2"));
        assert_eq!(JSON_400_STATUS, 400);
        assert_eq!(REDIRECT_302_STATUS, 302);
        assert_eq!(THROTTLE_429_STATUS, 429);
        assert_eq!(UNAUTHENTICATED_401_STATUS, 401);
    }
}
