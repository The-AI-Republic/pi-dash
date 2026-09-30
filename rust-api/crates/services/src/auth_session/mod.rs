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
//! Plus the read/write query kernel named by PIDASHCONV-382:
//!
//! * [`queries`] — user/session/instance SQL builders, adapter create/update
//!   descriptors, the invite-join write sequence, F-05 password delegation.
//!
//! Plus the guard kernel named by PIDASHCONV-393:
//!
//! * [`guards`] — per-endpoint permission/throttle table and the DRF
//!   `SimpleRateThrottle` counter math (`rate_limit.py`, the view
//!   declarations, and the DRF settings defaults).
//!
//! Plus the publish-side task kernel named by PIDASHCONV-405:
//!
//! * [`tasks`] — `magic_link` + `user_activation_email` + `forgot_password`
//!   Celery v2 emit builders (task names, positional arg orders, triggering
//!   conditions) and the magic redis pre-state builders.
//!
//! Sibling D-16 issues (handlers, guards, queries, tasks) consume these pieces
//! read-only.

pub mod email;
pub mod guards;
pub mod models;
pub mod queries;
pub mod shapes;
pub mod tasks;

pub use guards::{
    allow_request, anon_cache_key, drf_throttled_body, endpoint_guards, parse_rate,
    throttle_cache_key, throttle_denied_body, throttle_denied_json, throttle_wait,
    unauthenticated_body, user_cache_key, AuthEndpoint, EndpointGuards, PermissionPolicy, Surface,
    ThrottleDecision, ThrottlePolicy, AUTHENTICATION_THROTTLE_RATE, AUTHENTICATION_THROTTLE_SCOPE,
    DEFAULT_ANON_RATE, DEFAULT_ANON_SCOPE, EMAIL_VERIFICATION_THROTTLE_RATE,
    EMAIL_VERIFICATION_THROTTLE_SCOPE,
};

pub use shapes::{
    allowed_hosts_for, base_host, csrf_context, endpoint_kind, error_code, error_dict_json,
    error_pairs, get_safe_redirect_url, is_password_too_weak, netloc_of, not_authenticated_body,
    python_bool, quote_plus, redirection_path_str, select_redirection_path, throttle_error_pairs,
    url_has_allowed_host_and_scheme, validate_next_path, EndpointKind, HostSettings, ParamValue,
    RedirectionTarget, AUTHENTICATION_ERROR_CODES, JSON_400_SLUGS, PASSWORD_MIN_SCORE,
    REDIRECT_302_SLUGS, USER_SERIALIZER_FIELDS, USER_SERIALIZER_READ_ONLY_FIELDS,
};
