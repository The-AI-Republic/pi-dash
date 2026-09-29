//! Authentication session/email/magic/password/CSRF domain (D-16, stage 5).
//!
//! Ports the model layer named by PIDASHCONV-370:
//!
//! * [`models`] — `Session` + `SessionStore` (`db/models/session.py`) and
//!   the `User` auth columns (`db/models/user.py:56-137`), as pure
//!   functions over row/JSON types. No tables are created here and no
//!   database handle is held: Django stays the schema owner (no
//!   migrations), and the SQL read/write half lives with the queries
//!   layer (PIDASHCONV-382).
//!
//! Plus the error/shape kernel named by PIDASHCONV-340:
//!
//! * [`shapes`] — `AUTHENTICATION_ERROR_CODES`, `get_error_dict`
//!   semantics, JSON-400 vs 302 mapping, safe-redirect builder,
//!   `base_host`, redirection-path selector, `zxcvbn` threshold,
//!   `csrf_failure` context, `UserSerializer` field list.
//!
//! Sibling D-16 issues (handlers, guards, queries) consume these pieces
//! read-only.

pub mod models;
pub mod shapes;

pub use shapes::{
    allowed_hosts_for, base_host, csrf_context, endpoint_kind, error_code, error_dict_json,
    error_pairs, get_safe_redirect_url, is_password_too_weak, netloc_of, not_authenticated_body,
    python_bool, quote_plus, redirection_path_str, select_redirection_path, throttle_error_pairs,
    url_has_allowed_host_and_scheme, validate_next_path, EndpointKind, HostSettings, ParamValue,
    RedirectionTarget, AUTHENTICATION_ERROR_CODES, JSON_400_SLUGS, PASSWORD_MIN_SCORE,
    REDIRECT_302_SLUGS, USER_SERIALIZER_FIELDS, USER_SERIALIZER_READ_ONLY_FIELDS,
};
