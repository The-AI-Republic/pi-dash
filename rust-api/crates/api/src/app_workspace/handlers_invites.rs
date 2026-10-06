//! Workspace invitations + join + join-request endpoints (D-24, stage 5, PIDASHCONV-617).
//!
//! Ports the 5 handler units from `apps/api/pi_dash/app/views/workspace/invite.py:37-305`
//! and `app/views/workspace/join_request.py:32-255`, routes
//! `apps/api/pi_dash/app/urls/workspace.py:67-111`:
//!
//! * `WorkspaceInvitationsViewset` — admin-gated list/retrieve/patch/delete + create
//!   (`invite.py:37-148`)
//! * `WorkspaceJoinEndpoint.post` — AllowAny accept/reject (`invite.py:150-236`)
//! * `WorkspaceJoinEndpoint.get` + `UserWorkspaceInvitationsViewSet` — invite detail,
//!   my-list, bulk accept (`invite.py:238-305`)
//! * `UserWorkspaceJoinRequestViewSet` — own list + neutral create (`join_request.py:32-154`)
//! * `WorkspaceJoinRequestViewSet` — owner-gated pending list + approve/deny
//!   (`join_request.py:156-255`)
//!
//! Wiring only, no new logic: shapes from
//! `pidash_services::app_workspace::ser_invite` (PIDASHCONV-601) and
//! `ser_workspace` (PIDASHCONV-600), SQL predicates from
//! `pidash_services::app_workspace::queries_membership` (PIDASHCONV-609),
//! gates from [`super::gates`] (PIDASHCONV-613), enqueues from
//! `pidash_services::app_workspace::tasks` (PIDASHCONV-614). Fixture:
//! `rust-api/fixtures/app_workspace/handlers/routes.golden.json` (F-W24-15;
//! trace: `rust-api/fixtures/app_workspace/TRACE.md`).
//!
//! Request order per route (Django's order, preserved): session auth via
//! [`crate::license::resolve_actor`] (401 anonymous, except the two
//! `AllowAny` join actions), then the `permission_classes` membership gate
//! (403 [`super::gates::CLASS_DENIED_BODY`]; unknown slugs 403 here, never
//! 404), then `@invalidate_cache` keys (which run before the body even when
//! the body later 4xx — the decorators wrap the method), then the handler
//! body. Lists are plain JSON arrays: no `pagination_class` is configured
//! (`settings/common.py` carries no `DEFAULT_PAGINATION_CLASS`), so the
//! inherited `ModelViewSet.list` never paginates and the `BasePaginator`
//! cursor params are ignored on these routes.
//!
//! Ported bugs and quirks (translate, don't redesign; also listed in the PR):
//!
//! * BUG-jwt-dict (`invite.py:96-100`, R10): the `"email"` JWT claim holds
//!   the WHOLE per-email dict (address plus role), not the address.
//! * BUG-invalid-email-repr (`invite.py:105-111`): the 400 interpolates the
//!   Python `repr` of the whole email dict (single quotes, insertion order).
//! * BUG-join-last-workspace (`invite.py:204-205`, R12): `user.last_workspace_id`
//!   is set on the `User` model, which has no such column (it lives on
//!   `Profile`) — Django drops it on save, so only `updated_at` is stamped
//!   ([`user_touch_sql`]); the approve path writes `Profile` instead (R18).
//! * BUG-stuck-invite (`invite.py:222-231`, R12): when the invitee has no
//!   account, or rejects, the invite is kept with `responded_at` set —
//!   permanently "already responded".
//! * BUG-no-created-by (`invite.py:197-201`, R12): the view passes no
//!   `created_by` for the join-created member (the bulk path, R14, and
//!   approve, R18, set it explicitly), so the crum save-stamp provides
//!   it — the responder when authed, NULL when anonymous.
//! * BUG-owner-no-active (`permissions/workspace.py:51-58`): the owner gate
//!   has no `is_active` filter — deactivated admins still pass.
//! * BUG-admin-admits-member (`permissions/workspace.py:61-71`): despite the
//!   name, the "admin" gate admits `Member` (15) too.
//! * Owner-vs-member 404 split: inherited `retrieve` 404s through DRF's
//!   `Http404` (`{"Detail": "No <Model> matches the given query."}`) while
//!   the custom `destroy` and the join `get`/`post` 404 through the
//!   base-view `ObjectDoesNotExist` branch
//!   (`{"error": "The required object does not exist."}`).
//! * `accepted` is raw Python truthiness (`invite.py:177`): `"false"`,
//!   `1`, `[0]` all accept.
//! * `role` is raw `int()` (`invite.py:63`): `"20"` passes, `"20.0"` 500s,
//!   huge values 400/500 by sign.
//! * `message` on join-request create rides `TextField.get_prep_value`
//!   (`join_request.py:117-123`), which is `str(value)`: bools store as
//!   `True`/`False`, dicts/lists store as their repr (and 201),
//!   numbers store as their Python rendering.
//! * `my-invitations` accept takes any iterable (`invite.py:257-259`):
//!   strings/dicts iterate (chars/keys), non-iterables 500, unparseable
//!   UUIDs 400 `"Please provide valid detail"`.
//! * Approve busts cache keys even when it then 400s (decorators wrap the
//!   method); deny busts nothing.
//!
//! Documented approximations (no contract or fixture input covers them):
//!
//! * JWT non-ASCII payloads: PyJWT dumps with `ensure_ascii`, `serde_json`
//!   emits raw UTF-8. Both decode identically; only the token string
//!   differs, and only for non-ASCII (IDN-domain) invitee emails.
//! * Non-ASCII `role` digit strings (e.g. Arabic-Indic digits, which
//!   Python `int()` accepts) answer 500 instead of coercing.
//! * Redis transport failures on cache busts are swallowed: the key math
//!   is exact, but a down cache never breaks a response.

use std::collections::HashMap;

use axum::extract::{Path, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use chrono::{DateTime, SubsecRound, Utc};
use http_body_util::BodyExt;
use serde_json::{Map, Value};
use sqlx::PgPool;

use pidash_services::app_project::ser_member as lite_ws;
use pidash_services::app_project::ser_shared as lite_user;
use pidash_services::app_workspace::{
    models_workspace, queries_membership as queries, ser_invite, ser_workspace, tasks,
};

use super::gates;
use crate::middleware::SessionHandle;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Exact bodies
// ---------------------------------------------------------------------------

/// Invite create without `emails` (400, `invite.py:56-57`).
pub const EMAILS_REQUIRED_BODY: &str = r#"{"error":"Emails are required"}"#;
/// Invite create with an invited role above the requester's (400, `invite.py:63-67`).
pub const HIGHER_ROLE_BODY: &str = r#"{"error":"You cannot invite a user with higher role"}"#;
/// Invite create when some invitees are already members (400, `invite.py:79-86`;
/// the `workspace_users` key is appended by the handler).
pub const ALREADY_MEMBER_ERROR: &str = "Some users are already member of workspace";
/// Join post with a missing/mismatched token (403, `invite.py:169-173`).
pub const JOIN_FORBIDDEN_BODY: &str =
    r#"{"error":"You do not have permission to join the workspace"}"#;
/// Second response to an invite (400, `invite.py:233-236`).
pub const ALREADY_RESPONDED_INVITE_BODY: &str =
    r#"{"error":"You have already responded to the invitation request"}"#;
/// Join-request create with a malformed admin email (400, `join_request.py:55-59`).
pub const ADMIN_EMAIL_REQUIRED_BODY: &str =
    r#"{"error":"A valid workspace admin email is required"}"#;
/// Join-request create with the requester's own email (400, `join_request.py:62-66`).
pub const OWN_EMAIL_BODY: &str =
    r#"{"error":"You cannot request to join a workspace using your own email"}"#;
/// Approve/deny of a non-pending request (400, `join_request.py:190-194,244-248`).
pub const ALREADY_RESPONDED_REQUEST_BODY: &str =
    r#"{"error":"This request has already been responded to"}"#;
/// Inherited `retrieve` miss (404): DRF renders the `get_object_or_404`
/// `Http404("No <Model> matches the given query.")` through
/// `exception_handler` (`{'Detail': ...}`), verified live against DRF 3.15.2.
pub const INVITE_NOT_FOUND_BODY: &str =
    r#"{"Detail":"No WorkspaceMemberInvite matches the given query."}"#;
/// Approve/deny fetch miss (404, `join_request.py:188,242`).
pub const JOIN_REQUEST_NOT_FOUND_BODY: &str =
    r#"{"Detail":"No WorkspaceJoinRequest matches the given query."}"#;
/// Bare `.get()` miss (404, `app/views/base.py:232-236`): custom `destroy`
/// and both join actions.
pub const MISSING_OBJECT_BODY: &str = r#"{"error":"The required object does not exist."}"#;
/// `handle_exception`'s `IntegrityError` branch (400, `app/views/base.py:220-224`).
pub const INVALID_PAYLOAD_BODY: &str = r#"{"error":"The payload is not valid"}"#;
/// `handle_exception`'s `ValidationError` branch (400, `app/views/base.py:226-230`):
/// unparseable UUIDs in the accept list.
pub const VALID_DETAIL_BODY: &str = r#"{"error":"Please provide valid detail"}"#;
/// `handle_exception`'s fallthrough (500, `app/views/base.py:244-248`).
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// Malformed-JSON 400 (`app_pages` precedent: DRF's `ParseError` prefix;
/// the `serde_json` suffix is backend-specific and not ported).
const JSON_PARSE_ERROR: &str = "JSON parse error";
/// Invite create success (200, `invite.py:142`).
const INVITES_SENT_MESSAGE: &str = "Emails sent successfully";
/// Join accept (200, `invite.py:222-225`).
const JOIN_ACCEPTED_MESSAGE: &str = "Workspace Invitation Accepted";
/// Join reject (200, `invite.py:228-231`).
const JOIN_REJECTED_MESSAGE: &str = "Workspace Invitation was not accepted";
/// Join-request create neutral success (201, `join_request.py:152-153`).
const REQUEST_SENT_MESSAGE: &str = "Request sent";
/// Already-member short-circuit (200, `join_request.py:97-100`).
const ALREADY_MEMBER_MESSAGE: &str = "You are already a member of this workspace";
/// Approve success (200, `join_request.py:239`).
const REQUEST_APPROVED_MESSAGE: &str = "Request approved";
/// Deny success (200, `join_request.py:255`).
const REQUEST_DENIED_MESSAGE: &str = "Request denied";

// ---------------------------------------------------------------------------
// Handler-level denial
// ---------------------------------------------------------------------------

/// What an invite/join handler answers without running the happy path.
#[derive(Debug)]
pub enum Denial {
    /// Anonymous on a guarded route: 401 [`gates::ANON_BODY`].
    Unauthorized,
    /// Authenticated but gated out: 403 [`gates::CLASS_DENIED_BODY`].
    ForbiddenClass,
    /// Join token mismatch: 403 [`JOIN_FORBIDDEN_BODY`].
    JoinForbidden,
    /// Inherited retrieve miss: 404 `Detail` body.
    InviteNotFound,
    /// Approve/deny fetch miss: 404 `Detail` body.
    JoinRequestNotFound,
    /// Bare `.get()` miss: 404 `error` body.
    MissingObject,
    /// 400, `{"error": ...}`.
    BadError(String),
    /// 400, `{"Detail": ...}` (body parse errors).
    BadDetail(String),
    /// 400, pre-rendered serializer-errors body (`{"field": [...]}`).
    BadJson(Value),
    /// Database failure / unexpected shape: 500 [`SERVER_ERROR_BODY`].
    ServerError,
}

/// `{"Detail": message}` envelope (DRF `exception_handler` shape).
fn detail_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("Detail".to_owned(), Value::String(message));
    Value::Object(body)
}

/// `{"error": message}` envelope (`handle_exception` branch shape).
fn error_envelope(message: String) -> Value {
    let mut body = Map::new();
    body.insert("error".to_owned(), Value::String(message));
    Value::Object(body)
}

/// `{"message": message}` envelope (success shapes).
fn message_envelope(message: &str) -> Value {
    let mut body = Map::new();
    body.insert("message".to_owned(), Value::String(message.to_owned()));
    Value::Object(body)
}

fn json_response(status: StatusCode, body: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_owned()))
        .expect("invites-handler response")
}

impl IntoResponse for Denial {
    fn into_response(self) -> Response {
        match self {
            Denial::Unauthorized => json_response(StatusCode::UNAUTHORIZED, gates::ANON_BODY),
            Denial::ForbiddenClass => {
                json_response(StatusCode::FORBIDDEN, gates::CLASS_DENIED_BODY)
            }
            Denial::JoinForbidden => json_response(StatusCode::FORBIDDEN, JOIN_FORBIDDEN_BODY),
            Denial::InviteNotFound => json_response(StatusCode::NOT_FOUND, INVITE_NOT_FOUND_BODY),
            Denial::JoinRequestNotFound => {
                json_response(StatusCode::NOT_FOUND, JOIN_REQUEST_NOT_FOUND_BODY)
            }
            Denial::MissingObject => json_response(StatusCode::NOT_FOUND, MISSING_OBJECT_BODY),
            Denial::BadError(message) => {
                (StatusCode::BAD_REQUEST, Json(error_envelope(message))).into_response()
            }
            Denial::BadDetail(message) => {
                (StatusCode::BAD_REQUEST, Json(detail_envelope(message))).into_response()
            }
            Denial::BadJson(value) => (StatusCode::BAD_REQUEST, Json(value)).into_response(),
            Denial::ServerError => {
                json_response(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_BODY)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request plumbing (the `app_scheduler` precedent)
// ---------------------------------------------------------------------------

fn pool_of(state: &AppState) -> Result<&PgPool, Denial> {
    state
        .pools()
        .map(|pools| pools.primary())
        .ok_or(Denial::ServerError)
}

/// Read the request body as JSON: empty bytes are `{}` (a bodiless POST
/// carries no content type, so DRF parses `{}`, not a `ParseError`);
/// malformed JSON is the 400 `ParseError` prefix.
async fn read_json_body(req: Request) -> Result<Value, Denial> {
    let (_parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Err(Denial::ServerError),
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes).map_err(|_| Denial::BadDetail(JSON_PARSE_ERROR.to_owned()))
}

/// Whether a sqlx failure is an integrity violation (SQLSTATE class `23`),
/// which Django's `handle_exception` answers 400 for (`app_pages` precedent).
fn is_integrity_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code.starts_with("23"))
}

/// Convert a [`queries`] `:named` statement to sqlx `$n` placeholders.
/// Numbering follows `params` order; a `:name` only matches when followed
/// by a non-identifier byte, so `:now` never matches inside `:now2`.
pub fn positional(sql: &str, params: &[&str]) -> String {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            if j > i + 1 {
                let name = &sql[i + 1..j];
                if let Some(position) = params.iter().position(|candidate| *candidate == name) {
                    out.push('$');
                    out.push_str(&(position + 1).to_string());
                    i = j;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Python semantics (probed live against CPython + Django 4.2.30 / DRF 3.15.2)
// ---------------------------------------------------------------------------

/// Python `str.strip()` with no arguments: Unicode whitespace plus
/// `\x1c`-`\x1f`, which Rust's `char::is_whitespace` does not cover.
fn py_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// What CPython `int()` strips: Unicode whitespace EXCEPT `\x1c`-`\x1f`
/// (probed — `int("\x1c5")` raises while `"\x1c5".strip()` is `"5"`),
/// i.e. exactly `char::is_whitespace`.
fn py_int_strip(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace())
}

/// Python `int()` over a JSON value (`invite.py:63`): bools are 0/1,
/// floats truncate toward zero, strings parse after stripping with an
/// optional sign (underscores allowed between digits, ASCII digits only —
/// non-ASCII digit strings are a documented approximation). Anything
/// else (null, arrays, objects, unparseable strings) is a `TypeError` /
/// `ValueError`, which `handle_exception` answers 500 for.
fn py_int(value: &Value) -> Result<i128, ()> {
    match value {
        Value::Bool(true) => Ok(1),
        Value::Bool(false) => Ok(0),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i as i128)
            } else if let Some(u) = n.as_u64() {
                Ok(u as i128)
            } else if let Some(f) = n.as_f64() {
                // `arbitrary_precision` numbers that fit neither int form
                // are floats; huge magnitudes saturate (still off-scale
                // for the role comparison, which is all the caller needs).
                Ok(f.trunc().clamp(i128::MIN as f64, i128::MAX as f64) as i128)
            } else if !n.to_string().contains(['.', 'e', 'E']) {
                // `as_f64` filters non-finite, so `None` on a digit-only
                // literal is a Python int past f64 range (`int()` still
                // succeeds): saturate by sign like the string arm below.
                Ok(if n.to_string().starts_with('-') {
                    -i128::MAX
                } else {
                    i128::MAX
                })
            } else {
                // A float literal past f64 range (`1e999`): Python's
                // `int()` raises `OverflowError` → 500.
                Err(())
            }
        }
        Value::String(raw) => {
            let text = py_int_strip(raw);
            let digits = text
                .strip_prefix('+')
                .or_else(|| text.strip_prefix('-'))
                .unwrap_or(text);
            if digits.is_empty() {
                return Err(());
            }
            // Underscores only between digits (`int("2_0") == 20`).
            let mut cleaned = String::with_capacity(digits.len());
            let mut prev_underscore = true;
            for c in digits.chars() {
                if c == '_' {
                    if prev_underscore {
                        return Err(());
                    }
                    prev_underscore = true;
                } else if c.is_ascii_digit() {
                    cleaned.push(c);
                    prev_underscore = false;
                } else {
                    return Err(());
                }
            }
            if prev_underscore {
                return Err(());
            }
            // Saturate on overflow: a huge positive still 400s the role
            // cap and a huge negative still 500s the smallint column —
            // the same outcomes unbounded Python ints get.
            let magnitude: i128 = cleaned.parse().unwrap_or(i128::MAX);
            Ok(if text.starts_with('-') {
                -magnitude
            } else {
                magnitude
            })
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Err(()),
    }
}

/// Python truthiness (`invite.py:177`): `accepted` is whatever
/// `request.data.get("accepted", False)` is truthy for — `"false"`, `1`
/// and `[0]` all accept.
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(u) = n.as_u64() {
                u != 0
            } else {
                // `as_f64` filters non-finite, so `None` here is a ±inf
                // literal — and Python `bool(inf)` is `True`.
                n.as_f64().is_none_or(|f| f != 0.0)
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// Python `repr()` of a JSON scalar's float: the shortest round-trip
/// digits (as `serde_json` renders them) laid out by CPython's rule —
/// fixed notation when `-4 < point <= 16` (where the value is
/// `0.digits × 10^point`), scientific with `e±XX` otherwise, and a
/// `.0` whenever fixed notation would show no point (`1e16` →
/// `"1e+16"`, `1.5e-5` → `"1.5e-05"`, `5.0` → `"5.0"`).
fn py_float_repr(f: f64) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    let raw = serde_json::Number::from_f64(f.abs())
        .map(|n| n.to_string())
        .unwrap_or_default();
    // Split the ryu rendering into digit string + decimal point.
    let (mantissa, exp_value) = match raw.split_once('e') {
        Some((mantissa, exp)) => (mantissa, exp.parse::<i32>().unwrap_or(0)),
        None => (raw.as_str(), 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (mantissa, ""),
    };
    let digits = format!("{int_part}{frac_part}");
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // value = D × 10^E with D = int(digits).
    let exp_total = exp_value - frac_part.len() as i32;
    let point = exp_total + digits.len() as i32;
    let sign = if f.is_sign_negative() { "-" } else { "" };
    if !(-4 < point && point <= 16) {
        let head = &digits[..1];
        let tail = &digits[1..];
        let tail = tail.trim_end_matches('0');
        let coefficient = if tail.is_empty() {
            head.to_owned()
        } else {
            format!("{head}.{tail}")
        };
        let exp = point - 1;
        let (esign, edigits) = if exp < 0 {
            ("-", (-exp).to_string())
        } else {
            ("+", exp.to_string())
        };
        let edigits = if edigits.len() < 2 {
            format!("0{edigits}")
        } else {
            edigits
        };
        return format!("{sign}{coefficient}e{esign}{edigits}");
    }
    if point <= 0 {
        return format!("{sign}0.{}{digits}", "0".repeat(-point as usize));
    }
    if point as usize >= digits.len() {
        let mut out = format!(
            "{sign}{digits}{}",
            "0".repeat(point as usize - digits.len())
        );
        out.push_str(".0");
        return out;
    }
    let (head, tail) = digits.split_at(point as usize);
    format!("{sign}{head}.{tail}")
}

/// Python `str()` of a JSON number (`str(data)` in `ChoiceField`, the
/// `UUIDField` error displays, `TextField.get_prep_value`): ints render
/// exact digits (parsed, so `-0` is `"0"`), float literals render via
/// [`py_float_repr`], and float literals past f64 range render
/// `"inf"`/`"-inf"`. `as_f64` filters non-finite, so `None` there means
/// ±inf for float syntax — but a digit-only literal past f64 range is a
/// Python int and keeps its exact digits.
fn py_num_str(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else if let Some(f) = n.as_f64() {
        let raw = n.to_string();
        if raw.contains(['.', 'e', 'E']) {
            py_float_repr(f)
        } else {
            raw
        }
    } else {
        let raw = n.to_string();
        if raw.contains(['.', 'e', 'E']) {
            if raw.starts_with('-') {
                "-inf".to_owned()
            } else {
                "inf".to_owned()
            }
        } else {
            raw
        }
    }
}

/// Python `repr()` of a JSON string: single quotes unless the value holds
/// a single quote and no double quote; control characters escaped,
/// printable Unicode verbatim.
fn py_str_repr(value: &str) -> String {
    let use_double = value.contains('\'') && !value.contains('"');
    let quote = if use_double { '"' } else { '\'' };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `repr()` of a JSON value, for the invalid-email 400
/// (`invite.py:106-111`): dicts render insertion-ordered with
/// single-quoted keys (`serde_json` runs with `preserve_order`, so the
/// wire order survives parsing).
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => py_str_repr(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_str_repr(k), py_repr(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

/// `str(timezone.now())` (`invite.py:137,215,284`, `join_request.py:235`):
/// `YYYY-MM-DD HH:MM:SS[.ffffff]+HH:MM` — the fraction is omitted when the
/// microsecond is exactly zero (probed live).
fn django_str_now(now: &DateTime<Utc>) -> String {
    let base = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let micros = now.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

/// `datetime.now().timestamp()` (`invite.py:97`): epoch seconds as an f64.
/// Microsecond precision like CPython (whole microseconds, exactly
/// representable below 2^53).
fn epoch_float(now: &DateTime<Utc>) -> f64 {
    let micros = now.timestamp_micros();
    micros as f64 / 1_000_000.0
}

/// `auto_now` for the invite PATCH save: Django's clock resolves
/// microseconds, so the value bound (and echoed in the 200) does too —
/// a raw `Utc::now()` carries sub-microsecond digits that render as 9
/// fraction digits where DRF renders 6 (per the merged PIDASHCONV-620
/// precedent).
fn patch_now() -> DateTime<Utc> {
    Utc::now().round_subsecs(6)
}

/// `jwt.encode({"email": ..., "timestamp": ...}, SECRET_KEY, HS256)`
/// (`invite.py:96-100`): the `"email"` claim is the WHOLE per-email dict
/// (ported bug), the header is `{"typ":"JWT","alg":"HS256"}`, and both
/// segments dump compact (`,`/`:` separators, matching `serde_json`).
fn invite_token(secret: &[u8], email_value: &Value, timestamp: f64) -> Result<String, Denial> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    let mut payload = Map::with_capacity(2);
    payload.insert("email".to_owned(), email_value.clone());
    payload.insert(
        "timestamp".to_owned(),
        serde_json::Number::from_f64(timestamp)
            .map(Value::Number)
            .ok_or(Denial::ServerError)?,
    );
    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".to_owned());
    encode(
        &header,
        &Value::Object(payload),
        &EncodingKey::from_secret(secret),
    )
    .map_err(|_| Denial::ServerError)
}

/// Split the direct `validate_email` call (`invite.py:91`) into
/// Django's three outcomes (probed live): falsy input (`None`, `""`,
/// `0`, `[]`, …) fails the validator's `not value` check
/// (`ValidationError` → 400); a container holding `"@"` passes the
/// membership check and dies in `.rsplit` (`AttributeError` → 500);
/// containers without it fail validation (400); truthy
/// numbers/bools fail the `in` check (`TypeError` → 500); strings go
/// to the `EmailValidator` port.
enum EmailCheck {
    /// Failed validation: answer the caller's 400.
    Invalid,
    /// `TypeError`/`AttributeError`: answer 500.
    TypeError,
    /// A string to run through [`is_valid_email_str`].
    Candidate(String),
}

fn classify_email(value: Option<&Value>) -> EmailCheck {
    match value {
        None | Some(Value::Null) => EmailCheck::Invalid,
        // Falsy guard first: `""` must take the `not value` 400, not
        // the `Candidate` arm below (behavior-identical either way —
        // the validator rejects `""` — but the test pins the shape).
        Some(value) if !py_truthy(value) => EmailCheck::Invalid,
        Some(Value::String(s)) => EmailCheck::Candidate(s.clone()),
        // `"@" in value`: element equality for lists, key lookup for
        // dicts — reaching `.rsplit` 500s; missing it 400s.
        Some(Value::Array(items))
            if items
                .iter()
                .any(|item| item.as_str().is_some_and(|s| s == "@")) =>
        {
            EmailCheck::TypeError
        }
        Some(Value::Object(map)) if map.contains_key("@") => EmailCheck::TypeError,
        Some(Value::Array(_)) | Some(Value::Object(_)) => EmailCheck::Invalid,
        Some(_) => EmailCheck::TypeError,
    }
}

/// Reuse the merged Django `EmailValidator` port rather than forking it
/// (`v1_projects::handlers_members::is_valid_email`, probed against
/// Django 4.2 in PIDASHCONV-371).
fn is_valid_email_str(value: &str) -> bool {
    crate::v1_projects::handlers_members::is_valid_email(value)
}

/// Audit columns after `BaseModel.save()` on an UPDATE
/// (`db/models/base.py:23-43`): an authed caller stamps `updated_by`
/// and keeps `created_by`; an anonymous caller nulls both. Returns
/// `(updated_by_id, created_by_id)` for the `UPDATE`.
fn audit_columns_on_update(
    caller: Option<uuid::Uuid>,
    current_created_by: Option<uuid::Uuid>,
) -> (Option<uuid::Uuid>, Option<uuid::Uuid>) {
    (caller, caller.and(current_created_by))
}

/// Parse one member JSON default fresh per row: `get_default_props` /
/// `get_issue_props` / `dict` are callables, so each Django row gets a
/// fresh dict — never share one parsed value across rows (the merged
/// `handlers_workspace.rs` precedent).
fn member_default_json(text: &str) -> Result<Value, Denial> {
    serde_json::from_str(text).map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// Rows (`sqlx::FromRow` maps by column name)
// ---------------------------------------------------------------------------

/// One `workspace_member_invites` row: every column the invite shape or a
/// write path reads (`db/models/workspace.py:236-243` over `AuditModel`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct InviteRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    email: String,
    accepted: bool,
    token: String,
    message: Option<String>,
    responded_at: Option<DateTime<Utc>>,
    role: i16,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
}

/// One `workspace_join_requests` row (`db/models/workspace.py:264-304`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct JoinRequestRow {
    id: uuid::Uuid,
    workspace_id: Option<uuid::Uuid>,
    requester_id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    admin_email: String,
    message: Option<String>,
    role: i16,
    status: String,
    responded_at: Option<DateTime<Utc>>,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
    responded_by_id: Option<uuid::Uuid>,
}

/// One `workspace_members` row for the already-member 400
/// (`db/models/workspace.py:196-211`): the full `WorkSpaceMemberSerializer`
/// core plus the scope columns.
#[derive(Debug, Clone, sqlx::FromRow)]
struct MemberRow {
    id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    member_id: uuid::Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    role: i16,
    company_role: Option<String>,
    view_props: Value,
    default_props: Value,
    issue_props: Value,
    is_active: bool,
    getting_started_checklist: Value,
    tips: Value,
    explored_features: Value,
    created_by_id: Option<uuid::Uuid>,
    updated_by_id: Option<uuid::Uuid>,
}

/// The `UserLite` columns (`user.py:141-154`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct UserLiteRow {
    id: uuid::Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_asset_id: Option<uuid::Uuid>,
    is_bot: bool,
    display_name: String,
}

/// The `WorkspaceLite` columns (`workspace.py:79-83`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct WorkspaceLiteRow {
    id: uuid::Uuid,
    name: String,
    slug: String,
    logo: Option<String>,
    logo_asset_id: Option<uuid::Uuid>,
}

// ---------------------------------------------------------------------------
// Auth + gates (DRF `initial()` order: authN, then `permission_classes`)
// ---------------------------------------------------------------------------

/// Gate/fetch SQL with a `workspace__slug` traversal. Django ignores
/// the workspace soft-delete scope on forward-FK traversal (probed),
/// so these carry NO `w.deleted_at` filter — while the member/invite/
/// request-side scopes stay. Pinned by
/// `forward_fk_traversal_ignores_workspace_scope`.
const ADMIN_GATE_SQL: &str = "SELECT w.id, wm.role FROM workspace_members wm \
     JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE w.slug = $1 AND wm.member_id = $2 \
     AND wm.is_active = TRUE AND wm.deleted_at IS NULL AND wm.role IN (20, 15)";

/// [`ADMIN_GATE_SQL`] for the owner gate (role Admin, no `is_active`).
const OWNER_GATE_SQL: &str = "SELECT w.id FROM workspace_members wm \
     JOIN workspaces w ON w.id = wm.workspace_id \
     WHERE w.slug = $1 AND wm.member_id = $2 \
     AND wm.deleted_at IS NULL AND wm.role = 20";

/// One invite by pk + workspace slug (`invite.py:145,164,239`).
const FETCH_INVITE_SQL: &str = "SELECT i.id, i.workspace_id, i.created_at, i.updated_at, \
    i.deleted_at, i.email, i.accepted, i.token, i.message, i.responded_at, i.role, \
    i.created_by_id, i.updated_by_id \
    FROM workspace_member_invites i JOIN workspaces w ON w.id = i.workspace_id \
    WHERE i.id = $1 AND w.slug = $2 AND i.deleted_at IS NULL";

/// One join request by pk + workspace slug (`join_request.py:188,242`).
const FETCH_JOIN_REQUEST_SQL: &str = "SELECT jr.id, jr.workspace_id, jr.requester_id, \
    jr.created_at, jr.updated_at, jr.deleted_at, jr.admin_email, jr.message, jr.role, \
    jr.status, jr.responded_at, jr.created_by_id, jr.updated_by_id, jr.responded_by_id \
    FROM workspace_join_requests jr JOIN workspaces w ON w.id = jr.workspace_id \
    WHERE jr.id = $1 AND w.slug = $2 AND jr.deleted_at IS NULL";

/// The caller's workspace row for admin-gated routes: role (for the
/// invite cap) plus the scoped workspace id.
struct AdminGate {
    role: i32,
    workspace_id: uuid::Uuid,
}

/// Session auth + `WorkSpaceAdminPermission` (`permissions/workspace.py:61-71`):
/// anonymous 401s; active Admin/Member passes. Unknown slugs 403 here —
/// the gate runs before any object lookup, never 404. Django's
/// `workspace__slug` traversal ignores the workspace soft-delete scope
/// (probed), so no `w.deleted_at` filter here — on a deleted workspace
/// the gate passes and the later direct `Workspace` lookup 404s.
async fn resolve_class_admin(
    state: &AppState,
    slug: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(crate::license::Actor, AdminGate), Denial> {
    let pool = pool_of(state)?;
    let actor =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    let row: Option<(uuid::Uuid, i16)> = sqlx::query_as(ADMIN_GATE_SQL)
        .bind(slug)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id, role)) = row else {
        return Err(Denial::ForbiddenClass);
    };
    Ok((
        actor,
        AdminGate {
            role: i32::from(role),
            workspace_id,
        },
    ))
}

/// Session auth + `WorkspaceOwnerPermission` (`permissions/workspace.py:51-58`):
/// role Admin with NO `is_active` check (ported as-is). The
/// `workspace__slug` traversal is unscoped on the workspace side (see
/// [`resolve_class_admin`]).
async fn resolve_class_owner(
    state: &AppState,
    slug: &str,
    extension: Option<Extension<SessionHandle>>,
) -> Result<(crate::license::Actor, uuid::Uuid), Denial> {
    let pool = pool_of(state)?;
    let actor =
        crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
            .await
            .map_err(|_| Denial::ServerError)?
            .ok_or(Denial::Unauthorized)?;
    let row: Option<(uuid::Uuid,)> = sqlx::query_as(OWNER_GATE_SQL)
        .bind(slug)
        .bind(actor.id)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let Some((workspace_id,)) = row else {
        return Err(Denial::ForbiddenClass);
    };
    Ok((actor, workspace_id))
}

/// Session auth only (the `IsAuthenticated` default on the `users/me/`
/// routes): anonymous 401s with [`gates::ANON_BODY`].
async fn resolve_authenticated(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<crate::license::Actor, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)?
        .ok_or(Denial::Unauthorized)
}

/// The `AllowAny` peek (the `app_pages` precedent): no session, no key, or
/// an unresolvable session means anonymous — the join actions run either
/// way, with the token as the only credential.
async fn peek_actor(
    state: &AppState,
    extension: Option<Extension<SessionHandle>>,
) -> Result<Option<crate::license::Actor>, Denial> {
    let pool = pool_of(state)?;
    crate::license::resolve_actor(pool, state.settings().secret_key.as_bytes(), extension)
        .await
        .map_err(|_| Denial::ServerError)
}

/// Parse `<uuid:pk>`: Django's `UUIDConverter` matches lowercase
/// hyphenated hex only (`[0-9a-f]{8}-...`, case-sensitive) — anything
/// else matches NO route and proxies to Django's resolver 404 (routing
/// precedes auth — the `app_cycles` precedent).
fn parse_pk(raw: &str) -> Result<uuid::Uuid, ()> {
    let bytes = raw.as_bytes();
    if bytes.len() != 36 {
        return Err(());
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if hyphen != (*byte == b'-') {
            return Err(());
        }
        if !hyphen && !byte.is_ascii_hexdigit() {
            return Err(());
        }
    }
    if raw.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(());
    }
    raw.parse::<uuid::Uuid>().map_err(|_| ())
}

// ---------------------------------------------------------------------------
// Cache + tasks (side effects)
// ---------------------------------------------------------------------------

/// Run one [`gates::InvalidateAction`]'s keys (`utils/cache.py:54-69`):
/// `multiple` deletes by glob (`*{key}*`), single keys delete exactly
/// (a glob without wildcards matches exactly, so the shared
/// `invalidate_matching` primitive covers both). Best-effort: a missing
/// cache or a transport failure never breaks the response.
async fn bust_action(
    state: &AppState,
    action: gates::InvalidateAction,
    slug: &str,
    user_id: Option<&str>,
) {
    let Some(redis) = state.redis() else {
        return;
    };
    for inv in gates::invalidations_for(action) {
        let (key, multiple) = gates::invalidation_key(inv, slug, user_id);
        let pattern = if multiple { format!("*{key}*") } else { key };
        let _ = redis.invalidate_matching(&pattern).await;
    }
}

/// The per-invite direct bust inside the bulk-accept body
/// (`invite.py:263-268`): `user=False`, `multiple=True`, per workspace.
async fn bust_members_key(state: &AppState, slug: &str) {
    let Some(redis) = state.redis() else {
        return;
    };
    let key = gates::bulk_create_direct_key(slug);
    let _ = redis.invalidate_matching(&format!("*{key}*")).await;
}

/// Best-effort post-commit task enqueue (the D-02 intake pattern):
/// without a queue table the response still stands.
async fn enqueue_best_effort(
    pool: &PgPool,
    task: &str,
    args: Vec<Value>,
    kwargs: Map<String, Value>,
) {
    let message = pidash_jobs::celery::CeleryTaskMessage::new(task, args, kwargs);
    let job = pidash_jobs::queue::NewJob::new(
        message.task.clone(),
        Value::Array(message.args.clone()),
        Value::Object(message.kwargs.clone()),
    );
    if let Err(error) = pidash_jobs::queue::enqueue(pool, &job).await {
        tracing::warn!(%error, task = job.task.as_str(), "task enqueue failed; response stands");
    }
}

async fn enqueue_track(pool: &PgPool, emit: &tasks::TrackEventEmit) {
    enqueue_best_effort(pool, emit.task_name(), vec![], emit.kwargs()).await;
}

async fn enqueue_invitation(pool: &PgPool, emit: &tasks::WorkspaceInvitationEmit) {
    enqueue_best_effort(pool, emit.task_name(), emit.args(), Map::new()).await;
}

// ---------------------------------------------------------------------------
// Fetch + render (rows in, `ser_invite` views out)
// ---------------------------------------------------------------------------

/// Owned render buffers for one invite: every string the borrowed
/// [`ser_invite::MemberInviteRow`] points at, plus the nested workspace
/// lite inputs.
struct RenderedInvite {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    email: String,
    accepted: bool,
    token: String,
    message: Option<String>,
    responded_at: Option<String>,
    role: i64,
    created_by: Option<String>,
    updated_by: Option<String>,
    workspace_name: String,
    workspace_slug: String,
    workspace_id: String,
    workspace_logo_url: Option<String>,
}

impl RenderedInvite {
    /// Render through [`ser_invite::invite_to_representation`] into an
    /// owned `Value` (the views borrow their rows, so they serialize
    /// inside).
    fn view_value(&self) -> Value {
        let lite_row = lite_ws::WorkspaceLiteRow {
            name: &self.workspace_name,
            slug: &self.workspace_slug,
            id: &self.workspace_id,
            logo_url: self.workspace_logo_url.as_deref(),
        };
        let workspace = lite_ws::workspace_lite_to_representation(&lite_row);
        let row = ser_invite::MemberInviteRow {
            id: &self.id,
            workspace,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
            deleted_at: self.deleted_at.as_deref(),
            email: &self.email,
            accepted: self.accepted,
            token: &self.token,
            message: self.message.as_deref(),
            responded_at: self.responded_at.as_deref(),
            role: self.role,
            created_by: self.created_by.as_deref(),
            updated_by: self.updated_by.as_deref(),
        };
        serde_json::to_value(ser_invite::invite_to_representation(&row)).unwrap_or(Value::Null)
    }
}

/// Owned render buffers for one join request (admin shape).
struct RenderedJoinRequest {
    id: String,
    workspace: Option<RenderedWorkspaceLite>,
    requester: RenderedUserLite,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    admin_email: String,
    message: Option<String>,
    role: i64,
    status: String,
    responded_at: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    responded_by: Option<String>,
}

#[derive(Clone)]
struct RenderedWorkspaceLite {
    name: String,
    slug: String,
    id: String,
    logo_url: Option<String>,
}

impl RenderedWorkspaceLite {
    fn lite_row(&self) -> lite_ws::WorkspaceLiteRow<'_> {
        lite_ws::WorkspaceLiteRow {
            name: &self.name,
            slug: &self.slug,
            id: &self.id,
            logo_url: self.logo_url.as_deref(),
        }
    }
}

#[derive(Clone)]
struct RenderedUserLite {
    id: String,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

impl RenderedUserLite {
    fn lite_row(&self) -> lite_user::UserLiteRow<'_> {
        lite_user::UserLiteRow {
            id: &self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }
}

impl RenderedJoinRequest {
    /// Render through [`ser_invite::join_request_to_representation`]
    /// into an owned `Value` (the views borrow their rows, so they
    /// serialize inside).
    fn view_value(&self) -> Value {
        let workspace_row = self.workspace.as_ref().map(|w| w.lite_row());
        let workspace = workspace_row
            .as_ref()
            .map(lite_ws::workspace_lite_to_representation);
        let requester_row = self.requester.lite_row();
        let requester = lite_user::user_lite_to_representation(&requester_row);
        let row = ser_invite::JoinRequestRow {
            id: &self.id,
            workspace,
            requester,
            created_at: &self.created_at,
            updated_at: &self.updated_at,
            deleted_at: self.deleted_at.as_deref(),
            admin_email: &self.admin_email,
            message: self.message.as_deref(),
            role: self.role,
            status: &self.status,
            responded_at: self.responded_at.as_deref(),
            created_by: self.created_by.as_deref(),
            updated_by: self.updated_by.as_deref(),
            responded_by: self.responded_by.as_deref(),
        };
        serde_json::to_value(ser_invite::join_request_to_representation(&row))
            .unwrap_or(Value::Null)
    }

    /// The requester-facing 8-key shape: no `workspace` key at all
    /// (anti-enumeration, `workspace.py:155-163`).
    fn user_view_value(&self) -> Value {
        let requester_row = self.requester.lite_row();
        let requester = lite_user::user_lite_to_representation(&requester_row);
        let row = ser_invite::UserJoinRequestRow {
            id: &self.id,
            requester,
            admin_email: &self.admin_email,
            message: self.message.as_deref(),
            status: &self.status,
            responded_at: self.responded_at.as_deref(),
            created_at: &self.created_at,
            updated_at: &self.updated_at,
        };
        serde_json::to_value(ser_invite::user_join_request_to_representation(&row))
            .unwrap_or(Value::Null)
    }
}

struct MemberOwned {
    id: String,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    company_role: Option<String>,
    view_props: Value,
    default_props: Value,
    issue_props: Value,
    getting_started_checklist: Value,
    tips: Value,
    explored_features: Value,
    created_by: Option<String>,
    updated_by: Option<String>,
}

/// Render one member through [`ser_workspace::member_to_representation`]
/// into an owned `Value` (the view borrows, so it serializes inside).
fn render_member_value(row: &MemberRow, member: &RenderedUserLite, tz: &chrono_tz::Tz) -> Value {
    let owned = MemberOwned {
        id: row.id.to_string(),
        created_at: crate::serializer::render_datetime_in(&row.created_at, tz),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        company_role: row.company_role.clone(),
        view_props: row.view_props.clone(),
        default_props: row.default_props.clone(),
        issue_props: row.issue_props.clone(),
        getting_started_checklist: row.getting_started_checklist.clone(),
        tips: row.tips.clone(),
        explored_features: row.explored_features.clone(),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
    };
    let workspace = row.workspace_id.to_string();
    let member_lite_row = member.lite_row();
    let member_view = lite_user::user_lite_to_representation(&member_lite_row);
    let member_row = ser_workspace::WorkSpaceMemberRow {
        core: ser_workspace::WorkspaceMemberCore {
            id: &owned.id,
            created_at: &owned.created_at,
            updated_at: &owned.updated_at,
            deleted_at: owned.deleted_at.as_deref(),
            role: i64::from(row.role),
            company_role: owned.company_role.as_deref(),
            view_props: &owned.view_props,
            default_props: &owned.default_props,
            issue_props: &owned.issue_props,
            is_active: row.is_active,
            getting_started_checklist: &owned.getting_started_checklist,
            tips: &owned.tips,
            explored_features: &owned.explored_features,
            created_by: owned.created_by.as_deref(),
            updated_by: owned.updated_by.as_deref(),
        },
        workspace: &workspace,
        member: member_view,
    };
    let view = ser_workspace::member_to_representation(&member_row);
    serde_json::to_value(&view).unwrap_or(Value::Null)
}

fn render_user_lite(row: &UserLiteRow, assets: &HashMap<uuid::Uuid, String>) -> RenderedUserLite {
    // `avatar_url` property (`db/models/user.py:143-151`): the asset URL
    // when `avatar_asset` is set (no fall-through when unresolvable),
    // else the avatar text when non-empty, else null.
    let avatar_url = match row.avatar_asset_id {
        Some(asset_id) => assets.get(&asset_id).cloned(),
        None if row.avatar.is_empty() => None,
        None => Some(row.avatar.clone()),
    };
    RenderedUserLite {
        id: row.id.to_string(),
        first_name: row.first_name.clone(),
        last_name: row.last_name.clone(),
        avatar: row.avatar.clone(),
        avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name.clone(),
    }
}

fn render_workspace_lite(
    row: &WorkspaceLiteRow,
    assets: &HashMap<uuid::Uuid, String>,
) -> RenderedWorkspaceLite {
    // `logo_url` property (`db/models/workspace.py:146-154`): same
    // branches as `avatar_url`.
    let logo_url = match row.logo_asset_id {
        Some(asset_id) => assets.get(&asset_id).cloned(),
        None => row.logo.as_ref().filter(|logo| !logo.is_empty()).cloned(),
    };
    RenderedWorkspaceLite {
        name: row.name.clone(),
        slug: row.slug.clone(),
        id: row.id.to_string(),
        logo_url,
    }
}

fn render_invite(
    row: &InviteRow,
    workspace: &RenderedWorkspaceLite,
    tz: &chrono_tz::Tz,
) -> RenderedInvite {
    RenderedInvite {
        id: row.id.to_string(),
        created_at: crate::serializer::render_datetime_in(&row.created_at, tz),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        email: row.email.clone(),
        accepted: row.accepted,
        token: row.token.clone(),
        message: row.message.clone(),
        responded_at: row
            .responded_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        role: i64::from(row.role),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        workspace_name: workspace.name.clone(),
        workspace_slug: workspace.slug.clone(),
        workspace_id: workspace.id.clone(),
        workspace_logo_url: workspace.logo_url.clone(),
    }
}

fn render_join_request(
    row: &JoinRequestRow,
    workspace: Option<&RenderedWorkspaceLite>,
    requester: &RenderedUserLite,
    tz: &chrono_tz::Tz,
) -> RenderedJoinRequest {
    RenderedJoinRequest {
        id: row.id.to_string(),
        workspace: workspace.cloned(),
        requester: requester.clone(),
        created_at: crate::serializer::render_datetime_in(&row.created_at, tz),
        updated_at: crate::serializer::render_datetime_in(&row.updated_at, tz),
        deleted_at: row
            .deleted_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        admin_email: row.admin_email.clone(),
        message: row.message.clone(),
        role: i64::from(row.role),
        status: row.status.clone(),
        responded_at: row
            .responded_at
            .as_ref()
            .map(|dt| crate::serializer::render_datetime_in(dt, tz)),
        created_by: row.created_by_id.map(|id| id.to_string()),
        updated_by: row.updated_by_id.map(|id| id.to_string()),
        responded_by: row.responded_by_id.map(|id| id.to_string()),
    }
}

/// Fetch `UserLite` rows for a set of user ids (one statement; `users`
/// carries no `deleted_at`).
async fn fetch_user_lites(
    pool: &PgPool,
    user_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, UserLiteRow>, Denial> {
    let mut out = HashMap::new();
    if user_ids.is_empty() {
        return Ok(out);
    }
    let placeholders: Vec<String> = (1..=user_ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT u.id, u.first_name, u.last_name, u.avatar, u.avatar_asset_id, u.is_bot, u.display_name \
         FROM users u WHERE u.id IN ({})",
        placeholders.join(", ")
    );
    let mut query = sqlx::query_as::<_, UserLiteRow>(&sql);
    for id in user_ids {
        query = query.bind(*id);
    }
    for row in query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    {
        out.insert(row.id, row);
    }
    Ok(out)
}

/// Fetch `WorkspaceLite` rows by id (one statement): NO soft-delete
/// scope — forward-FK access and `select_related` resolve through the
/// unfiltered `_base_manager`, so invites on soft-deleted workspaces
/// still render/process (slug-scoped callers pre-scope to live rows via
/// their gate/fetch predicates, matching the `workspace__slug` filter).
async fn fetch_workspace_lites(
    pool: &PgPool,
    workspace_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, WorkspaceLiteRow>, Denial> {
    let mut out = HashMap::new();
    if workspace_ids.is_empty() {
        return Ok(out);
    }
    let placeholders: Vec<String> = (1..=workspace_ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT w.id, w.name, w.slug, w.logo, w.logo_asset_id FROM workspaces w \
         WHERE w.id IN ({})",
        placeholders.join(", ")
    );
    let mut query = sqlx::query_as::<_, WorkspaceLiteRow>(&sql);
    for id in workspace_ids {
        query = query.bind(*id);
    }
    for row in query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?
    {
        out.insert(row.id, row);
    }
    Ok(out)
}

/// Fetch `asset_url` values for a set of asset ids, reusing the merged
/// fetcher rather than forking it.
async fn fetch_assets(
    pool: &PgPool,
    asset_ids: &[uuid::Uuid],
) -> Result<HashMap<uuid::Uuid, String>, Denial> {
    crate::v1_projects::handlers_members::fetch_asset_urls(pool, asset_ids)
        .await
        .map_err(|_| Denial::ServerError)
}

// ---------------------------------------------------------------------------
// PATCH validation (DRF `ModelSerializer` field ports, probed live)
// ---------------------------------------------------------------------------

/// Validated PATCH assignments: `None` per field means "key absent".
#[derive(Debug, Default, PartialEq)]
struct InvitePatch {
    role: Option<i16>,
    accepted: Option<bool>,
    deleted_at: Option<Option<DateTime<Utc>>>,
    created_by: Option<Option<uuid::Uuid>>,
    // No `updated_by`: the body value is validated (an invalid one
    // still 400s) but `BaseModel.save()` overwrites it with the caller
    // (`db/models/base.py:42`), so it is discarded after validation.
}

/// `ChoiceField(choices=[20, 15, 5])` (`role`, `workspace.py:241`):
/// `str(data)` must hit a choice, else `"<input>" is not a valid
/// choice.` with the Python-`str()` display. `None` is `may not be
/// null` (validated before `to_internal_value`, probed live).
fn validate_patch_role(value: &Value) -> Result<i16, String> {
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    // Python `str()` of the JSON scalar for the choice lookup.
    let display = match value {
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(n) => py_num_str(n),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => py_repr(value),
        Value::Null => unreachable!("null returned above"),
    };
    match display.as_str() {
        "20" => Ok(20),
        "15" => Ok(15),
        "5" => Ok(5),
        _ => Err(format!("\"{display}\" is not a valid choice.")),
    }
}

/// DRF `BooleanField` (`accepted`): the `TRUE_VALUES` / `FALSE_VALUES`
/// sets (probed live — `1.0` is true and `0.0` is false by numeric
/// equality with `1`/`0`; `2`, `2.0` and `""` are invalid).
/// `None` is `may not be null`.
/// Exact float equality is the semantics (set membership), not an
/// approximation — hence the lint scope below.
#[allow(clippy::float_cmp, clippy::float_cmp_const)]
fn validate_patch_accepted(value: &Value) -> Result<bool, String> {
    const INVALID: &str = "Must be a valid boolean.";
    if value.is_null() {
        return Err("This field may not be null.".to_owned());
    }
    match value {
        Value::Bool(b) => Ok(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match i {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(INVALID.to_owned()),
                }
            } else if let Some(u) = n.as_u64() {
                match u {
                    1 => Ok(true),
                    0 => Ok(false),
                    _ => Err(INVALID.to_owned()),
                }
            } else if let Some(f) = n.as_f64() {
                // Set membership: `1.0 == 1` is in `TRUE_VALUES`,
                // `0.0 == 0` is in `FALSE_VALUES`, the rest are out.
                if f == 1.0 {
                    Ok(true)
                } else if f == 0.0 {
                    Ok(false)
                } else {
                    Err(INVALID.to_owned())
                }
            } else {
                Err(INVALID.to_owned())
            }
        }
        Value::String(s) => match s.as_str() {
            "t" | "T" | "y" | "Y" | "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on"
            | "On" | "ON" | "1" => Ok(true),
            "f" | "F" | "n" | "N" | "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off"
            | "Off" | "OFF" | "0" => Ok(false),
            _ => Err(INVALID.to_owned()),
        },
        Value::Array(_) | Value::Object(_) => Err(INVALID.to_owned()),
        Value::Null => unreachable!("null returned above"),
    }
}

/// DRF `DateTimeField` wrong-format message (probed live).
const DATETIME_INVALID_MESSAGE: &str =
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].";

/// A parsed `deleted_at` input: aware values carry their instant, naive
/// values the wall time (DRF then attaches the request timezone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedDt {
    Aware(DateTime<Utc>),
    Naive(chrono::NaiveDateTime),
}

/// Parse exactly what Django's `parse_datetime` accepts
/// (`django/utils/dateparse.py`, probed live against Django 4.2.30 /
/// CPython 3.12): `datetime.fromisoformat` first, then the regex
/// fallback — the union of both arms. Anything else is `None` (the
/// caller's wrong-format 400; a `ValueError` from either arm lands on
/// the same 400 through DRF's `contextlib.suppress`, so `None` covers
/// both outcomes).
fn parse_django_datetime(text: &str) -> Option<ParsedDt> {
    parse_iso_datetime(text).or_else(|| parse_regex_datetime(text))
}

/// The `fromisoformat` arm (CPython 3.12): calendar dates (`YYYY-MM-DD`,
/// `YYYYMMDD`) and ISO week dates (`YYYY-Www[-d]`, `YYYYWww[d]`, weekday
/// default 1), an optional time after any single non-digit separator
/// (exactly-two-digit parts, seconds optional, any-length fraction
/// truncated to 6), and an optional `Z`/numeric offset (total strictly
/// under 24h). Unpadded dates, whitespace gaps and the trailing-newline
/// quirk belong to the regex arm below.
fn parse_iso_datetime(text: &str) -> Option<ParsedDt> {
    if text.is_empty() || py_strip(text).len() != text.len() {
        return None;
    }
    let bytes = text.as_bytes();
    // Date part: 4-digit year first in every accepted shape.
    if bytes.len() < 4 || !bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = text[..4].parse().ok()?;
    let (date, rest) = parse_iso_date(text, year)?;
    if rest.is_empty() {
        // Date-only renders midnight.
        return Some(ParsedDt::Naive(
            date.and_hms_opt(0, 0, 0).expect("midnight valid"),
        ));
    }
    // Any single non-digit separator (`T`, `t`, `X`, space, tab, even
    // `+`/`-` — the offset only starts after the time part).
    let rest = match rest.strip_prefix('T') {
        Some(tail) => tail,
        None => {
            let mut chars = rest.chars();
            let sep = chars.next()?;
            if sep.is_ascii_digit() {
                return None;
            }
            chars.as_str()
        }
    };
    if rest.is_empty() {
        return None;
    }
    parse_iso_time(date, rest)
}

/// Parse the date head, returning the day and the unparsed tail.
fn parse_iso_date(text: &str, year: i32) -> Option<(chrono::NaiveDate, &str)> {
    // chrono accepts the proleptic year 0; CPython raises (`ValueError:
    // year 0 is out of range`), so Django rejects — match it.
    if !(1..=9999).contains(&year) {
        return None;
    }
    let bytes = text.as_bytes();
    // Week dates contain an uppercase `W` (`2024-W03[-1]`, `2024W03[1]`).
    // A `-` after day-less basic `YYYYWww` is the time separator, not a
    // rejected dash-day: it falls through to the Monday-default arm below
    // and the caller's generic separator logic (`2030W23-12:00:00` parses;
    // `2030W23-1` still rejects on its 1-char time).
    if bytes.len() > 4 && bytes[4] == b'W' {
        let tail = &text[5..];
        if tail.len() < 2 || !tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let week: u32 = tail[..2].parse().ok()?;
        let (weekday, rest) = match tail.as_bytes().get(2) {
            Some(digit) if digit.is_ascii_digit() => (tail[2..3].parse().ok()?, &tail[3..]),
            _ => (1, &tail[2..]),
        };
        if !(1..=7).contains(&weekday) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday_as_monday0(weekday))?;
        return Some((date, rest));
    }
    if bytes.len() > 4 && bytes[4] == b'-' {
        // `YYYY-Www` extended week form.
        if bytes.get(5) == Some(&b'W') {
            let tail = &text[6..];
            if tail.len() < 2 || !tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let week: u32 = tail[..2].parse().ok()?;
            let (weekday, rest) = match tail[2..].strip_prefix('-') {
                Some(day) => {
                    let digit = day.as_bytes().first()?;
                    if !digit.is_ascii_digit() {
                        return None;
                    }
                    (day[..1].parse().ok()?, &day[1..])
                }
                None => (1, &tail[2..]),
            };
            if !(1..=7).contains(&weekday) {
                return None;
            }
            let date = chrono::NaiveDate::from_isoywd_opt(year, week, weekday_as_monday0(weekday))?;
            return Some((date, rest));
        }
        // `YYYY-MM-DD`, strictly zero-padded (byte-checked before
        // slicing so non-ASCII input rejects instead of panicking).
        if bytes.len() < 10 || bytes[7] != b'-' {
            return None;
        }
        if !bytes[5..7].iter().all(|b| b.is_ascii_digit())
            || !bytes[8..10].iter().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let (month, day): (u32, u32) = (text[5..7].parse().ok()?, text[8..10].parse().ok()?);
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, &text[10..]));
    }
    // Basic `YYYYMMDD`.
    if bytes.len() < 8 || !bytes[4..8].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (month, day): (u32, u32) = (text[4..6].parse().ok()?, text[6..8].parse().ok()?);
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    Some((date, &text[8..]))
}

fn weekday_as_monday0(weekday: u32) -> chrono::Weekday {
    match weekday {
        1 => chrono::Weekday::Mon,
        2 => chrono::Weekday::Tue,
        3 => chrono::Weekday::Wed,
        4 => chrono::Weekday::Thu,
        5 => chrono::Weekday::Fri,
        6 => chrono::Weekday::Sat,
        _ => chrono::Weekday::Sun,
    }
}

/// Parse the time tail (time + optional fraction + optional offset).
/// `fromisoformat` takes exactly-two-digit parts (`HH[:MM[:SS]]` or
/// `HHMM[SS]`; probed: `T3`, `T03:4` and `T03:04:5` all fail on padded
/// and basic dates alike — single-digit parts belong to the regex
/// arm). The fraction (`.`/`,` anywhere, `:` only after extended
/// seconds — `T030405:06` fails) is always a seconds fraction, even on
/// the hour or minute (`T10.5` is 10:00:00.5, probed). In basic form a
/// digit run past the pairs is the fraction too, but needs at least 2
/// digits (`T0304050` fails, `T03040500` parses). An empty fraction is
/// only valid ahead of a tz (`05.+tz` parses, `05.` does not — probed).
fn parse_iso_time(date: chrono::NaiveDate, rest: &str) -> Option<ParsedDt> {
    let bytes = rest.as_bytes();
    if bytes.len() < 2 || !bytes[..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: u32 = rest[..2].parse().ok()?;
    if hour > 23 {
        return None;
    }
    let rest = &rest[2..];
    let (minute, second, rest, basic) = if let Some(tail) = rest.strip_prefix(':') {
        let tb = tail.as_bytes();
        if tb.len() < 2 || !tb[..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let minute: u32 = tail[..2].parse().ok()?;
        let tail = &tail[2..];
        if let Some(tail) = tail.strip_prefix(':') {
            let tb = tail.as_bytes();
            if tb.len() < 2 || !tb[..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let second: u32 = tail[..2].parse().ok()?;
            (minute, second, &tail[2..], false)
        } else {
            (minute, 0, tail, false)
        }
    } else if rest.len() >= 2 && rest.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
        // Basic `HHMM[SS]` (no colon after the hour).
        let minute: u32 = rest[..2].parse().ok()?;
        let tail = &rest[2..];
        if tail.len() >= 2 && tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
            let second: u32 = tail[..2].parse().ok()?;
            (minute, second, &tail[2..], true)
        } else {
            (minute, 0, tail, true)
        }
    } else {
        (0, 0, rest, true)
    };
    if minute > 59 || second > 59 {
        return None;
    }
    // A digit run past basic pairs is the fraction (`T03040500`);
    // extended parts never take one (`T03:04:056` fails).
    let (micros, rest, frac_consumed) = if basic {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            (0, rest, false)
        } else {
            if digits.len() < 2 {
                return None;
            }
            (frac_micros(&digits), &rest[digits.len()..], true)
        }
    } else {
        (0, rest, false)
    };
    // A separated fraction — `.`/`,` anywhere, `:` only after extended
    // seconds — and never after a digit fraction (`T03040500.5` fails).
    let (micros, rest) = if frac_consumed {
        (micros, rest)
    } else if let Some(frac) = rest.strip_prefix(['.', ',']) {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() && frac.is_empty() {
            return None;
        }
        (frac_micros(&digits), &frac[digits.len()..])
    } else if !basic && rest.starts_with(':') {
        // `:` after extended seconds is the fraction (`T03:04:05:06`).
        // (After extended minutes-without-seconds the rest never starts
        // with `:` — a colon there was already consumed as seconds — so
        // this branch implies seconds were parsed.)
        let frac = &rest[1..];
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() && frac.is_empty() {
            return None;
        }
        (frac_micros(&digits), &frac[digits.len()..])
    } else {
        (micros, rest)
    };
    let naive = date.and_hms_micro_opt(hour, minute, second, micros)?;
    if rest.is_empty() {
        return Some(ParsedDt::Naive(naive));
    }
    let offset = parse_iso_offset(rest)?;
    Some(ParsedDt::Aware(naive.and_utc() - offset))
}

/// A seconds fraction past its separator: any digit length, truncated
/// to 6, right-padded to microseconds (`:1` is 100000us, `:1234567` is
/// 123456us — probed). Empty (only valid ahead of a tz in the time
/// part) is zero.
fn frac_micros(digits: &str) -> u32 {
    let mut buf = digits.to_owned();
    buf.truncate(6);
    while buf.len() < 6 {
        buf.push('0');
    }
    buf.parse().unwrap_or(0)
}

/// `Z` (uppercase only) or a numeric offset. The fraction — after `.`,
/// `,` or `:` in extended form (`+05:00:00:01` is +5h + 0.01s), after
/// `.`/`,` in basic form (`+0500.5`), or as extra basic digits past
/// `HHMMSS` (`+0500001234`; at least 2 — `+0500001` fails) — is always
/// a seconds fraction (`+05.5` is 5h + 0.5s, probed), never a second
/// fraction-twice (`+05000000.5` fails). Parts are unchecked (`+00:61`
/// is +01:01); only the total must stay strictly under 24h (CPython
/// raises past it, which DRF suppresses into the wrong-format 400).
fn parse_iso_offset(text: &str) -> Option<chrono::Duration> {
    if text == "Z" {
        return Some(chrono::Duration::seconds(0));
    }
    let bytes = text.as_bytes();
    let sign: i64 = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = &text[1..];
    let db = digits.as_bytes();
    if db.len() < 2 || !db[..2].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let tail = &digits[2..];
    let (minutes, seconds, micros) = if let Some(tail) = tail.strip_prefix(':') {
        // Extended: every present part is exactly two digits.
        let tb = tail.as_bytes();
        if tb.len() < 2 || !tb[..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let minutes: i64 = tail[..2].parse().ok()?;
        let tail = &tail[2..];
        if let Some(tail) = tail.strip_prefix(':') {
            let tb = tail.as_bytes();
            if tb.len() < 2 || !tb[..2].iter().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let seconds: i64 = tail[..2].parse().ok()?;
            let tail = &tail[2..];
            let (micros, tail) = parse_offset_frac(tail, true)?;
            if !tail.is_empty() {
                return None;
            }
            (minutes, seconds, micros)
        } else {
            let (micros, tail) = parse_offset_frac(tail, false)?;
            if !tail.is_empty() {
                return None;
            }
            (minutes, 0, micros)
        }
    } else if tail.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        // Basic `HHMM[SS][fraction-digits]`: greedy pairs first.
        let tb = tail.as_bytes();
        if tb.len() < 2 || !tb[..2].iter().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let minutes: i64 = tail[..2].parse().ok()?;
        let tail = &tail[2..];
        let (seconds, tail) =
            if tail.len() >= 2 && tail.as_bytes()[..2].iter().all(|b| b.is_ascii_digit()) {
                (tail[..2].parse().ok()?, &tail[2..])
            } else {
                (0, tail)
            };
        let run: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if run.is_empty() {
            let (micros, tail) = parse_offset_frac(tail, false)?;
            if !tail.is_empty() {
                return None;
            }
            (minutes, seconds, micros)
        } else {
            // Digit fraction: at least 2 digits, then end (a second
            // fraction after it fails).
            if run.len() < 2 {
                return None;
            }
            let tail = &tail[run.len()..];
            if !tail.is_empty() {
                return None;
            }
            (minutes, seconds, frac_micros(&run))
        }
    } else {
        // Hours only, optionally with a `.`/`,` fraction (`+05.5`).
        let (micros, tail) = parse_offset_frac(tail, false)?;
        if !tail.is_empty() {
            return None;
        }
        (0, 0, micros)
    };
    let total_micros =
        sign * ((hours * 3600 + minutes * 60 + seconds) * 1_000_000 + i64::from(micros));
    if total_micros.abs() >= 86_400_000_000 {
        return None;
    }
    Some(chrono::Duration::microseconds(total_micros))
}

/// The optional offset fraction: `.`/`,` anywhere, `:` only after
/// extended seconds. Absent is zero; the offset is always last, so a
/// separator with an empty fraction fails (`+05:00:00.` does not parse
/// — probed).
fn parse_offset_frac(tail: &str, colon: bool) -> Option<(u32, &str)> {
    let frac =
        tail.strip_prefix(['.', ','])
            .or_else(|| if colon { tail.strip_prefix(':') } else { None });
    let Some(frac) = frac else {
        return Some((0, tail));
    };
    let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    Some((frac_micros(&digits), &frac[digits.len()..]))
}

/// The regex fallback arm (`datetime_re` in `django/utils/dateparse.py`,
/// read from the Django 4.2.30 source and probed): unpadded extended
/// dates (`\d{4}-\d{1,2}-\d{1,2}`), `T`/space separator, unpadded time
/// with optional seconds and a 1-12 digit fraction (first 6 kept),
/// `\s*` gaps before the tz or end, and a `Z`/`±HH`/`±HHMM`/`±HH:MM`
/// tz whose total stays strictly under 24h (parts unchecked —
/// `+00:61` is +01:01). Python's `$` also matches just before one
/// trailing newline (`Z\n` parses, `Z ` and `Z\n\n` do not — probed).
fn parse_regex_datetime(text: &str) -> Option<ParsedDt> {
    // The `$`-before-final-newline quirk: strip at most one trailing
    // `\n` up front, then require an exact end below.
    let text = text.strip_suffix('\n').unwrap_or(text);
    let (date, time) = text.split_once(['T', ' '])?;
    let mut date_parts = date.split('-');
    let (year, month, day) = (date_parts.next()?, date_parts.next()?, date_parts.next()?);
    if date_parts.next().is_some() {
        return None;
    }
    if !(year.len() == 4 && (1..=2).contains(&month.len()) && (1..=2).contains(&day.len())) {
        return None;
    }
    if !year.bytes().all(|b| b.is_ascii_digit())
        || !month.bytes().all(|b| b.is_ascii_digit())
        || !day.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let year_num: i32 = year.parse().ok()?;
    // Same year-0 clamp as the fromisoformat arm: `datetime(0, ...)`
    // raises, which lands in the invalid arm.
    if !(1..=9999).contains(&year_num) {
        return None;
    }
    // Split the tz suffix first (`Z` or `±HH[[:]MM]` at the end;
    // uppercase `Z` only). The `\s*` gap sits between the clock and
    // the tz — never after it — so the clock trims but the zone must
    // match exactly.
    let none_offset = chrono::Duration::seconds(0);
    let (clock, offset, aware) = if let Some(clock) = time.strip_suffix('Z') {
        if clock.contains(['+', '-']) {
            return None;
        }
        (clock, none_offset, true)
    } else if let Some(pos) = time.rfind(['+', '-']) {
        // A sign past position 0 starts the tz (the clock itself has
        // no signs; `rfind` on the whole time part is safe because
        // date and time split above).
        let (clock, zone) = time.split_at(pos);
        let sign: i64 = if zone.starts_with('-') { -1 } else { 1 };
        let tail = &zone[1..];
        let digits: String = match tail.len() {
            2 | 4 => tail.to_owned(),
            5 if tail.as_bytes()[2] == b':' => {
                format!("{}{}", &tail[..2], &tail[3..])
            }
            _ => return None,
        };
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hours: i64 = digits[..2].parse().ok()?;
        let mins: i64 = if digits.len() == 4 {
            digits[2..].parse().ok()?
        } else {
            0
        };
        // No per-part range check (`get_fixed_timezone` only bounds
        // the total, in minutes, strictly under a day).
        let total_mins = sign * (hours * 60 + mins);
        if total_mins.abs() >= 1440 {
            return None;
        }
        (clock, chrono::Duration::minutes(total_mins), true)
    } else {
        (time, none_offset, false)
    };
    let clock = clock.trim_end_matches(|c: char| c.is_whitespace());
    let mut clock_parts = clock.split(':');
    let (hour, minute) = (clock_parts.next()?, clock_parts.next()?);
    let second_raw = clock_parts.next().unwrap_or("00");
    if clock_parts.next().is_some() {
        return None;
    }
    if !((1..=2).contains(&hour.len()) && (1..=2).contains(&minute.len())) {
        return None;
    }
    if !hour.bytes().all(|b| b.is_ascii_digit()) || !minute.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (second, micros) = match second_raw.split_once(['.', ',']) {
        Some((sec, frac)) => {
            // `(\d{1,6})\d{0,6}`: 1-12 digits, first 6 kept.
            if !(1..=2).contains(&sec.len())
                || frac.is_empty()
                || frac.len() > 12
                || !frac.bytes().all(|b| b.is_ascii_digit())
                || !sec.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            (sec, frac_micros(frac))
        }
        None => {
            if !(1..=2).contains(&second_raw.len())
                || !second_raw.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            (second_raw, 0)
        }
    };
    let (hour, minute, second): (u32, u32, u32) = (
        hour.parse().ok()?,
        minute.parse().ok()?,
        second.parse().ok()?,
    );
    let naive = chrono::NaiveDate::from_ymd_opt(year_num, month.parse().ok()?, day.parse().ok()?)?
        .and_hms_micro_opt(hour, minute, second, micros)?;
    if aware {
        // `Z` and zero offsets (`+00`, `-00:00`) are aware UTC, not
        // naive: under a non-UTC request tz the instants differ.
        Some(ParsedDt::Aware(naive.and_utc() - offset))
    } else {
        Some(ParsedDt::Naive(naive))
    }
}

/// Python `datetime` range (`0001-01-01` through `9999-12-31`): DRF's
/// `astimezone` raises past it, on either representation (the UTC-side
/// subtraction or the request-tz wall). `chrono_tz` offset lookups are
/// infallible (they extrapolate past the transition tables), so an
/// out-of-range wall date is the only overflow signal.
fn python_range_contains(date: &chrono::NaiveDate) -> bool {
    *date >= chrono::NaiveDate::from_ymd_opt(1, 1, 1).expect("min date")
        && *date <= chrono::NaiveDate::from_ymd_opt(9999, 12, 31).expect("max date")
}

/// DRF `enforce_timezone` for naive input: attach the request timezone
/// (`make_aware`, fold 0; a DST-gap wall time takes the pre-transition
/// offset, like zoneinfo's non-raising attach — probed: NY `02:30` in
/// the spring gap attaches `-05:00`).
fn attach_request_tz(naive: &chrono::NaiveDateTime, timezone: &chrono_tz::Tz) -> DateTime<Utc> {
    use chrono::{MappedLocalTime, TimeZone};
    match timezone.from_local_datetime(naive) {
        MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
            local.with_timezone(&Utc)
        }
        MappedLocalTime::None => {
            // DST gap: step back to the wall time just before it and
            // attach that offset (`shift` is the negated UTC offset, so
            // the instant is `naive + shift`). Day-scale gaps (the
            // Apia/Kwajalein skipped days) need more than one step back;
            // the walk is bounded and the subtraction checked so a
            // year-min wall can never panic.
            let mut probe = *naive;
            for _ in 0..72 {
                let Some(back) = probe.checked_sub_signed(chrono::Duration::hours(1)) else {
                    break;
                };
                probe = back;
                match timezone.from_local_datetime(&probe) {
                    MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
                        let shift = local.timestamp() - probe.and_utc().timestamp();
                        return naive.and_utc() + chrono::Duration::seconds(shift);
                    }
                    MappedLocalTime::None => {}
                }
            }
            naive.and_utc()
        }
    }
}

/// DRF `DateTimeField` `overflow` message (probed live).
const DATETIME_OVERFLOW_MESSAGE: &str = "Datetime value out of range.";

/// A `deleted_at` validation failure: the wrong-format/`overflow` 400
/// message, or the raw-`OverflowError` 500 (naive wall whose UTC
/// conversion leaves Python's range — re-raised past DRF's `except`,
/// probed live).
#[derive(Debug, PartialEq)]
enum PatchDtError {
    Invalid(String),
    ServerError,
}

/// `deleted_at` (`DateTimeField(null=True)` → `allow_null`): `None`
/// clears; strings parse through [`parse_django_datetime`] with naive
/// inputs attaching the request timezone (`TimezoneMixin.initial`
/// activates it, `enforce_timezone` → `make_aware`); an aware instant
/// outside Python's range is the `overflow` 400; anything else is the
/// wrong-format 400 (all probed live against Django 4.2.30 / CPython
/// 3.12).
fn validate_patch_deleted_at(
    value: &Value,
    timezone: &chrono_tz::Tz,
) -> Result<Option<DateTime<Utc>>, PatchDtError> {
    if value.is_null() {
        return Ok(None);
    }
    let Value::String(s) = value else {
        return Err(PatchDtError::Invalid(DATETIME_INVALID_MESSAGE.to_owned()));
    };
    match parse_django_datetime(s) {
        Some(ParsedDt::Aware(utc)) => {
            let wall = utc.with_timezone(timezone).date_naive();
            if python_range_contains(&utc.date_naive()) && python_range_contains(&wall) {
                Ok(Some(utc))
            } else {
                Err(PatchDtError::Invalid(DATETIME_OVERFLOW_MESSAGE.to_owned()))
            }
        }
        Some(ParsedDt::Naive(naive)) => {
            let utc = attach_request_tz(&naive, timezone);
            if python_range_contains(&utc.date_naive()) {
                Ok(Some(utc))
            } else {
                Err(PatchDtError::ServerError)
            }
        }
        None => Err(PatchDtError::Invalid(DATETIME_INVALID_MESSAGE.to_owned())),
    }
}

/// Python `uuid.UUID(hex=...)` (CPython 3.12 `uuid.py`, probed live):
/// case-sensitive global `urn:`/`uuid:` strip, `{}` strip on both ends,
/// ALL hyphens removed anywhere (misplaced and doubled hyphens parse),
/// 32 chars required — then `int(_, 16)` (leading/trailing int-ws
/// stripped, one sign, single underscores between digits; negative is
/// out of range). Non-ASCII hex is a documented approximation
/// (ASCII-only here, mirroring [`py_int`]).
fn parse_uuid_hex(raw: &str) -> Option<uuid::Uuid> {
    let stripped = raw.replace("urn:", "").replace("uuid:", "");
    let stripped = stripped.trim_matches(|c| c == '{' || c == '}');
    let compact: String = stripped.chars().filter(|c| *c != '-').collect();
    if compact.chars().count() != 32 {
        return None;
    }
    let text = py_int_strip(&compact);
    // One `+` sign (`int("+..", 16)` works; `-` cannot survive the
    // hyphen strip above, and a negative int is out of range anyway).
    let text = text.strip_prefix('+').unwrap_or(text);
    // Single underscores only between digits (`int("2_0", 16)` works).
    let mut cleaned = String::with_capacity(text.len());
    let mut prev_underscore = true;
    for c in text.chars() {
        if c == '_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
        } else if c.is_ascii_hexdigit() {
            cleaned.push(c);
            prev_underscore = false;
        } else {
            return None;
        }
    }
    if prev_underscore {
        return None;
    }
    u128::from_str_radix(&cleaned, 16)
        .ok()
        .map(uuid::Uuid::from_u128)
}

/// `PrimaryKeyRelatedField(queryset=User, allow_null=True)` (`created_by` /
/// `updated_by`, probed against DRF 3.15.2 + Django 4.2.30): `None`
/// clears; bools are `incorrect_type`; ints ride `UUID(int=)` (any
/// `0 <= i < 2**128`; out of range → the smart-quote invalid); strings
/// ride `UUID(hex=)` ([`parse_uuid_hex`]; unparseable → the smart-quote
/// invalid); floats/arrays/objects are the smart-quote invalid;
/// well-formed but unknown UUIDs are
/// `Invalid pk "<input>" - object does not exist.` with the ORIGINAL
/// input display.
async fn validate_patch_user(pool: &PgPool, value: &Value) -> Result<Option<uuid::Uuid>, String> {
    if value.is_null() {
        return Ok(None);
    }
    // Django `UUIDField.to_python` (`fields/__init__.py:2684-2695`).
    let parsed: uuid::Uuid = match value {
        Value::Bool(_) => {
            let kind = "bool";
            return Err(format!(
                "Incorrect type. Expected pk value, received {kind}."
            ));
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i < 0 {
                    return Err(format!("“{i}” is not a valid UUID."));
                }
                uuid::Uuid::from_u128(i as u128)
            } else if let Some(u) = n.as_u64() {
                uuid::Uuid::from_u128(u as u128)
            } else if let Ok(wide) = n.to_string().parse::<u128>() {
                // `uuid.UUID(int=)` takes any `0 <= i < 2**128`
                // (probed live); the raw literal is exact under
                // `arbitrary_precision`.
                uuid::Uuid::from_u128(wide)
            } else {
                return Err(format!("“{}” is not a valid UUID.", py_num_str(n)));
            }
        }
        Value::String(s) => match parse_uuid_hex(s) {
            Some(id) => id,
            None => return Err(format!("“{s}” is not a valid UUID.")),
        },
        Value::Array(_) | Value::Object(_) => {
            return Err(format!("“{}” is not a valid UUID.", py_repr(value)));
        }
        Value::Null => unreachable!("null returned above"),
    };
    // `queryset.get(pk=...)`: `users` has no soft-delete scope.
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
        .bind(parsed)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if !known {
        // `pk_value=data`: the ORIGINAL input, not the normalized UUID.
        let display = match value {
            Value::String(s) => s.clone(),
            // Only ints reach here (parsed OK above); `str(int)`.
            Value::Number(n) => py_num_str(n),
            _ => parsed.to_string(),
        };
        return Err(format!("Invalid pk \"{display}\" - object does not exist."));
    }
    Ok(Some(parsed))
}

/// `type(data).__name__` for a JSON number (DRF `serializers.py:485`):
/// Python parses digit-only literals as `int` at any width, so the kind
/// comes from the literal syntax, not the range — `is_i64()/is_u64()`
/// would misreport `2**64` as `float` (probed live on CPython 3.12).
fn json_number_kind(n: &serde_json::Number) -> &'static str {
    if n.to_string().contains(['.', 'e', 'E']) {
        "float"
    } else {
        "int"
    }
}

/// Validate a PATCH body for an invite: non-dict bodies fail with the
/// `non_field_errors` shape, unknown and read-only keys are ignored, and
/// field errors collect in serializer-field order (`deleted_at`,
/// `accepted`, `role`, `created_by`, `updated_by` — the
/// [`ser_invite::INVITE_WIRE_FIELDS`] order restricted to the writable
/// remainder, probed live).
async fn validate_invite_patch(
    pool: &PgPool,
    body: &Value,
    timezone: &chrono_tz::Tz,
) -> Result<InvitePatch, Denial> {
    let data = match body {
        Value::Object(map) => map,
        Value::Null => {
            let mut errors = Map::new();
            errors.insert(
                "non_field_errors".to_owned(),
                Value::Array(vec![Value::String("No data provided".to_owned())]),
            );
            return Err(Denial::BadJson(Value::Object(errors)));
        }
        Value::Array(_) => return Err(Denial::BadJson(non_dict_errors("list"))),
        Value::String(_) => return Err(Denial::BadJson(non_dict_errors("str"))),
        Value::Bool(_) => return Err(Denial::BadJson(non_dict_errors("bool"))),
        Value::Number(n) => {
            return Err(Denial::BadJson(non_dict_errors(json_number_kind(n))));
        }
    };
    let mut errors = Map::new();
    let mut patch = InvitePatch::default();
    if let Some(value) = data.get("deleted_at") {
        match validate_patch_deleted_at(value, timezone) {
            Ok(dt) => patch.deleted_at = Some(dt),
            Err(PatchDtError::Invalid(message)) => {
                errors.insert(
                    "deleted_at".to_owned(),
                    Value::Array(vec![Value::String(message)]),
                );
            }
            // The naive-wall 500 short-circuits the whole body (a raw
            // `OverflowError`, not a field error — probed live).
            Err(PatchDtError::ServerError) => return Err(Denial::ServerError),
        }
    }
    if let Some(value) = data.get("accepted") {
        match validate_patch_accepted(value) {
            Ok(accepted) => patch.accepted = Some(accepted),
            Err(message) => {
                errors.insert(
                    "accepted".to_owned(),
                    Value::Array(vec![Value::String(message)]),
                );
            }
        }
    }
    if let Some(value) = data.get("role") {
        match validate_patch_role(value) {
            Ok(role) => patch.role = Some(role),
            Err(message) => {
                errors.insert(
                    "role".to_owned(),
                    Value::Array(vec![Value::String(message)]),
                );
            }
        }
    }
    if let Some(value) = data.get("created_by") {
        match validate_patch_user(pool, value).await {
            Ok(user) => patch.created_by = Some(user),
            Err(message) => {
                errors.insert(
                    "created_by".to_owned(),
                    Value::Array(vec![Value::String(message)]),
                );
            }
        }
    }
    if let Some(value) = data.get("updated_by") {
        // Validated for the 400 shape, then discarded: the save stamps
        // the caller over it (`db/models/base.py:42`).
        if let Err(message) = validate_patch_user(pool, value).await {
            errors.insert(
                "updated_by".to_owned(),
                Value::Array(vec![Value::String(message)]),
            );
        }
    }
    if errors.is_empty() {
        Ok(patch)
    } else {
        Err(Denial::BadJson(Value::Object(errors)))
    }
}

fn non_dict_errors(kind: &str) -> Value {
    let mut errors = Map::new();
    errors.insert(
        "non_field_errors".to_owned(),
        Value::Array(vec![Value::String(format!(
            "Invalid data. Expected a dictionary, but got {kind}."
        ))]),
    );
    Value::Object(errors)
}

// ---------------------------------------------------------------------------
// Unit 1: `WorkspaceInvitationsViewset` (`invite.py:37-148`)
// ---------------------------------------------------------------------------

/// Fetch one invite's row by pk + workspace (`invite.py:145,164,239`).
/// The `workspace__slug` traversal is unscoped on the workspace side
/// (see [`resolve_class_admin`]).
async fn fetch_invite(
    pool: &PgPool,
    pk: &uuid::Uuid,
    slug: &str,
) -> Result<Option<InviteRow>, Denial> {
    sqlx::query_as::<_, InviteRow>(FETCH_INVITE_SQL)
        .bind(pk)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `GET /api/workspaces/<slug>/invitations/` (`invite.py:45-51`): the
/// inherited `list` over the slug scope, `-created_at` first, as a plain
/// array (no pagination class is configured).
async fn invite_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, gate) = match resolve_class_admin(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let rows: Vec<InviteRow> = match sqlx::query_as::<_, InviteRow>(
        "SELECT i.id, i.workspace_id, i.created_at, i.updated_at, i.deleted_at, i.email, \
         i.accepted, i.token, i.message, i.responded_at, i.role, i.created_by_id, i.updated_by_id \
         FROM workspace_member_invites i \
         WHERE i.workspace_id = $1 AND i.deleted_at IS NULL \
         ORDER BY i.created_at DESC",
    )
    .bind(gate.workspace_id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let workspaces = match fetch_workspace_lites(pool, &[gate.workspace_id]).await {
        Ok(workspaces) => workspaces,
        Err(denial) => return denial.into_response(),
    };
    let Some(workspace_row) = workspaces.get(&gate.workspace_id) else {
        return Denial::ServerError.into_response();
    };
    let asset_ids: Vec<uuid::Uuid> = workspace_row.logo_asset_id.into_iter().collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let workspace = render_workspace_lite(workspace_row, &assets);
    let rendered: Vec<RenderedInvite> = rows
        .iter()
        .map(|row| render_invite(row, &workspace, &actor.timezone))
        .collect();
    let views: Vec<Value> = rendered.iter().map(RenderedInvite::view_value).collect();
    (StatusCode::OK, Json(views)).into_response()
}

/// One validated invite-create row: the stored address, role and token.
struct PendingInvite {
    email: String,
    role: i16,
    token: String,
}

/// How the `emails` input classifies (`invite.py:54-56`): missing,
/// null, or falsy input 400s (`if not emails`); a truthy non-list
/// iterates to an `AttributeError`/`TypeError` (500) — only a
/// non-empty list reaches the role loop.
enum EmailsInput<'a> {
    /// A non-empty list: validate each entry.
    Entries(&'a Vec<Value>),
    /// Missing/null/falsy: the emails-required 400.
    Missing,
    /// Truthy non-list: 500.
    ServerError,
}

fn classify_emails_input(value: Option<&Value>) -> EmailsInput<'_> {
    match value {
        Some(Value::Array(entries)) if !entries.is_empty() => EmailsInput::Entries(entries),
        Some(value) if py_truthy(value) => EmailsInput::ServerError,
        _ => EmailsInput::Missing,
    }
}

/// `POST /api/workspaces/<slug>/invitations/` (`invite.py:53-142`), in
/// Django's order: emails-required → requesting-user role → higher-role
/// cap → workspace → already-member → per-email validate →
/// bulk-create → per-row enqueues → 200.
async fn invite_create(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, gate) = match resolve_class_admin(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get("emails", [])` — a non-dict body has no `.get`
    // (`AttributeError` → 500).
    let data = match body.as_object() {
        Some(data) => data,
        None => return Denial::ServerError.into_response(),
    };
    // `if not emails` — missing, null, and falsy values 400; a
    // truthy non-list 500s in the iteration (`:63`).
    let entries: &Vec<Value> = match classify_emails_input(data.get("emails")) {
        EmailsInput::Entries(entries) => entries,
        EmailsInput::Missing => {
            return json_response(StatusCode::BAD_REQUEST, EMAILS_REQUIRED_BODY);
        }
        EmailsInput::ServerError => return Denial::ServerError.into_response(),
    };
    // The higher-role cap (`:63`): `int(email.get("role", 5))` per entry.
    // A non-dict entry has no `.get` (500); an uncoercible role is a
    // `TypeError`/`ValueError` (500).
    let default_role = Value::Number(5.into());
    let mut roles: Vec<i128> = Vec::with_capacity(entries.len());
    for entry in entries.iter() {
        let Some(item) = entry.as_object() else {
            return Denial::ServerError.into_response();
        };
        let role_value = item.get("role").unwrap_or(&default_role);
        match py_int(role_value) {
            Ok(role) => roles.push(role),
            Err(()) => return Denial::ServerError.into_response(),
        }
    }
    // [`queries::invite_role_cap_blocks`], widened to `i128` so huge
    // `int()` values compare instead of overflowing (equal roles pass).
    if roles.iter().any(|role| *role > i128::from(gate.role)) {
        return json_response(StatusCode::BAD_REQUEST, HIGHER_ROLE_BODY);
    }
    // The workspace (`:70`): a direct `Workspace.objects.get` (live-only
    // base manager — the scope stays). On a deleted workspace the
    // unscoped gate passed and this 404s late, exactly like Django.
    let workspace_id = match sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT w.id FROM workspaces w WHERE w.slug = $1 AND w.deleted_at IS NULL",
    )
    .bind(&slug)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(id)) => id,
        Ok(None) => return Denial::MissingObject.into_response(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    debug_assert_eq!(workspace_id, gate.workspace_id);
    // The already-member check (`:73-86`): `if queryset:` fetches ALL
    // matching rows (not `EXISTS`), rendered with
    // `WorkSpaceMemberSerializer(many=True)`.
    let addresses: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry.get("email"))
        .filter_map(Value::as_str)
        .collect();
    if !addresses.is_empty() {
        let placeholders: Vec<String> = (1..=addresses.len())
            .map(|i| format!("${}", i + 1))
            .collect();
        let sql = format!(
            "SELECT wm.id, wm.workspace_id, wm.member_id, wm.created_at, wm.updated_at, \
             wm.deleted_at, wm.role, wm.company_role, wm.view_props, wm.default_props, \
             wm.issue_props, wm.is_active, wm.getting_started_checklist, wm.tips, \
             wm.explored_features, wm.created_by_id, wm.updated_by_id \
             FROM workspace_members wm JOIN users u ON u.id = wm.member_id \
             WHERE wm.workspace_id = $1 AND u.email IN ({}) AND wm.is_active = TRUE \
             AND wm.deleted_at IS NULL ORDER BY wm.created_at DESC",
            placeholders.join(", ")
        );
        let mut query = sqlx::query_as::<_, MemberRow>(&sql).bind(workspace_id);
        for address in &addresses {
            query = query.bind(*address);
        }
        let members: Vec<MemberRow> = match query.fetch_all(pool).await {
            Ok(members) => members,
            Err(_) => return Denial::ServerError.into_response(),
        };
        if !members.is_empty() {
            return already_member_response(pool, &members, &actor.timezone).await;
        }
    }
    // Per-email validate + row build (`:88-111`): the FIRST invalid email
    // aborts (nothing is created — the bulk runs after the loop).
    let now = Utc::now();
    let secret = state.settings().secret_key.as_bytes().to_owned();
    let mut pending: Vec<PendingInvite> = Vec::with_capacity(entries.len());
    for entry in entries.iter() {
        let item = entry.as_object().expect("entries checked for dict above");
        let raw_email = item.get("email");
        let valid = match classify_email(raw_email) {
            EmailCheck::Invalid => false,
            EmailCheck::TypeError => return Denial::ServerError.into_response(),
            EmailCheck::Candidate(candidate) => is_valid_email_str(&candidate),
        };
        if !valid {
            let mut body = Map::new();
            body.insert(
                "error".to_owned(),
                Value::String(format!(
                    "Invalid email - {} provided a valid email address is required to send the invite",
                    py_repr(entry)
                )),
            );
            return (StatusCode::BAD_REQUEST, Json(Value::Object(body))).into_response();
        }
        let address = item
            .get("email")
            .and_then(Value::as_str)
            .expect("validated as a string above");
        // `varchar(255)` (`db/models/workspace.py:238`): `validate_email`
        // allows 320 chars, so overlong addresses die in the bulk with a
        // `DataError` (500, not `IntegrityError`).
        let stored = py_strip(address).to_lowercase();
        if stored.chars().count() > 255 {
            return Denial::ServerError.into_response();
        }
        let role = roles[pending.len()];
        // `smallint` (`PositiveSmallIntegerField`): out of range dies in
        // the bulk with a `DataError` (500). Negatives have no `CHECK`
        // behind the field validators (which `bulk_create` skips), so
        // they store — only the column range 500s.
        if !(i128::from(i16::MIN)..=i128::from(i16::MAX)).contains(&role) {
            return Denial::ServerError.into_response();
        }
        let token = match invite_token(&secret, entry, epoch_float(&now)) {
            Ok(token) => token,
            Err(denial) => return denial.into_response(),
        };
        if token.len() > 255 {
            return Denial::ServerError.into_response();
        }
        pending.push(PendingInvite {
            email: stored,
            role: role as i16,
            token,
        });
    }
    // `bulk_create(batch_size=10, ignore_conflicts=True)` (`:113-115`):
    // duplicate `(email, workspace)` rows are silently skipped while the
    // endpoint still answers success. `bulk_create` writes every model
    // field (`accepted=False`, `message`/`responded_at` NULL, no
    // `save()` audit stamp so `updated_by`/`deleted_at` NULL) and no
    // column carries a DB default — the literals below are required,
    // not cosmetic (PIDASHCONV-751: omitting `accepted` 400s).
    for batch in pending.chunks(queries::INVITE_BULK_BATCH_SIZE as usize) {
        let mut values: Vec<String> = Vec::with_capacity(batch.len());
        for index in 0..batch.len() {
            let base = index * 8;
            values.push(format!(
                "(${}, ${}, ${}, ${}, NULL, NULL, ${}, FALSE, ${}, ${}, NULL, NULL, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8
            ));
        }
        let sql = format!(
            "INSERT INTO workspace_member_invites \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
             email, accepted, workspace_id, token, message, responded_at, role) \
             VALUES {} ON CONFLICT DO NOTHING",
            values.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for invite in batch.iter() {
            query = query
                .bind(uuid::Uuid::new_v4())
                .bind(now)
                .bind(now)
                .bind(actor.id)
                .bind(&invite.email)
                .bind(workspace_id)
                .bind(&invite.token)
                .bind(invite.role);
        }
        if let Err(error) = query.execute(pool).await {
            if is_integrity_error(&error) {
                return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
            }
            return Denial::ServerError.into_response();
        }
    }
    // Per-row enqueues (`:117-140`): the invitation mail plus the track
    // event, for EVERY input row (conflict-skipped rows still enqueue —
    // their in-memory `email`/`token` are set regardless).
    let current_site = crate::v1_projects::handlers_project::app_origin(&state);
    let invited_at = django_str_now(&now);
    let inviter = actor.email.clone().unwrap_or_default();
    let workspace_slug = slug.clone();
    let workspace_id_text = workspace_id.to_string();
    let user_id_text = actor.id.to_string();
    for invite in pending.iter() {
        enqueue_invitation(
            pool,
            &tasks::workspace_invitation_emit(
                &invite.email,
                &workspace_id_text,
                &invite.token,
                &current_site,
                &inviter,
            ),
        )
        .await;
        enqueue_track(
            pool,
            &tasks::user_invited_event(
                &user_id_text,
                &workspace_id_text,
                &workspace_slug,
                i32::from(invite.role),
                &invited_at,
                &invite.email,
            ),
        )
        .await;
    }
    (StatusCode::OK, Json(message_envelope(INVITES_SENT_MESSAGE))).into_response()
}

/// The already-member 400 (`invite.py:79-86`): `error` plus the
/// `workspace_users` rendering, `-created_at` first.
async fn already_member_response(
    pool: &PgPool,
    members: &[MemberRow],
    tz: &chrono_tz::Tz,
) -> Response {
    let user_ids: Vec<uuid::Uuid> = members.iter().map(|row| row.member_id).collect();
    let users = match fetch_user_lites(pool, &user_ids).await {
        Ok(users) => users,
        Err(denial) => return denial.into_response(),
    };
    let asset_ids: Vec<uuid::Uuid> = users
        .values()
        .filter_map(|row| row.avatar_asset_id)
        .collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let mut rendered = Vec::with_capacity(members.len());
    for row in members {
        // The join guarantees the user row exists; a missing row is
        // unreachable (still 500 like the serializer's `RelatedObject`
        // lookup failing).
        let Some(user) = users.get(&row.member_id) else {
            return Denial::ServerError.into_response();
        };
        rendered.push(render_member_value(
            row,
            &render_user_lite(user, &assets),
            tz,
        ));
    }
    let mut body = Map::new();
    body.insert(
        "error".to_owned(),
        Value::String(ALREADY_MEMBER_ERROR.to_owned()),
    );
    body.insert("workspace_users".to_owned(), Value::Array(rendered));
    (StatusCode::BAD_REQUEST, Json(Value::Object(body))).into_response()
}

/// `GET /api/workspaces/<slug>/invitations/<pk>/` (`invite.py:37-51`):
/// the inherited `retrieve` (404 through DRF's `Http404`).
async fn invite_retrieve(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(id) = parse_pk(&pk) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, gate) = match resolve_class_admin(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_invite(pool, &id, &slug).await {
        Ok(Some(row)) => row,
        Ok(None) => return Denial::InviteNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    render_single_invite(pool, &row, &gate, &actor).await
}

/// Render one invite with its workspace lite (retrieve / PATCH / join-get).
async fn render_single_invite(
    pool: &PgPool,
    row: &InviteRow,
    gate: &AdminGate,
    actor: &crate::license::Actor,
) -> Response {
    let workspaces = match fetch_workspace_lites(pool, &[gate.workspace_id]).await {
        Ok(workspaces) => workspaces,
        Err(denial) => return denial.into_response(),
    };
    let Some(workspace_row) = workspaces.get(&gate.workspace_id) else {
        return Denial::ServerError.into_response();
    };
    let asset_ids: Vec<uuid::Uuid> = workspace_row.logo_asset_id.into_iter().collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let workspace = render_workspace_lite(workspace_row, &assets);
    let rendered = render_invite(row, &workspace, &actor.timezone);
    (StatusCode::OK, Json(rendered.view_value())).into_response()
}

/// `PATCH /api/workspaces/<slug>/invitations/<pk>/` (`invite.py:37-51`):
/// the inherited `partial_update` — 404, validate, save, re-render 200.
async fn invite_patch(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    // Routing precedes auth: an unparseable pk proxies to Django.
    let (parts, body) = req.into_parts();
    let Ok(id) = parse_pk(&pk) else {
        let req = Request::from_parts(parts, body);
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, gate) = match resolve_class_admin(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let row = match fetch_invite(pool, &id, &slug).await {
        Ok(Some(row)) => row,
        Ok(None) => return Denial::InviteNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let patch_body: Value = if bytes.is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(_) => return Denial::BadDetail(JSON_PARSE_ERROR.to_owned()).into_response(),
        }
    };
    let patch = match validate_invite_patch(pool, &patch_body, &actor.timezone).await {
        Ok(patch) => patch,
        Err(denial) => return denial.into_response(),
    };
    // `perform_update` → `save()` (stamps `updated_at` even for
    // `{}`), and `BaseModel.save()` stamps `updated_by` with the caller
    // on every update (`db/models/base.py:42`) — the body value was
    // validated above but never stored. Microsecond clock: the bound
    // value is echoed in the 200, so it resolves like Django's.
    let now = patch_now();
    let role = patch.role.unwrap_or(row.role);
    let accepted = patch.accepted.unwrap_or(row.accepted);
    let deleted_at = patch.deleted_at.unwrap_or(row.deleted_at);
    let created_by_id = patch.created_by.unwrap_or(row.created_by_id);
    let updated_by_id = Some(actor.id);
    if let Err(error) = sqlx::query(
        "UPDATE workspace_member_invites SET role = $1, accepted = $2, deleted_at = $3, \
         created_by_id = $4, updated_by_id = $5, updated_at = $6 WHERE id = $7",
    )
    .bind(role)
    .bind(accepted)
    .bind(deleted_at)
    .bind(created_by_id)
    .bind(updated_by_id)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await
    {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    let saved = InviteRow {
        role,
        accepted,
        deleted_at,
        created_by_id,
        updated_by_id,
        updated_at: now,
        ..row
    };
    render_single_invite(pool, &saved, &gate, &actor).await
}

/// `DELETE /api/workspaces/<slug>/invitations/<pk>/` (`invite.py:144-147`):
/// the custom `destroy` — bare `.get` (404 `error` body), soft-delete, 204.
async fn invite_destroy(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(id) = parse_pk(&pk) else {
        let req = Request::from_parts(parts, body);
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, _gate) = match resolve_class_admin(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    match fetch_invite(pool, &id, &slug).await {
        Ok(Some(_)) => {}
        Ok(None) => return Denial::MissingObject.into_response(),
        Err(denial) => return denial.into_response(),
    }
    // `SoftDeleteModel.delete(soft=True)` (`db/mixins.py:72-76`): stamps
    // `deleted_at` through a full `save()` (so `updated_at` and the
    // crum `updated_by` too). The `soft_delete_related_objects` sweep
    // it fires is owned by the jobs plane (no D-24 publisher; unpinned
    // by F-W24-14).
    let now = Utc::now();
    if let Err(error) = sqlx::query(
        "UPDATE workspace_member_invites SET deleted_at = $1, updated_at = $1, updated_by_id = $3 \
         WHERE id = $2",
    )
    .bind(now)
    .bind(id)
    .bind(actor.id)
    .execute(pool)
    .await
    {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Units 2-3: `WorkspaceJoinEndpoint` + `UserWorkspaceInvitationsViewSet`
// (`invite.py:150-305`)
// ---------------------------------------------------------------------------

/// `POST /api/workspaces/<slug>/invitations/<pk>/join/` (`invite.py:163-236`,
/// `AllowAny`): the four cache keys bust BEFORE the body runs (the
/// decorators wrap the method, so even 403s and 400s bust); then fetch →
/// token → responded → accept/reject.
async fn join_post(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    // Routing precedes everything: an unparseable pk proxies to Django
    // (no bust — Django's resolver 404s before the view runs).
    let (parts, body) = req.into_parts();
    let Ok(id) = parse_pk(&pk) else {
        let req = Request::from_parts(parts, body);
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match peek_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let caller_id = actor.as_ref().map(|actor| actor.id.to_string());
    bust_action(
        &state,
        gates::InvalidateAction::JoinPost,
        &slug,
        caller_id.as_deref(),
    )
    .await;
    let invite = match fetch_invite(pool, &id, &slug).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return Denial::MissingObject.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Denial::ServerError.into_response(),
    };
    let data: Value = if bytes.is_empty() {
        Value::Object(Map::new())
    } else {
        match serde_json::from_slice(&bytes) {
            Ok(body) => body,
            Err(_) => return Denial::BadDetail(JSON_PARSE_ERROR.to_owned()).into_response(),
        }
    };
    // `request.data.get("token", "")` — a non-dict body 500s.
    let Some(data) = data.as_object() else {
        return Denial::ServerError.into_response();
    };
    // `if not token or invite.token != token` — a missing, empty, or
    // non-string token mismatches (a non-string never equals the
    // stored string).
    let provided = data.get("token").and_then(Value::as_str).unwrap_or("");
    if queries::join_token_denied(provided, &invite.token) {
        return Denial::JoinForbidden.into_response();
    }
    if invite.responded_at.is_some() {
        return json_response(StatusCode::BAD_REQUEST, ALREADY_RESPONDED_INVITE_BODY);
    }
    // `accepted = ...; responded_at = now; save()` — full-row update.
    // `BaseModel.save()` stamps `updated_by` with the responder
    // (`db/models/base.py:42`); anonymous nulls BOTH audit columns
    // (`:31-33`), so `created_by` keeps the row's value only when authed.
    let now = Utc::now();
    let accepted = data.get("accepted").is_some_and(py_truthy);
    let responder_id: Option<uuid::Uuid> = actor.as_ref().map(|actor| actor.id);
    let (updated_by_id, created_by_id) =
        audit_columns_on_update(responder_id, invite.created_by_id);
    if let Err(error) = sqlx::query(
        "UPDATE workspace_member_invites SET accepted = $1, responded_at = $2, updated_at = $2, \
         updated_by_id = $3, created_by_id = $4 WHERE id = $5",
    )
    .bind(accepted)
    .bind(now)
    .bind(updated_by_id)
    .bind(created_by_id)
    .bind(id)
    .execute(pool)
    .await
    {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    if !accepted {
        // Rejected: the invite is KEPT (with `responded_at` set — stuck).
        return (
            StatusCode::OK,
            Json(message_envelope(JOIN_REJECTED_MESSAGE)),
        )
            .into_response();
    }
    // Accepted: the invitee may have no account yet (`:183`) — then the
    // accepted message still answers and the invite is KEPT (stuck).
    let user: Option<(uuid::Uuid,)> = match sqlx::query_as(
        "SELECT u.id FROM users u WHERE u.email = $1 ORDER BY u.created_at DESC LIMIT 1",
    )
    .bind(&invite.email)
    .fetch_optional(pool)
    .await
    {
        Ok(user) => user,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let Some((user_id,)) = user else {
        return (
            StatusCode::OK,
            Json(message_envelope(JOIN_ACCEPTED_MESSAGE)),
        )
            .into_response();
    };
    // Reactivate-or-create (`:188-201`) + the `User`-model pointer
    // no-op + track + delete. Both branches run a full `save()`: the
    // update stamps `updated_by` (anonymous nulls both audit columns);
    // the create stamps `created_by` with the responder and leaves
    // `updated_by` NULL when authed (`db/models/base.py:37-39`),
    // nulling both when anonymous (`:31-33`).
    let existing: Option<(uuid::Uuid, Option<uuid::Uuid>)> = match sqlx::query_as(
        "SELECT wm.id, wm.created_by_id FROM workspace_members wm \
         WHERE wm.workspace_id = $1 AND wm.member_id = $2 \
         AND wm.deleted_at IS NULL ORDER BY wm.created_at DESC LIMIT 1",
    )
    .bind(invite.workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    {
        Ok(existing) => existing,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // Django-side member defaults for the create arm below: no member
    // column carries a DB default, so the INSERT must supply every
    // NOT NULL field (`workspace.py:207-213`; PIDASHCONV-751).
    let view_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let default_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let issue_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_ISSUE_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let empty_dict = match member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON)
    {
        Ok(props) => props,
        Err(denial) => return denial.into_response(),
    };
    if let Some((member_id, member_created_by)) = existing {
        let (updated_by_id, created_by_id) =
            audit_columns_on_update(responder_id, member_created_by);
        if let Err(error) = sqlx::query(
            "UPDATE workspace_members SET is_active = TRUE, role = $1, updated_at = $2, \
             updated_by_id = $3, created_by_id = $4 WHERE id = $5",
        )
        .bind(invite.role)
        .bind(now)
        .bind(updated_by_id)
        .bind(created_by_id)
        .bind(member_id)
        .execute(pool)
        .await
        {
            if is_integrity_error(&error) {
                return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
            }
            return Denial::ServerError.into_response();
        }
    } else if let Err(error) = sqlx::query(
        "INSERT INTO workspace_members \
         (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
         workspace_id, member_id, role, company_role, view_props, default_props, issue_props, \
         getting_started_checklist, tips, explored_features, is_active) \
         VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, NULL, $7, $8, $9, $10, $11, $12, TRUE)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(responder_id)
    .bind(invite.workspace_id)
    .bind(user_id)
    .bind(invite.role)
    .bind(view_props)
    .bind(default_props)
    .bind(issue_props)
    .bind(empty_dict.clone())
    .bind(empty_dict.clone())
    .bind(empty_dict)
    .execute(pool)
    .await
    {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    // `user.last_workspace_id = ...; user.save()` (`:204-205`): `User`
    // has no such column, so Django drops the pointer and the save only
    // stamps `updated_at` ([`queries::user_touch_sql`]).
    if sqlx::query("UPDATE users SET updated_at = $1 WHERE id = $2")
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await
        .is_err()
    {
        return Denial::ServerError.into_response();
    }
    let joined_at = django_str_now(&now);
    enqueue_track(
        pool,
        &tasks::user_joined_event(
            &user_id.to_string(),
            &invite.workspace_id.to_string(),
            &slug,
            i32::from(invite.role),
            &joined_at,
        ),
    )
    .await;
    // The accept soft-delete (`.delete()` → `save()`): stamps
    // `updated_by` with the responder, nulling both audit columns when
    // anonymous (`db/models/base.py:31-42`).
    let (accept_updated_by, accept_created_by) =
        audit_columns_on_update(responder_id, invite.created_by_id);
    if sqlx::query(
        "UPDATE workspace_member_invites SET deleted_at = $1, updated_at = $1, updated_by_id = $3, \
         created_by_id = $4 WHERE id = $2",
    )
    .bind(now)
    .bind(id)
    .bind(accept_updated_by)
    .bind(accept_created_by)
    .execute(pool)
    .await
    .is_err()
    {
        return Denial::ServerError.into_response();
    }
    (
        StatusCode::OK,
        Json(message_envelope(JOIN_ACCEPTED_MESSAGE)),
    )
        .into_response()
}

/// `GET /api/workspaces/<slug>/invitations/<pk>/join/` (`invite.py:238-241`,
/// `AllowAny`): the full invite shape, readable unauthenticated (anon
/// datetimes render UTC — `TimezoneMixin` deactivates).
async fn join_get(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let Ok(id) = parse_pk(&pk) else {
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match peek_actor(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let invite = match fetch_invite(pool, &id, &slug).await {
        Ok(Some(invite)) => invite,
        Ok(None) => return Denial::MissingObject.into_response(),
        Err(denial) => return denial.into_response(),
    };
    let workspaces = match fetch_workspace_lites(pool, &[invite.workspace_id]).await {
        Ok(workspaces) => workspaces,
        Err(denial) => return denial.into_response(),
    };
    let Some(workspace_row) = workspaces.get(&invite.workspace_id) else {
        return Denial::ServerError.into_response();
    };
    let asset_ids: Vec<uuid::Uuid> = workspace_row.logo_asset_id.into_iter().collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let workspace = render_workspace_lite(workspace_row, &assets);
    let timezone = actor.map(|actor| actor.timezone).unwrap_or(chrono_tz::UTC);
    let rendered = render_invite(&invite, &workspace, &timezone);
    (StatusCode::OK, Json(rendered.view_value())).into_response()
}

/// `GET /api/users/me/workspaces/invitations/` (`invite.py:248-251`): the
/// caller's own invites by CURRENT email (stale when they changed it
/// after the invite — ported as-is), as a plain array.
async fn my_invites_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match resolve_authenticated(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    // `filter(email=None)` renders `IS NULL` over a non-nullable column:
    // no rows, still 200 `[]`.
    let Some(email) = actor.email.as_deref() else {
        return (StatusCode::OK, Json(Value::Array(vec![]))).into_response();
    };
    let rows: Vec<InviteRow> = match sqlx::query_as::<_, InviteRow>(
        "SELECT i.id, i.workspace_id, i.created_at, i.updated_at, i.deleted_at, i.email, \
         i.accepted, i.token, i.message, i.responded_at, i.role, i.created_by_id, i.updated_by_id \
         FROM workspace_member_invites i \
         WHERE i.email = $1 AND i.deleted_at IS NULL ORDER BY i.created_at DESC",
    )
    .bind(email)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    render_invite_list(pool, &rows, &actor.timezone).await
}

/// Render invite rows from mixed workspaces (my-list): one workspace
/// fetch plus one asset fetch for the batch.
async fn render_invite_list(pool: &PgPool, rows: &[InviteRow], tz: &chrono_tz::Tz) -> Response {
    let mut workspace_ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.workspace_id).collect();
    workspace_ids.sort();
    workspace_ids.dedup();
    let workspaces = match fetch_workspace_lites(pool, &workspace_ids).await {
        Ok(workspaces) => workspaces,
        Err(denial) => return denial.into_response(),
    };
    let asset_ids: Vec<uuid::Uuid> = workspaces
        .values()
        .filter_map(|row| row.logo_asset_id)
        .collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        // Inner join in Django (`select_related`): a missing workspace
        // drops the row (unreachable — the FK is non-nullable).
        let Some(workspace_row) = workspaces.get(&row.workspace_id) else {
            continue;
        };
        let workspace = render_workspace_lite(workspace_row, &assets);
        let rendered = render_invite(row, &workspace, tz);
        views.push(rendered.view_value());
    }
    (StatusCode::OK, Json(Value::Array(views))).into_response()
}

/// How the `invitations` input classifies (`invite.py:256-259`).
enum InvitationsInput {
    /// Query these ids (possibly none).
    Ids(Vec<uuid::Uuid>),
    /// Unparseable UUID: 400 `"Please provide valid detail"`.
    InvalidUuid,
    /// Non-iterable / null: 500.
    ServerError,
}

/// `filter(pk__in=...)` over a JSON value: Django iterates the input
/// (strings iterate chars, dicts iterate keys — empty iterates to
/// nothing, matching no rows) and UUID-preps each item (`None` items
/// prep to `NULL`, matching nothing; bools ride `UUID(int=)`; negative
/// ints, floats, arrays and objects fail prep → `ValidationError` →
/// 400). Non-iterables (numbers, bools, null) raise `TypeError` → 500.
fn classify_invitations(value: Option<&Value>) -> InvitationsInput {
    let Some(value) = value else {
        return InvitationsInput::Ids(vec![]);
    };
    // One UUID prep (`UUIDField.to_python`): `None` skips, bools/ints
    // ride `int=`, strings ride `hex=`, the rest fail.
    fn prep(item: &Value, out: &mut Vec<uuid::Uuid>) -> Result<(), ()> {
        match item {
            Value::Null => Ok(()),
            Value::Bool(true) => {
                out.push(uuid::Uuid::from_u128(1));
                Ok(())
            }
            Value::Bool(false) => {
                out.push(uuid::Uuid::from_u128(0));
                Ok(())
            }
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    if i < 0 {
                        return Err(());
                    }
                    out.push(uuid::Uuid::from_u128(i as u128));
                    Ok(())
                } else if let Some(u) = n.as_u64() {
                    out.push(uuid::Uuid::from_u128(u as u128));
                    Ok(())
                } else if let Ok(wide) = n.to_string().parse::<u128>() {
                    // `uuid.UUID(int=)` takes any `0 <= i < 2**128`.
                    out.push(uuid::Uuid::from_u128(wide));
                    Ok(())
                } else {
                    Err(())
                }
            }
            Value::String(s) => {
                out.push(parse_uuid_hex(s).ok_or(())?);
                Ok(())
            }
            Value::Array(_) | Value::Object(_) => Err(()),
        }
    }
    match value {
        Value::Null | Value::Number(_) | Value::Bool(_) => InvitationsInput::ServerError,
        Value::Array(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                if prep(item, &mut ids).is_err() {
                    return InvitationsInput::InvalidUuid;
                }
            }
            InvitationsInput::Ids(ids)
        }
        // A string iterates its chars; a dict iterates its keys.
        Value::String(s) => {
            let mut ids = Vec::new();
            for c in s.chars() {
                let mut buf = [0u8; 4];
                let ch = c.encode_utf8(&mut buf);
                if prep(&Value::String(ch.to_owned()), &mut ids).is_err() {
                    return InvitationsInput::InvalidUuid;
                }
            }
            InvitationsInput::Ids(ids)
        }
        Value::Object(map) => {
            let mut ids = Vec::with_capacity(map.len());
            for key in map.keys() {
                if prep(&Value::String(key.clone()), &mut ids).is_err() {
                    return InvitationsInput::InvalidUuid;
                }
            }
            InvitationsInput::Ids(ids)
        }
    }
}

/// `POST /api/users/me/workspaces/invitations/` (`invite.py:255-304`): the
/// two decorator keys bust after the auth check but before the body;
/// then per-invite direct bust + member update + track, bulk-create,
/// invite delete, 204.
async fn my_invites_create(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match resolve_authenticated(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let caller_id = actor.id.to_string();
    bust_action(
        &state,
        gates::InvalidateAction::JoinBulkCreate,
        "",
        Some(&caller_id),
    )
    .await;
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `request.data.get("invitations", [])` — a non-dict body 500s.
    let data = match body.as_object() {
        Some(data) => data,
        None => return Denial::ServerError.into_response(),
    };
    let ids = match classify_invitations(data.get("invitations")) {
        InvitationsInput::Ids(ids) => ids,
        InvitationsInput::InvalidUuid => {
            return json_response(StatusCode::BAD_REQUEST, VALID_DETAIL_BODY);
        }
        InvitationsInput::ServerError => return Denial::ServerError.into_response(),
    };
    // `filter(pk__in=..., email=...)` (`:257-259`; other users' pks are
    // silently ignored — the email conjunct filters them out).
    let Some(email) = actor.email.as_deref() else {
        // `email=None` matches nothing; the writes below are no-ops.
        return StatusCode::NO_CONTENT.into_response();
    };
    if ids.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT i.id, i.workspace_id, i.created_at, i.updated_at, i.deleted_at, i.email, \
         i.accepted, i.token, i.message, i.responded_at, i.role, i.created_by_id, i.updated_by_id \
         FROM workspace_member_invites i \
         WHERE i.id IN ({}) AND i.email = ${} AND i.deleted_at IS NULL \
         ORDER BY i.created_at DESC",
        placeholders.join(", "),
        ids.len() + 1
    );
    let mut query = sqlx::query_as::<_, InviteRow>(&sql);
    for id in &ids {
        query = query.bind(*id);
    }
    let invites: Vec<InviteRow> = match query.bind(email).fetch_all(pool).await {
        Ok(invites) => invites,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let now = Utc::now();
    let joined_at = django_str_now(&now);
    let user_id = actor.id;
    let user_id_text = user_id.to_string();
    // Per-invite member update (`:270-272`, `SET` only — no `updated_at`)
    // plus the direct bust and the track event.
    for invite in invites.iter() {
        let workspaces = match fetch_workspace_lites(pool, &[invite.workspace_id]).await {
            Ok(workspaces) => workspaces,
            Err(denial) => return denial.into_response(),
        };
        // Inner join in Django: a missing workspace drops the invite.
        let Some(workspace_row) = workspaces.get(&invite.workspace_id) else {
            continue;
        };
        bust_members_key(&state, &workspace_row.slug).await;
        if let Err(error) = sqlx::query(
            "UPDATE workspace_members SET is_active = TRUE, role = $1 \
             WHERE workspace_id = $2 AND member_id = $3 AND deleted_at IS NULL",
        )
        .bind(invite.role)
        .bind(invite.workspace_id)
        .bind(user_id)
        .execute(pool)
        .await
        {
            if is_integrity_error(&error) {
                return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
            }
            return Denial::ServerError.into_response();
        }
        enqueue_track(
            pool,
            &tasks::user_joined_event(
                &user_id_text,
                &invite.workspace_id.to_string(),
                &workspace_row.slug,
                i32::from(invite.role),
                &joined_at,
            ),
        )
        .await;
    }
    // The bulk-create (`:289-300`, `created_by` explicit, no batch size)
    // then the queryset soft-delete (`:303`, `deleted_at` only).
    // `bulk_create` writes every model field and no member column
    // carries a DB default, so the JSON props and `is_active` ride
    // along explicitly (PIDASHCONV-751).
    if !invites.is_empty() {
        let mut values: Vec<String> = Vec::with_capacity(invites.len());
        for index in 0..invites.len() {
            let base = index * 13;
            values.push(format!(
                "(${}, ${}, ${}, ${}, NULL, NULL, ${}, ${}, ${}, NULL, \
                 ${}, ${}, ${}, ${}, ${}, ${}, TRUE)",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 10,
                base + 11,
                base + 12,
                base + 13
            ));
        }
        let sql = format!(
            "INSERT INTO workspace_members \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
             workspace_id, member_id, role, company_role, view_props, default_props, \
             issue_props, getting_started_checklist, tips, explored_features, is_active) \
             VALUES {} ON CONFLICT DO NOTHING",
            values.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for invite in invites.iter() {
            // Fresh defaults per row: the Django callables mint a new
            // dict per instance.
            let view_props = match member_default_json(
                models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON,
            ) {
                Ok(props) => props,
                Err(denial) => return denial.into_response(),
            };
            let default_props = match member_default_json(
                models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON,
            ) {
                Ok(props) => props,
                Err(denial) => return denial.into_response(),
            };
            let issue_props = match member_default_json(
                models_workspace::workspace_member::DEFAULT_ISSUE_PROPS_JSON,
            ) {
                Ok(props) => props,
                Err(denial) => return denial.into_response(),
            };
            let empty_dict =
                match member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON) {
                    Ok(props) => props,
                    Err(denial) => return denial.into_response(),
                };
            query = query
                .bind(uuid::Uuid::new_v4())
                .bind(now)
                .bind(now)
                .bind(user_id)
                .bind(invite.workspace_id)
                .bind(user_id)
                .bind(invite.role)
                .bind(view_props)
                .bind(default_props)
                .bind(issue_props)
                .bind(empty_dict.clone())
                .bind(empty_dict.clone())
                .bind(empty_dict);
        }
        if let Err(error) = query.execute(pool).await {
            if is_integrity_error(&error) {
                return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
            }
            return Denial::ServerError.into_response();
        }
        let placeholders: Vec<String> =
            (1..=invites.len()).map(|i| format!("${}", i + 1)).collect();
        let sql = format!(
            "UPDATE workspace_member_invites SET deleted_at = $1 \
             WHERE id IN ({}) AND email = ${} AND deleted_at IS NULL",
            placeholders.join(", "),
            invites.len() + 2
        );
        let mut query = sqlx::query(&sql).bind(now);
        for invite in invites.iter() {
            query = query.bind(invite.id);
        }
        if query.bind(email).execute(pool).await.is_err() {
            return Denial::ServerError.into_response();
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// Units 4-5: join requests (`join_request.py:32-255`)
// ---------------------------------------------------------------------------

/// `TextField` coercion for `message` (`join_request.py:117-123`):
/// `TextField.get_prep_value` is `str(value)` (pinned Django source,
/// probed live) — bools store as `True`/`False`, dicts/lists store as
/// their repr (and the endpoint 201s), numbers store as their Python
/// rendering. Missing/null stores `NULL`.
enum MessageValue {
    Null,
    Text(String),
}

fn coerce_message(value: Option<&Value>) -> MessageValue {
    match value {
        None | Some(Value::Null) => MessageValue::Null,
        Some(Value::String(s)) => MessageValue::Text(s.clone()),
        Some(Value::Bool(true)) => MessageValue::Text("True".to_owned()),
        Some(Value::Bool(false)) => MessageValue::Text("False".to_owned()),
        Some(Value::Number(n)) => MessageValue::Text(py_num_str(n)),
        // `str()` of a container is its repr ([`py_repr`]).
        Some(value @ (Value::Array(_) | Value::Object(_))) => MessageValue::Text(py_repr(value)),
    }
}

/// `GET /api/users/me/workspaces/join-requests/` (`join_request.py:43-46`):
/// the inherited `list` over the caller's own requests (8-key shape, no
/// `workspace` key), as a plain array.
async fn user_join_requests_list(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match resolve_authenticated(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let rows: Vec<JoinRequestRow> = match sqlx::query_as::<_, JoinRequestRow>(
        "SELECT jr.id, jr.workspace_id, jr.requester_id, jr.created_at, jr.updated_at, \
         jr.deleted_at, jr.admin_email, jr.message, jr.role, jr.status, jr.responded_at, \
         jr.created_by_id, jr.updated_by_id, jr.responded_by_id \
         FROM workspace_join_requests jr \
         WHERE jr.requester_id = $1 AND jr.deleted_at IS NULL ORDER BY jr.created_at DESC",
    )
    .bind(actor.id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // Every row's requester is the caller: one user fetch.
    let users = match fetch_user_lites(pool, &[actor.id]).await {
        Ok(users) => users,
        Err(denial) => return denial.into_response(),
    };
    let Some(user_row) = users.get(&actor.id) else {
        if rows.is_empty() {
            return (StatusCode::OK, Json(Value::Array(vec![]))).into_response();
        }
        return Denial::ServerError.into_response();
    };
    let asset_ids: Vec<uuid::Uuid> = user_row.avatar_asset_id.into_iter().collect();
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let requester = render_user_lite(user_row, &assets);
    let mut views = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        let rendered = render_join_request(row, None, &requester, &actor.timezone);
        views.push(rendered.user_view_value());
    }
    (StatusCode::OK, Json(Value::Array(views))).into_response()
}

/// `POST /api/users/me/workspaces/join-requests/` (`join_request.py:48-153`),
/// in Django's order: admin-email format → own-email → admin resolution →
/// already-member short-circuit → per-workspace idempotent create (or the
/// unresolved branch) → neutral 201.
async fn user_join_requests_create(
    State(state): State<AppState>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let actor = match resolve_authenticated(&state, extension).await {
        Ok(actor) => actor,
        Err(denial) => return denial.into_response(),
    };
    let body = match read_json_body(req).await {
        Ok(body) => body,
        Err(denial) => return denial.into_response(),
    };
    // `(request.data.get("admin_email") or "").strip().lower()` — a
    // non-dict body has no `.get` (500); a truthy non-string has no
    // `.strip` (500); falsy values become `""` (400 below).
    let data = match body.as_object() {
        Some(data) => data,
        None => return Denial::ServerError.into_response(),
    };
    let admin_email = match data.get("admin_email") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) if s.is_empty() => String::new(),
        Some(Value::String(s)) => py_strip(s).to_lowercase(),
        Some(v) if !py_truthy(v) => String::new(),
        Some(_) => return Denial::ServerError.into_response(),
    };
    if !is_valid_email_str(&admin_email) {
        return json_response(StatusCode::BAD_REQUEST, ADMIN_EMAIL_REQUIRED_BODY);
    }
    // A user cannot request with their own email (`:62-66`; a null or
    // empty caller email skips the check).
    if let Some(own) = actor.email.as_deref().filter(|email| !email.is_empty()) {
        if admin_email == py_strip(own).to_lowercase() {
            return json_response(StatusCode::BAD_REQUEST, OWN_EMAIL_BODY);
        }
    }
    let message = match coerce_message(data.get("message")) {
        MessageValue::Null => None,
        MessageValue::Text(text) => Some(text),
    };
    // Admin resolution (`:70-75`): active Admin memberships of the typed
    // email, UNION workspaces it owns.
    let targets: Vec<uuid::Uuid> = match sqlx::query_scalar(
        "SELECT wm.workspace_id FROM workspace_members wm JOIN users u ON u.id = wm.member_id \
         WHERE u.email = $1 AND wm.role = 20 AND wm.is_active = TRUE AND wm.deleted_at IS NULL \
         UNION SELECT w.id FROM workspaces w WHERE w.owner_id IN \
         (SELECT u2.id FROM users u2 WHERE u2.email = $1) AND w.deleted_at IS NULL",
    )
    .bind(&admin_email)
    .fetch_all(pool)
    .await
    {
        Ok(targets) => targets,
        Err(_) => return Denial::ServerError.into_response(),
    };
    // Minus workspaces the requester already belongs to (`:78-83`).
    let already: Vec<uuid::Uuid> = if targets.is_empty() {
        vec![]
    } else {
        let placeholders: Vec<String> =
            (1..=targets.len()).map(|i| format!("${}", i + 1)).collect();
        let sql = format!(
            "SELECT wm.workspace_id FROM workspace_members wm WHERE wm.member_id = $1 \
             AND wm.workspace_id IN ({}) AND wm.is_active = TRUE AND wm.deleted_at IS NULL",
            placeholders.join(", ")
        );
        let mut query = sqlx::query_scalar::<_, uuid::Uuid>(&sql).bind(actor.id);
        for target in &targets {
            query = query.bind(*target);
        }
        match query.fetch_all(pool).await {
            Ok(already) => already,
            Err(_) => return Denial::ServerError.into_response(),
        }
    };
    let targets: Vec<uuid::Uuid> = targets
        .into_iter()
        .filter(|id| !already.contains(id))
        .collect();
    // All-already (`:90-100`): route into the earliest-created existing
    // workspace (200 with the slug — safe to name, they belong).
    if targets.is_empty() && !already.is_empty() {
        let placeholders: Vec<String> = (1..=already.len()).map(|i| format!("${i}")).collect();
        let sql = format!(
            "SELECT w.slug FROM workspaces w WHERE w.id IN ({}) AND w.deleted_at IS NULL \
             ORDER BY w.created_at ASC LIMIT 1",
            placeholders.join(", ")
        );
        let mut query = sqlx::query_scalar::<_, String>(&sql);
        for id in &already {
            query = query.bind(*id);
        }
        let slug: Option<String> = match query.fetch_optional(pool).await {
            Ok(slug) => slug,
            Err(_) => return Denial::ServerError.into_response(),
        };
        let mut body = Map::new();
        body.insert(
            "message".to_owned(),
            Value::String(ALREADY_MEMBER_MESSAGE.to_owned()),
        );
        body.insert(
            "workspace_slug".to_owned(),
            slug.map(Value::String).unwrap_or(Value::Null),
        );
        return (StatusCode::OK, Json(Value::Object(body))).into_response();
    }
    let now = Utc::now();
    if !targets.is_empty() {
        for workspace_id in targets.iter() {
            // Idempotent per-workspace create (`:105-125`): the pending
            // `exists()` skips, and a concurrent loser is swallowed as
            // `IntegrityError` (any class-23, like Django).
            let pending: bool = match sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM workspace_join_requests jr \
                 WHERE jr.requester_id = $1 AND jr.workspace_id = $2 AND jr.status = 'PENDING' \
                 AND jr.deleted_at IS NULL)",
            )
            .bind(actor.id)
            .bind(workspace_id)
            .fetch_one(pool)
            .await
            {
                Ok(pending) => pending,
                Err(_) => return Denial::ServerError.into_response(),
            };
            if pending {
                continue;
            }
            let result = sqlx::query(
                "INSERT INTO workspace_join_requests \
                 (id, created_at, updated_at, created_by_id, requester_id, workspace_id, \
                 admin_email, message, role, status) \
                 VALUES ($1, $2, $2, $3, $3, $4, $5, $6, $7, 'PENDING')",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(actor.id)
            .bind(workspace_id)
            .bind(&admin_email)
            .bind(message.as_deref())
            .bind(queries::JOIN_REQUEST_DEFAULT_ROLE as i16)
            .execute(pool)
            .await;
            if let Err(error) = result {
                if !is_integrity_error(&error) {
                    return Denial::ServerError.into_response();
                }
            }
        }
    } else {
        // Unresolved (`:137-150`): record a workspace-less request so the
        // pending state looks identical (the partial unique does NOT
        // cover `NULL` — the `exists()` is the only guard).
        let pending: bool = match sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workspace_join_requests jr \
             WHERE jr.requester_id = $1 AND jr.workspace_id IS NULL AND jr.admin_email = $2 \
             AND jr.status = 'PENDING' AND jr.deleted_at IS NULL)",
        )
        .bind(actor.id)
        .bind(&admin_email)
        .fetch_one(pool)
        .await
        {
            Ok(pending) => pending,
            Err(_) => return Denial::ServerError.into_response(),
        };
        if !pending {
            if let Err(error) = sqlx::query(
                "INSERT INTO workspace_join_requests \
                 (id, created_at, updated_at, created_by_id, requester_id, workspace_id, \
                 admin_email, message, role, status) \
                 VALUES ($1, $2, $2, $3, $3, NULL, $4, $5, $6, 'PENDING')",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(actor.id)
            .bind(&admin_email)
            .bind(message.as_deref())
            .bind(queries::JOIN_REQUEST_DEFAULT_ROLE as i16)
            .execute(pool)
            .await
            {
                if is_integrity_error(&error) {
                    return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
                }
                return Denial::ServerError.into_response();
            }
        }
    }
    // Always the neutral response (`:152-153`), resolved or not.
    (
        StatusCode::CREATED,
        Json(message_envelope(REQUEST_SENT_MESSAGE)),
    )
        .into_response()
}

/// Fetch one join request by pk + workspace for approve/deny
/// (`join_request.py:188,242`): direct `get_object_or_404`, NO status
/// filter (non-pending rows 400 below, not 404). The
/// `workspace__slug` traversal is unscoped on the workspace side (see
/// [`resolve_class_admin`]).
async fn fetch_join_request(
    pool: &PgPool,
    pk: &uuid::Uuid,
    slug: &str,
) -> Result<Option<JoinRequestRow>, Denial> {
    sqlx::query_as::<_, JoinRequestRow>(FETCH_JOIN_REQUEST_SQL)
        .bind(pk)
        .bind(slug)
        .fetch_optional(pool)
        .await
        .map_err(|_| Denial::ServerError)
}

/// `GET /api/workspaces/<slug>/join-requests/` (`join_request.py:168-177`):
/// the pending requests, `-created_at` first, as a plain array.
async fn admin_join_requests_list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    extension: Option<Extension<SessionHandle>>,
) -> Response {
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, workspace_id) = match resolve_class_owner(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let rows: Vec<JoinRequestRow> = match sqlx::query_as::<_, JoinRequestRow>(
        "SELECT jr.id, jr.workspace_id, jr.requester_id, jr.created_at, jr.updated_at, \
         jr.deleted_at, jr.admin_email, jr.message, jr.role, jr.status, jr.responded_at, \
         jr.created_by_id, jr.updated_by_id, jr.responded_by_id \
         FROM workspace_join_requests jr \
         WHERE jr.workspace_id = $1 AND jr.status = 'PENDING' AND jr.deleted_at IS NULL \
         ORDER BY jr.created_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let workspaces = match fetch_workspace_lites(pool, &[workspace_id]).await {
        Ok(workspaces) => workspaces,
        Err(denial) => return denial.into_response(),
    };
    let Some(workspace_row) = workspaces.get(&workspace_id) else {
        if rows.is_empty() {
            return (StatusCode::OK, Json(Value::Array(vec![]))).into_response();
        }
        return Denial::ServerError.into_response();
    };
    let mut requester_ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.requester_id).collect();
    requester_ids.sort();
    requester_ids.dedup();
    let users = match fetch_user_lites(pool, &requester_ids).await {
        Ok(users) => users,
        Err(denial) => return denial.into_response(),
    };
    let mut asset_ids: Vec<uuid::Uuid> = users
        .values()
        .filter_map(|row| row.avatar_asset_id)
        .collect();
    asset_ids.extend(workspace_row.logo_asset_id);
    let assets = match fetch_assets(pool, &asset_ids).await {
        Ok(assets) => assets,
        Err(denial) => return denial.into_response(),
    };
    let workspace = render_workspace_lite(workspace_row, &assets);
    let mut views = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        // Inner join in Django: a missing requester drops the row.
        let Some(user_row) = users.get(&row.requester_id) else {
            continue;
        };
        let requester = render_user_lite(user_row, &assets);
        let rendered = render_join_request(row, Some(&workspace), &requester, &actor.timezone);
        views.push(rendered.view_value());
    }
    (StatusCode::OK, Json(Value::Array(views))).into_response()
}

/// `POST /api/workspaces/<slug>/join-requests/<pk>/approve/`
/// (`join_request.py:187-239`): the three cache keys bust after the
/// owner check but before the body (even a later 400 busts); then
/// fetch → pending → atomic member/profile/status → track → 200.
async fn join_request_approve(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let (parts, _body) = req.into_parts();
    let Ok(id) = parse_pk(&pk) else {
        let req = Request::from_parts(parts, axum::body::Body::empty());
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, _workspace_id) = match resolve_class_owner(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let caller_id = actor.id.to_string();
    bust_action(
        &state,
        gates::InvalidateAction::ApproveJoinRequest,
        &slug,
        Some(&caller_id),
    )
    .await;
    let request = match fetch_join_request(pool, &id, &slug).await {
        Ok(Some(request)) => request,
        Ok(None) => return Denial::JoinRequestNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    if queries::join_request_pending_blocks(&request.status) {
        return json_response(StatusCode::BAD_REQUEST, ALREADY_RESPONDED_REQUEST_BODY);
    }
    // The approvee's workspace: the fetch's inner join proved a
    // workspace row exists (liveness unscoped), so this only resolves
    // the id.
    let Some(workspace_id) = request.workspace_id else {
        return Denial::ServerError.into_response();
    };
    let now = Utc::now();
    // `transaction.atomic` (`:202-224`): member + profile pointer +
    // status land together. An `IntegrityError` inside aborts to the
    // 400 invalid-payload branch.
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(_) => return Denial::ServerError.into_response(),
    };
    let requester_id = request.requester_id;
    let request_role = request.role;
    let approver_id = actor.id;
    // Django-side member defaults for the create arm: no member column
    // carries a DB default (`workspace.py:207-213`; PIDASHCONV-751).
    // Parsed outside the `async move` so a bad constant answers 500
    // through `Denial` instead of poisoning the `sqlx::Error` chain.
    let view_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let default_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_VIEW_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let issue_props =
        match member_default_json(models_workspace::workspace_member::DEFAULT_ISSUE_PROPS_JSON) {
            Ok(props) => props,
            Err(denial) => return denial.into_response(),
        };
    let empty_dict = match member_default_json(models_workspace::workspace_member::EMPTY_DICT_JSON)
    {
        Ok(props) => props,
        Err(denial) => return denial.into_response(),
    };
    let approved = async move {
        let existing: Option<(uuid::Uuid,)> = sqlx::query_as(
            "SELECT wm.id FROM workspace_members wm WHERE wm.workspace_id = $1 AND wm.member_id = $2 \
             AND wm.deleted_at IS NULL ORDER BY wm.created_at DESC LIMIT 1",
        )
        .bind(workspace_id)
        .bind(requester_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((member_id,)) = existing {
            // The reactivate `.save()` stamps `updated_by` with the
            // approver (`db/models/base.py:42`).
            sqlx::query(
                "UPDATE workspace_members SET is_active = TRUE, role = $1, updated_at = $2, \
                 updated_by_id = $4 WHERE id = $3",
            )
            .bind(request_role)
            .bind(now)
            .bind(member_id)
            .bind(approver_id)
            .execute(&mut *tx)
            .await?;
        } else {
            // The create `.save()` keeps `updated_by` NULL on add
            // (`db/models/base.py:37-39`), so the column stays unset —
            // while the JSON props ride the model defaults (`:207-213`).
            sqlx::query(
                "INSERT INTO workspace_members \
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                 workspace_id, member_id, role, company_role, view_props, default_props, \
                 issue_props, getting_started_checklist, tips, explored_features, is_active) \
                 VALUES ($1, $2, $2, $3, NULL, NULL, $4, $5, $6, NULL, $7, $8, $9, \
                 $10, $11, $12, TRUE)",
            )
            .bind(uuid::Uuid::new_v4())
            .bind(now)
            .bind(approver_id)
            .bind(workspace_id)
            .bind(requester_id)
            .bind(request_role)
            .bind(view_props)
            .bind(default_props)
            .bind(issue_props)
            .bind(empty_dict.clone())
            .bind(empty_dict.clone())
            .bind(empty_dict)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE profiles SET last_workspace_id = $1 WHERE user_id = $2")
            .bind(workspace_id)
            .bind(requester_id)
            .execute(&mut *tx)
            .await?;
        // The status `.save()` stamps `updated_by` with the approver
        // (`db/models/base.py:42`).
        sqlx::query(
            "UPDATE workspace_join_requests SET status = 'APPROVED', responded_at = $1, \
             responded_by_id = $2, updated_at = $1, updated_by_id = $2 WHERE id = $3",
        )
        .bind(now)
        .bind(approver_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;
    if let Err(error) = approved {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    let joined_at = django_str_now(&now);
    enqueue_track(
        pool,
        &tasks::join_request_approved_event(
            &request.requester_id.to_string(),
            &workspace_id.to_string(),
            &slug,
            i32::from(request.role),
            &joined_at,
        ),
    )
    .await;
    (
        StatusCode::OK,
        Json(message_envelope(REQUEST_APPROVED_MESSAGE)),
    )
        .into_response()
}

/// `POST /api/workspaces/<slug>/join-requests/<pk>/deny/`
/// (`join_request.py:241-255`): fetch → pending → single-save status
/// (NO transaction, NO cache bust) → 200.
async fn join_request_deny(
    State(state): State<AppState>,
    Path((slug, pk)): Path<(String, String)>,
    extension: Option<Extension<SessionHandle>>,
    req: Request,
) -> Response {
    let (parts, _body) = req.into_parts();
    let Ok(id) = parse_pk(&pk) else {
        let req = Request::from_parts(parts, axum::body::Body::empty());
        return crate::edge::proxy(State(state), req).await;
    };
    let pool = match pool_of(&state) {
        Ok(pool) => pool,
        Err(denial) => return denial.into_response(),
    };
    let (actor, _workspace_id) = match resolve_class_owner(&state, &slug, extension).await {
        Ok(resolved) => resolved,
        Err(denial) => return denial.into_response(),
    };
    let request = match fetch_join_request(pool, &id, &slug).await {
        Ok(Some(request)) => request,
        Ok(None) => return Denial::JoinRequestNotFound.into_response(),
        Err(denial) => return denial.into_response(),
    };
    if queries::join_request_pending_blocks(&request.status) {
        return json_response(StatusCode::BAD_REQUEST, ALREADY_RESPONDED_REQUEST_BODY);
    }
    let now = Utc::now();
    // The deny `.save()` stamps `updated_by` with the denier
    // (`db/models/base.py:42`; same id as `responded_by`, hence `$2`).
    if let Err(error) = sqlx::query(
        "UPDATE workspace_join_requests SET status = 'DENIED', responded_at = $1, \
         responded_by_id = $2, updated_at = $1, updated_by_id = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(actor.id)
    .bind(id)
    .execute(pool)
    .await
    {
        if is_integrity_error(&error) {
            return json_response(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_BODY);
        }
        return Denial::ServerError.into_response();
    }
    (
        StatusCode::OK,
        Json(message_envelope(REQUEST_DENIED_MESSAGE)),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// One owned path: the listed methods serve from Rust, everything else
/// falls through to Django (its 405s and DRF metadata live there).
/// OPTIONS proxies too: DRF answers metadata (401 anon / 200 authed)
/// where axum would 405.
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Invite/join/join-request routes (`app/urls/workspace.py:67-111`).
/// Sibling D-24 handler files expose their own `routes()`; the module
/// `routes()` merges them (merges keep both sides).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/workspaces/{slug}/invitations/",
            owned(
                get(invite_list).post(invite_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/invitations/{pk}/",
            owned(
                get(invite_retrieve)
                    .patch(invite_patch)
                    .delete(invite_destroy),
                &["POST", "PUT", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/workspaces/invitations/",
            owned(
                get(my_invites_list).post(my_invites_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/invitations/{pk}/join/",
            owned(
                get(join_get).post(join_post),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/users/me/workspaces/join-requests/",
            owned(
                get(user_join_requests_list).post(user_join_requests_create),
                &["PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/join-requests/",
            owned(
                get(admin_join_requests_list),
                &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/join-requests/{pk}/approve/",
            owned(
                post(join_request_approve),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/workspaces/{slug}/join-requests/{pk}/deny/",
            owned(
                post(join_request_deny),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_ROUTES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/app_workspace/handlers/routes.golden.json"
    );

    fn routes_fixture() -> Value {
        let raw = std::fs::read_to_string(FIXTURE_ROUTES).expect("fixture exists");
        serde_json::from_str(&raw).expect("fixture is valid JSON")
    }

    fn fixture_error_bodies() -> Vec<(u16, Value)> {
        routes_fixture()["errors"]
            .as_array()
            .expect("errors is a list")
            .iter()
            .map(|entry| {
                (
                    entry["status"].as_u64().expect("status") as u16,
                    entry["body"].clone(),
                )
            })
            .collect()
    }

    fn assert_error_pinned(status: u16, body: &str) {
        let body: Value = serde_json::from_str(body).expect("test body is JSON");
        assert!(
            fixture_error_bodies().contains(&(status, body.clone())),
            "missing F-W24-15 error: {status} {body}"
        );
    }

    // --- F-W24-15: these routes (W04-W11) -----------------------------------

    #[test]
    fn route_table_pins_all_eight_paths() {
        let fixture = routes_fixture();
        let routes = fixture["routes"].as_array().expect("routes is a list");
        let texts: Vec<&str> = routes.iter().filter_map(Value::as_str).collect();
        for want in [
            "W04 GET+POST workspaces/<slug>/invitations/",
            "W05 workspaces/<slug>/invitations/<pk>/",
            "W06 GET+POST users/me/workspaces/invitations/",
            "W07 GET+POST workspaces/<slug>/invitations/<pk>/join/",
            "W08 GET+POST users/me/workspaces/join-requests/",
            "W09 GET workspaces/<slug>/join-requests/",
            "W10 POST workspaces/<slug>/join-requests/<pk>/approve/",
            "W11 POST workspaces/<slug>/join-requests/<pk>/deny/",
        ] {
            assert!(
                texts.iter().any(|text| text.contains(want)),
                "missing F-W24-15 route leg: {want}"
            );
        }
    }

    #[test]
    fn error_bodies_match_f24_15() {
        assert_error_pinned(400, EMAILS_REQUIRED_BODY);
        assert_error_pinned(400, HIGHER_ROLE_BODY);
        assert_error_pinned(403, JOIN_FORBIDDEN_BODY);
        assert_error_pinned(400, ALREADY_RESPONDED_INVITE_BODY);
        assert_error_pinned(400, ADMIN_EMAIL_REQUIRED_BODY);
        assert_error_pinned(400, OWN_EMAIL_BODY);
        assert_error_pinned(400, ALREADY_RESPONDED_REQUEST_BODY);
        // The already-member body carries the serialized members; the
        // fixture pins the `error` key with a `"<serialized>"` marker.
        let bodies = fixture_error_bodies();
        assert!(bodies.iter().any(|(status, body)| {
            *status == 400
                && body.get("error").and_then(Value::as_str) == Some(ALREADY_MEMBER_ERROR)
                && body.get("workspace_users").is_some()
        }));
        // The invalid-email body interpolates the offending dict; the
        // fixture pins the template with a `{email}` marker.
        let rendered = format!(
            "Invalid email - {} provided a valid email address is required to send the invite",
            py_repr(&serde_json::json!({"email": "nope", "role": 5}))
        );
        assert_eq!(
            rendered,
            "Invalid email - {'email': 'nope', 'role': 5} provided a valid email address is required to send the invite"
        );
        assert!(bodies.iter().any(|(status, body)| {
            *status == 400
                && body
                    .get("error")
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.starts_with("Invalid email - "))
        }));
    }

    #[test]
    fn not_found_bodies_match_drf_conventions() {
        // Inherited `retrieve` / `get_object_or_404` (`Detail`) vs bare
        // `.get` (`error`) — the two 404 shapes this file serves.
        assert_eq!(
            serde_json::from_str::<Value>(INVITE_NOT_FOUND_BODY).expect("json")["Detail"],
            "No WorkspaceMemberInvite matches the given query."
        );
        assert_eq!(
            serde_json::from_str::<Value>(JOIN_REQUEST_NOT_FOUND_BODY).expect("json")["Detail"],
            "No WorkspaceJoinRequest matches the given query."
        );
        assert_error_pinned_invite_missing_object();
    }

    fn assert_error_pinned_invite_missing_object() {
        assert_eq!(
            serde_json::from_str::<Value>(MISSING_OBJECT_BODY).expect("json")["error"],
            "The required object does not exist."
        );
    }

    // --- Python semantics ----------------------------------------------------

    #[test]
    fn py_int_matches_builtin() {
        let cases: &[(&str, Result<i128, ()>)] = &[
            ("5", Ok(5)),
            ("\"20\"", Ok(20)),
            ("\"  -20  \"", Ok(-20)),
            ("\"+7\"", Ok(7)),
            ("\"2_0\"", Ok(20)),
            ("true", Ok(1)),
            ("false", Ok(0)),
            ("5.9", Ok(5)),
            ("-5.9", Ok(-5)),
            // `int()` strips Unicode whitespace — except `\x1c`-`\x1f`
            // (probed; `str.strip()` takes those too).
            ("\"\\u00a05\"", Ok(5)),
            ("\"\\u00855\"", Ok(5)),
            ("\"\\u001c5\"", Err(())),
            ("\"5\\u001f\"", Err(())),
            ("\"\"", Err(())),
            ("\"20.0\"", Err(())),
            ("\"0x10\"", Err(())),
            ("\"abc\"", Err(())),
            ("\"_5\"", Err(())),
            ("\"5_\"", Err(())),
            ("\"5__5\"", Err(())),
            ("null", Err(())),
            ("[]", Err(())),
            ("{}", Err(())),
        ];
        for (raw, want) in cases {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(py_int(&value), *want, "py_int({raw})");
        }
        // Huge magnitudes saturate by sign: positive still 400s the cap,
        // negative still 500s the column.
        let huge_pos: Value = serde_json::from_str("\"99999999999999999999999999\"").expect("json");
        assert!(py_int(&huge_pos).expect("parses") > i128::from(20));
        let huge_neg: Value =
            serde_json::from_str("\"-99999999999999999999999999\"").expect("json");
        assert!(py_int(&huge_neg).expect("parses") < i128::from(i16::MIN));
        // Huge JSON number literals: within f64 range they saturate
        // through the float arm; past f64 range a digit-only literal is
        // still a Python int (saturate by sign), while a float literal
        // is `OverflowError` (500).
        let wide: Value = serde_json::from_str("1267650600228229401496703205376").expect("json");
        assert!(py_int(&wide).expect("parses") > i128::from(20));
        let past_f64: Value = serde_json::from_str(&format!("1{}", "0".repeat(400))).expect("json");
        assert_eq!(py_int(&past_f64), Ok(i128::MAX));
        let past_f64_neg: Value =
            serde_json::from_str(&format!("-1{}", "0".repeat(400))).expect("json");
        assert_eq!(py_int(&past_f64_neg), Ok(-i128::MAX));
        let inf_lit: Value = serde_json::from_str("1e999").expect("json");
        assert_eq!(py_int(&inf_lit), Err(()));
    }

    #[test]
    fn role_cap_mirrors_queries_predicate() {
        // The handler widens [`queries::invite_role_cap_blocks`] to `i128`;
        // every in-range pair must agree with it (equal roles pass).
        for invited in [0, 5, 15, 19, 20, 21, 100] {
            for requester in [5, 15, 20] {
                assert_eq!(
                    i128::from(invited) > i128::from(requester),
                    queries::invite_role_cap_blocks(invited, requester),
                    "invited={invited} requester={requester}"
                );
            }
        }
    }

    #[test]
    fn py_truthy_matches_builtin() {
        let cases: &[(&str, bool)] = &[
            ("null", false),
            ("false", false),
            ("true", true),
            ("0", false),
            ("1", true),
            ("0.0", false),
            ("0.5", true),
            ("\"\"", false),
            ("\"false\"", true),
            ("[]", false),
            ("[0]", true),
            ("{}", false),
            ("{\"a\": 1}", true),
            // ±inf literals (`as_f64` filters non-finite, so `None`
            // means inf — and Python `bool(inf)` is `True`).
            ("1e999", true),
            ("-1e999", true),
        ];
        for (raw, want) in cases {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(py_truthy(&value), *want, "py_truthy({raw})");
        }
        // A digit-only literal past f64 range is a nonzero Python int.
        let huge: Value = serde_json::from_str(&format!("1{}", "0".repeat(400))).expect("json");
        assert!(py_truthy(&huge));
    }

    #[test]
    fn py_repr_matches_builtin() {
        // Insertion order survives (the `preserve_order` feature).
        let mut map = Map::new();
        map.insert("email".to_owned(), Value::String("nope".to_owned()));
        map.insert("role".to_owned(), Value::Number(5.into()));
        assert_eq!(py_repr(&Value::Object(map)), "{'email': 'nope', 'role': 5}");
        let cases: &[(&str, &str)] = &[
            ("null", "None"),
            ("true", "True"),
            ("false", "False"),
            ("5", "5"),
            ("\"x\"", "'x'"),
            ("\"a'b\"", "\"a'b\""),
            ("\"a'b\\\"c\"", "'a\\'b\"c'"),
            ("\"a\\nb\"", "'a\\nb'"),
            ("\"café\"", "'café'"),
            ("[]", "[]"),
            ("{}", "{}"),
            ("[1, \"x\", null]", "[1, 'x', None]"),
            ("{\"a\": [1]}", "{'a': [1]}"),
            ("1e16", "1e+16"),
            ("1e999", "inf"),
        ];
        for (raw, want) in cases {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(py_repr(&value), *want, "py_repr({raw})");
        }
    }

    #[test]
    fn py_float_repr_matches_python_exponents() {
        for (input, want) in [
            (5.0, "5.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (-2.5, "-2.5"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (1.5e-5, "1.5e-05"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1759490000.123456, "1759490000.123456"),
            (123456.789, "123456.789"),
        ] {
            assert_eq!(py_float_repr(input), want, "py_float_repr({input})");
        }
    }

    #[test]
    fn py_num_str_matches_python_str() {
        let wide = "1606938044258990275541962092341162602522202993782792835301376"; // 2**200
        let cases: &[(&str, &str)] = &[
            ("5", "5"),
            ("-0", "0"),
            ("5.0", "5.0"),
            ("1e16", "1e+16"),
            ("0.000001", "1e-06"),
            ("1E5", "100000.0"),
            ("1e999", "inf"),
            ("-1e999", "-inf"),
            (wide, wide),
        ];
        for (raw, want) in cases {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            let n = value.as_number().expect("number");
            assert_eq!(py_num_str(n), *want, "py_num_str({raw})");
        }
        // A digit-only literal past f64 range is a Python int: exact digits.
        let big = format!("1{}", "0".repeat(400));
        let value: Value = serde_json::from_str(&big).expect("json");
        let n = value.as_number().expect("number");
        assert_eq!(py_num_str(n), big);
    }

    #[test]
    fn django_str_now_omits_zero_micros() {
        let with_micros =
            chrono::DateTime::parse_from_rfc3339("2026-10-03T12:09:22.658721Z").expect("time");
        assert_eq!(
            django_str_now(&with_micros.with_timezone(&Utc)),
            "2026-10-03 12:09:22.658721+00:00"
        );
        let sharp = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").expect("time");
        assert_eq!(
            django_str_now(&sharp.with_timezone(&Utc)),
            "2026-01-01 00:00:00+00:00"
        );
    }

    #[test]
    fn patch_now_resolves_microseconds_like_django() {
        // `auto_now` binds (and echoes) microsecond clock values: a raw
        // `Utc::now()` would render 9 fraction digits where DRF renders 6.
        for _ in 0..100 {
            let now = patch_now();
            assert_eq!(now.timestamp_subsec_nanos() % 1000, 0, "{now:?}");
            let rendered = crate::serializer::render_datetime_in(&now, &chrono_tz::UTC);
            let fraction = rendered
                .split(['T'])
                .nth(1)
                .expect("time part")
                .split(['Z', '+', '-'])
                .next()
                .expect("fraction part");
            match fraction.split_once('.') {
                None => {}
                Some((_, digits)) => assert_eq!(digits.len(), 6, "{rendered}"),
            }
        }
    }

    #[test]
    fn email_classification_matches_django() {
        assert!(matches!(classify_email(None), EmailCheck::Invalid));
        // Falsy input → `ValidationError` (400), probed live.
        for raw in ["null", "\"\"", "0", "0.0", "false", "[]", "{}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(classify_email(Some(&value)), EmailCheck::Invalid),
                "classify_email({raw})"
            );
        }
        // Containers without `"@"` → 400; with it → `.rsplit`
        // `AttributeError` (500). Dict lookup is by key.
        for raw in ["[\"x\"]", "[[]]", "{\"a\": \"@\"}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(classify_email(Some(&value)), EmailCheck::Invalid),
                "classify_email({raw})"
            );
        }
        // Truthy numbers/bools → `TypeError` (500); containers holding
        // `"@"` → `AttributeError` (500).
        for raw in ["5", "5.0", "true", "[\"@\"]", "{\"@\": 1}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(classify_email(Some(&value)), EmailCheck::TypeError),
                "classify_email({raw})"
            );
        }
        assert!(matches!(
            classify_email(Some(&serde_json::json!("a@b.com"))),
            EmailCheck::Candidate(_)
        ));
        assert!(is_valid_email_str("a@b.com"));
        assert!(!is_valid_email_str("nope"));
        assert!(!is_valid_email_str(""));
    }

    // --- PATCH validation (probed live against DRF 3.15.2) --------------------

    #[test]
    fn patch_role_matches_choice_field() {
        let cases: &[(&str, Result<i16, &str>)] = &[
            ("5", Ok(5)),
            ("\"5\"", Ok(5)),
            ("5.0", Err("\"5.0\" is not a valid choice.")),
            ("true", Err("\"True\" is not a valid choice.")),
            ("\"abc\"", Err("\"abc\" is not a valid choice.")),
            ("\"\"", Err("\"\" is not a valid choice.")),
            ("null", Err("This field may not be null.")),
            ("[]", Err("\"[]\" is not a valid choice.")),
            ("{}", Err("\"{}\" is not a valid choice.")),
            ("7", Err("\"7\" is not a valid choice.")),
            ("\" 5\"", Err("\" 5\" is not a valid choice.")),
            ("1e16", Err("\"1e+16\" is not a valid choice.")),
            ("1e999", Err("\"inf\" is not a valid choice.")),
        ];
        for (raw, want) in cases {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            let got = validate_patch_role(&value);
            match want {
                Ok(role) => assert_eq!(got, Ok(*role), "role({raw})"),
                Err(message) => assert_eq!(got, Err((*message).to_owned()), "role({raw})"),
            }
        }
    }

    #[test]
    fn patch_accepted_matches_boolean_field() {
        let ok: &[(&str, bool)] = &[
            ("true", true),
            ("false", false),
            ("1", true),
            ("0", false),
            ("\"true\"", true),
            ("\"false\"", false),
            ("\"yes\"", true),
            ("\"1\"", true),
            ("\"0\"", false),
            // Set membership by numeric equality: `1.0` is true.
            ("1.0", true),
            ("0.0", false),
        ];
        for (raw, want) in ok {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(
                validate_patch_accepted(&value),
                Ok(*want),
                "accepted({raw})"
            );
        }
        let invalid = ["\"\"", "2", "2.0", "\"x\"", "[]", "1.5", "{}"];
        for raw in invalid {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(
                validate_patch_accepted(&value),
                Err("Must be a valid boolean.".to_owned()),
                "accepted({raw})"
            );
        }
        assert_eq!(
            validate_patch_accepted(&Value::Null),
            Err("This field may not be null.".to_owned())
        );
    }

    #[test]
    fn patch_deleted_at_matches_datetime_field() {
        let utc = &chrono_tz::UTC;
        let tokyo = &chrono_tz::Asia::Tokyo;
        let york = &chrono_tz::America::New_York;
        assert_eq!(validate_patch_deleted_at(&Value::Null, utc), Ok(None));
        // (input, timezone, expected UTC instant): the `fromisoformat` ∪
        // regex union, all probed live against Django 4.2.30 / CPython
        // 3.12. Naive inputs attach the request timezone.
        let parsed = |raw: &str, tz: &chrono_tz::Tz| {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            validate_patch_deleted_at(&value, tz)
                .expect("parses")
                .expect("some")
                .to_rfc3339()
        };
        for (raw, tz, want) in [
            ("\"2026-01-01T00:00:00Z\"", utc, "2026-01-01T00:00:00+00:00"),
            // Naive attaches the request tz (identity under UTC).
            ("\"2026-01-01 00:00:00\"", utc, "2026-01-01T00:00:00+00:00"),
            (
                "\"2026-01-02T03:04:05\"",
                tokyo,
                "2026-01-01T18:04:05+00:00",
            ),
            ("\"2026-01-02\"", tokyo, "2026-01-01T15:00:00+00:00"),
            (
                "\"2026-01-02T03:04:05Z\"",
                tokyo,
                "2026-01-02T03:04:05+00:00",
            ),
            // Any-char separator, basic/week/date-only shapes.
            ("\"2026-01-02t03:04:05\"", utc, "2026-01-02T03:04:05+00:00"),
            ("\"2026-01-02X03:04:05\"", utc, "2026-01-02T03:04:05+00:00"),
            ("\"20260102T030405\"", utc, "2026-01-02T03:04:05+00:00"),
            ("\"2026-01-02T03\"", utc, "2026-01-02T03:00:00+00:00"),
            ("\"2026-01-02T0304\"", utc, "2026-01-02T03:04:00+00:00"),
            ("\"2026-W05-6\"", utc, "2026-01-31T00:00:00+00:00"),
            ("\"2026W056\"", utc, "2026-01-31T00:00:00+00:00"),
            ("\"2026-01-02\"", utc, "2026-01-02T00:00:00+00:00"),
            // Fractions: any length truncated to 6.
            (
                "\"2026-01-02T03:04:05.1234567890123Z\"",
                utc,
                "2026-01-02T03:04:05.123456+00:00",
            ),
            (
                "\"2026-01-02T03:04:05.123456789012Z\"",
                utc,
                "2026-01-02T03:04:05.123456+00:00",
            ),
            (
                "\"2026-01-02T03:04:05:06\"",
                utc,
                "2026-01-02T03:04:05.060+00:00",
            ),
            ("\"20260102T03040500\"", utc, "2026-01-02T03:04:05+00:00"),
            // Offsets: seconds, fractions (always seconds fractions),
            // unchecked parts with the total under 24h.
            (
                "\"2026-01-02T03:04:05+05:00:00\"",
                utc,
                "2026-01-01T22:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+000061\"",
                utc,
                "2026-01-02T03:03:04+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+05:00:00.5\"",
                utc,
                "2026-01-01T22:04:04.500+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+00:61\"",
                utc,
                "2026-01-02T02:03:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+05\"",
                utc,
                "2026-01-01T22:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+0500\"",
                utc,
                "2026-01-01T22:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+05.5\"",
                utc,
                "2026-01-01T22:04:04.500+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+050000123\"",
                utc,
                "2026-01-01T22:04:04.877+00:00",
            ),
            (
                "\"2026-01-02T03:04:05+00\"",
                utc,
                "2026-01-02T03:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05-00\"",
                utc,
                "2026-01-02T03:04:05+00:00",
            ),
            // Regex arm: unpadded parts, `\s*` gaps, trailing newline,
            // empty fraction ahead of a tz.
            ("\"2026-1-2T3:04+05:30\"", utc, "2026-01-01T21:34:00+00:00"),
            (
                "\"2026-1-2T3:04:05+00:61\"",
                utc,
                "2026-01-02T02:03:05+00:00",
            ),
            ("\"2026-01-02T03:04:05 \"", utc, "2026-01-02T03:04:05+00:00"),
            (
                "\"2026-01-02T03:04:05  +05:00\"",
                utc,
                "2026-01-01T22:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05Z\\n\"",
                utc,
                "2026-01-02T03:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05.+05:00\"",
                utc,
                "2026-01-01T22:04:05+00:00",
            ),
            (
                "\"2026-01-02T03:04:05.Z\"",
                utc,
                "2026-01-02T03:04:05+00:00",
            ),
            (
                "\"20260102T030405.123+05:00\"",
                utc,
                "2026-01-01T22:04:05.123+00:00",
            ),
            // Folds attach fold-0 without failing (probed live).
            ("\"2026-11-01T01:30:00\"", york, "2026-11-01T05:30:00+00:00"),
            ("\"2026-03-08T02:30:00\"", york, "2026-03-08T07:30:00+00:00"),
            // Year-range boundaries (PIDASHCONV-768): 1 and 9999 still
            // parse — only 0 rejects.
            ("\"0001-01-01\"", utc, "0001-01-01T00:00:00+00:00"),
            ("\"0001-W01-1\"", utc, "0001-01-01T00:00:00+00:00"),
            ("\"9999-12-31\"", utc, "9999-12-31T00:00:00+00:00"),
        ] {
            assert_eq!(parsed(raw, tz), want, "deleted_at({raw})");
        }
        // Wrong-format 400s (both arms reject, or `ValueError`).
        for raw in [
            "\"nope\"",
            "\"\"",
            "5",
            "\"2026-13-45T99:99:99Z\"",
            "\"2026-01-02T03:04:05+05:\"",
            "\"2026-01-02T03:04:05+05::00\"",
            "\"2026-01-02T03:04:05+0:500\"",
            "[]",
            "\"2026-01-02T03:04:05Z \"",
            "\"2026-01-02T03:04:05z\"",
            "\" 2026-01-02T03:04:05\"",
            "\"2026-01-02T3\"",
            "\"2026-01-02T030\"",
            "\"2026-01-02T0304050\"",
            "\"20260102T03:4\"",
            "\"20260102T03:04:5\"",
            "\"2026-01-02T03:04:05+05000\"",
            "\"2026-01-02T03:04:05+0500001\"",
            "\"2026-01-02T03:04:05+050000:12\"",
            "\"2026-01-02T03:04:05+05000000.5\"",
            "\"2026-01-02T03:04:05.\"",
            "\"2026-01-02T03:04:05+05:00:00:00:00\"",
            "\"2026-01-02T03:04:05+5\"",
            "\"2026-01-02T03:04:05+053\"",
            "\"2026-01-02Z\"",
            "\"2026-01-02T\"",
            "\"2026-1-2T3:04:05.1234567890123\"",
            "\"2026-1-2T3:04:05.+05:00\"",
            "\"2026-01-02T03:04:05+24:00\"",
            "\"2026-01-02T03:04:05+99:99\"",
            "\"2026-1-2T03:04:05+24:00\"",
            "\"2026-01-02T03:04:05:06.5\"",
            "\"2026-01-02T03:04:05.5xyz\"",
            // Year 0 (PIDASHCONV-768): CPython raises (`ValueError: year
            // 0 is out of range`) in both arms — `fromisoformat`, and
            // the regex fallback's `datetime(0, ...)` — so Django lands
            // in the invalid arm while chrono's proleptic year 0 would
            // accept (same battery as PIDASHCONV-762).
            "\"0000-01-01\"",
            "\"00000101\"",
            "\"0000-01-01T00:00:00\"",
            "\"0000-01-01 00:00:00\"",
            "\"0000-01-01T00:00:00+00:00\"",
            "\"0000-01-01T00:00:00Z\"",
            "\"0000-W01-1\"",
            "\"0000-W01\"",
            "\"0000W011\"",
            "\"0000W01\"",
            "\"0000-W01-1T00:00:00+00:00\"",
            "\"0000-1-2T03:04:05\"",
            "\"0000-01-02T03:04:05\"",
            "\"0000-1-2 03:04:05+05:00\"",
        ] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(
                validate_patch_deleted_at(&value, utc),
                Err(PatchDtError::Invalid(DATETIME_INVALID_MESSAGE.to_owned())),
                "deleted_at({raw})"
            );
        }
        // Aware overflow is the `overflow` 400 ...
        let value: Value =
            serde_json::from_str("\"9999-12-31T23:30:00+00:00\"").expect("case is JSON");
        assert_eq!(
            validate_patch_deleted_at(&value, tokyo),
            Err(PatchDtError::Invalid(DATETIME_OVERFLOW_MESSAGE.to_owned())),
            "deleted_at(overflow)"
        );
        // ... while a naive wall converting past the range is a raw
        // `OverflowError` → 500 (probed live).
        for (raw, tz) in [
            ("\"0001-01-01T00:30:00\"", tokyo),
            ("\"9999-12-31T23:30:00\"", york),
        ] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert_eq!(
                validate_patch_deleted_at(&value, tz),
                Err(PatchDtError::ServerError),
                "deleted_at({raw})"
            );
        }
    }

    #[test]
    fn patch_non_dict_bodies_match_serializer() {
        for (raw, kind) in [
            ("[]", "list"),
            ("\"x\"", "str"),
            ("5", "int"),
            ("5.5", "float"),
            ("true", "bool"),
        ] {
            let errors = non_dict_errors(kind);
            assert_eq!(
                errors["non_field_errors"][0],
                format!("Invalid data. Expected a dictionary, but got {kind}."),
                "non-dict({raw})"
            );
        }
    }

    #[test]
    fn non_dict_number_kind_matches_python_type() {
        // `json_number_kind` is the `type(data).__name__` arm inside
        // `validate_invite_patch` (async/DB-bound, so pinned here at the
        // helper — the table above feeds the kind in directly and cannot
        // catch a range-based misreport).
        for (raw, want) in [
            ("5", "int"),
            ("-5", "int"),
            ("0", "int"),
            ("18446744073709551616", "int"),
            ("340282366920938463463374607431768211455", "int"),
            ("5.0", "float"),
            ("5.5", "float"),
            ("1e3", "float"),
            ("1E3", "float"),
            ("-2.5e-3", "float"),
        ] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            let Value::Number(n) = &value else {
                panic!("case is a number: {raw}");
            };
            assert_eq!(json_number_kind(n), want, "kind({raw})");
        }
    }

    #[test]
    fn uuid_hex_matches_python_uuid() {
        // `parse_uuid_hex` is the `UUID(hex=)` arm shared by PATCH-user
        // validation (async/DB-bound) and my-accept prep — pinned here
        // at the helper, plus through `classify_invitations` below. All
        // cases probed live on CPython 3.12.
        let canonical =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("canonical");
        for raw in [
            "550e8400-e29b-41d4-a716-446655440000",
            "550e8400e29b41d4a716446655440000",
            "550e8400e29b-41d4-a716-446655440000",
            "5-50e8400e29b41d4a716446655440000",
            "550e8400e29b--41d4-a716-446655440000",
            "{550e8400-e29b-41d4-a716-446655440000}",
            "{550e8400e29b41d4a716446655440000",
            "550e8400e29b41d4a716446655440000}",
            "urn:uuid:550e8400-e29b-41d4-a716-446655440000",
            "uuid:550e8400-e29b-41d4-a716-446655440000",
            "550e8400urn:e29b41d4a716446655440000",
            "550E8400E29B41D4A716446655440000",
        ] {
            assert_eq!(parse_uuid_hex(raw), Some(canonical), "uuid({raw})");
        }
        // `int(_, 16)` leniency: the value is the stripped 31-hex int,
        // zero-padded on render.
        let shifted =
            uuid::Uuid::parse_str("0550e840-0e29-b41d-4a71-644665544000").expect("shifted");
        for raw in [
            " 550e8400e29b41d4a71644665544000",
            "550e8400e29b41d4a71644665544000 ",
            "+550e8400e29b41d4a71644665544000",
            "550e8400_e29b41d4a71644665544000",
            "\t550e8400e29b41d4a71644665544000",
        ] {
            assert_eq!(parse_uuid_hex(raw), Some(shifted), "uuid({raw:?})");
        }
        for raw in [
            "",
            "550e8400-e29b-41d4-a716-44665544000",
            "550e8400-e29b-41d4-a716-4466554400000",
            "-550e8400e29b41d4a71644665544000",
            "550e8400e29b41d4a71644665544__00",
            "URN:UUID:550E8400-E29B-41D4-A716-446655440000",
            "550e8400e29b41d4a7164466554400g",
            "------------------------------------",
        ] {
            assert_eq!(parse_uuid_hex(raw), None, "uuid({raw:?})");
        }
    }

    #[test]
    fn invitations_uuid_leniency_matches_python() {
        // Shifted-hyphen ids ride prep (Django 204s where strict
        // parsing 400s — probed live).
        let value: Value = serde_json::from_str(
            "[\"550e8400e29b-41d4-a716-446655440000\", \"550e8400_e29b41d4a71644665544000\"]",
        )
        .expect("case is JSON");
        match classify_invitations(Some(&value)) {
            InvitationsInput::Ids(ids) => assert_eq!(ids.len(), 2),
            InvitationsInput::InvalidUuid => panic!("expected ids, got InvalidUuid"),
            InvitationsInput::ServerError => panic!("expected ids, got ServerError"),
        }
    }

    // --- Join-request input shaping -------------------------------------------

    #[test]
    fn invitations_input_matches_queryset_prep() {
        let good = uuid::Uuid::new_v4().to_string();
        // Missing → empty (204 no-op).
        assert!(matches!(classify_invitations(None), InvitationsInput::Ids(ids) if ids.is_empty()));
        // Null / non-iterables → 500.
        for raw in ["null", "5", "5.5", "true"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(
                    classify_invitations(Some(&value)),
                    InvitationsInput::ServerError
                ),
                "invitations({raw})"
            );
        }
        // Unparseable UUIDs → 400.
        for raw in [
            "[\"nope\"]",
            "[5.5]",
            "[[]]",
            "[{}]",
            "\"abc\"",
            "{\"k\": 1}",
        ] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(
                    classify_invitations(Some(&value)),
                    InvitationsInput::InvalidUuid
                ),
                "invitations({raw})"
            );
        }
        // Empty string / empty dict iterate to nothing (204 no-op).
        for raw in ["\"\"", "{}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(classify_invitations(Some(&value)), InvitationsInput::Ids(ids) if ids.is_empty()),
                "invitations({raw})"
            );
        }
        // Valid ids query (nulls skipped, bools/ints ride `int=`).
        let value: Value =
            serde_json::from_str(&format!("[\"{good}\", null, true, 5]")).expect("case is JSON");
        match classify_invitations(Some(&value)) {
            InvitationsInput::Ids(ids) => assert_eq!(ids.len(), 3),
            InvitationsInput::InvalidUuid => panic!("expected ids, got InvalidUuid"),
            InvitationsInput::ServerError => panic!("expected ids, got ServerError"),
        }
        // Negative ints fail prep → 400.
        let value: Value = serde_json::from_str("[-1]").expect("case is JSON");
        assert!(matches!(
            classify_invitations(Some(&value)),
            InvitationsInput::InvalidUuid
        ));
        // `uuid.UUID(int=)` takes any `0 <= i < 2**128`.
        let value: Value = serde_json::from_str(
            "[18446744073709551616, 1267650600228229401496703205376, 340282366920938463463374607431768211455]",
        )
        .expect("case is JSON");
        match classify_invitations(Some(&value)) {
            InvitationsInput::Ids(ids) => assert_eq!(ids.len(), 3),
            InvitationsInput::InvalidUuid => panic!("expected ids, got InvalidUuid"),
            InvitationsInput::ServerError => panic!("expected ids, got ServerError"),
        }
        // `2**128` and negative-huge fail prep → 400.
        for raw in [
            "[340282366920938463463374607431768211456]",
            "[-1180591620717411303424]",
        ] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(
                    classify_invitations(Some(&value)),
                    InvitationsInput::InvalidUuid
                ),
                "invitations({raw})"
            );
        }
    }

    #[test]
    fn emails_input_matches_required_check() {
        // Missing/null/falsy → the emails-required 400.
        assert!(matches!(classify_emails_input(None), EmailsInput::Missing));
        for raw in ["null", "\"\"", "0", "0.0", "false", "[]", "{}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(classify_emails_input(Some(&value)), EmailsInput::Missing),
                "emails({raw})"
            );
        }
        // A non-empty list reaches the role loop.
        let value: Value = serde_json::from_str("[{\"email\": \"a@b.co\"}]").expect("case is JSON");
        assert!(matches!(
            classify_emails_input(Some(&value)),
            EmailsInput::Entries(_)
        ));
        // A truthy non-list 500s in the iteration.
        for raw in ["\"x\"", "5", "5.5", "true", "{\"a\": 1}"] {
            let value: Value = serde_json::from_str(raw).expect("case is JSON");
            assert!(
                matches!(
                    classify_emails_input(Some(&value)),
                    EmailsInput::ServerError
                ),
                "emails({raw})"
            );
        }
    }

    #[test]
    fn audit_columns_match_base_model_save() {
        let caller = Some(uuid::Uuid::new_v4());
        let created = Some(uuid::Uuid::new_v4());
        // Authed update: stamp `updated_by`, keep `created_by`.
        assert_eq!(audit_columns_on_update(caller, created), (caller, created));
        assert_eq!(audit_columns_on_update(caller, None), (caller, None));
        // Anonymous update: null both.
        assert_eq!(audit_columns_on_update(None, created), (None, None));
        assert_eq!(audit_columns_on_update(None, None), (None, None));
    }

    #[test]
    fn forward_fk_traversal_ignores_workspace_scope() {
        // The gates and fetches traverse `workspace__slug`, which Django
        // does not scope to live workspaces — while the member/invite/
        // request-side scopes stay.
        for sql in [
            ADMIN_GATE_SQL,
            OWNER_GATE_SQL,
            FETCH_INVITE_SQL,
            FETCH_JOIN_REQUEST_SQL,
        ] {
            assert!(
                !sql.contains("w.deleted_at"),
                "workspace scope leaked into traversal SQL: {sql}"
            );
        }
        assert!(ADMIN_GATE_SQL.contains("wm.deleted_at IS NULL"));
        assert!(OWNER_GATE_SQL.contains("wm.deleted_at IS NULL"));
        assert!(FETCH_INVITE_SQL.contains("i.deleted_at IS NULL"));
        assert!(FETCH_JOIN_REQUEST_SQL.contains("jr.deleted_at IS NULL"));
    }

    #[test]
    fn message_coercion_matches_text_field() {
        assert!(matches!(coerce_message(None), MessageValue::Null));
        assert!(matches!(
            coerce_message(Some(&Value::Null)),
            MessageValue::Null
        ));
        match coerce_message(Some(&serde_json::json!("hi"))) {
            MessageValue::Text(text) => assert_eq!(text, "hi"),
            _ => panic!("string stores"),
        }
        match coerce_message(Some(&serde_json::json!(5))) {
            MessageValue::Text(text) => assert_eq!(text, "5"),
            _ => panic!("int stores as text"),
        }
        match coerce_message(Some(&serde_json::json!(true))) {
            MessageValue::Text(text) => assert_eq!(text, "True"),
            _ => panic!("bool stores as text"),
        }
        match coerce_message(Some(&serde_json::json!(false))) {
            MessageValue::Text(text) => assert_eq!(text, "False"),
            _ => panic!("bool stores as text"),
        }
        // `str()` of a container is its repr — stored, endpoint 201s.
        match coerce_message(Some(&serde_json::json!({"a": 1}))) {
            MessageValue::Text(text) => assert_eq!(text, "{'a': 1}"),
            _ => panic!("dict stores as repr"),
        }
        match coerce_message(Some(&serde_json::json!([1, "a"]))) {
            MessageValue::Text(text) => assert_eq!(text, "[1, 'a']"),
            _ => panic!("list stores as repr"),
        }
        let float: Value = serde_json::from_str("1e16").expect("json");
        match coerce_message(Some(&float)) {
            MessageValue::Text(text) => assert_eq!(text, "1e+16"),
            _ => panic!("float stores as text"),
        }
    }

    // --- JWT -------------------------------------------------------------------

    #[test]
    fn invite_token_shape_matches_pyjwt() {
        // The header segment is the standard HS256 header bytes.
        let token = invite_token(
            b"secret",
            &serde_json::json!({"email": "a@b.com", "role": 5}),
            1.5,
        )
        .expect("encodes");
        let mut segments = token.split('.');
        assert_eq!(
            segments.next(),
            Some("eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9")
        );
        // The payload round-trips the WHOLE dict plus the timestamp (the
        // ported bug), verifiable with the secret (no `exp` claim exists
        // to validate).
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.required_spec_claims.clear();
        let decoded = jsonwebtoken::decode::<Value>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(b"secret"),
            &validation,
        )
        .expect("decodes");
        assert_eq!(
            decoded.claims,
            serde_json::json!({"email": {"email": "a@b.com", "role": 5}, "timestamp": 1.5})
        );
        assert_eq!(queries::INVITE_TOKEN_CLAIMS, &["email", "timestamp"]);
    }

    // --- Consumed fixtures stay green -------------------------------------------

    #[test]
    fn f24_02_invite_wire_order_survives_render() {
        // One row through the real `ser_invite` renderer: keys in
        // `INVITE_WIRE_FIELDS` order, `invite_link` interpolated raw.
        let lite_row = lite_ws::WorkspaceLiteRow {
            name: "Acme",
            slug: "acme",
            id: "11111111-1111-1111-1111-111111111111",
            logo_url: None,
        };
        let workspace = lite_ws::workspace_lite_to_representation(&lite_row);
        let member_row = ser_invite::MemberInviteRow {
            id: "22222222-2222-2222-2222-222222222222",
            workspace,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            deleted_at: None,
            email: "s@example.com",
            accepted: false,
            token: "tok",
            message: None,
            responded_at: None,
            role: 5,
            created_by: None,
            updated_by: None,
        };
        let view = ser_invite::invite_to_representation(&member_row);
        let rendered = serde_json::to_value(&view).expect("serializes");
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_invite::INVITE_WIRE_FIELDS);
        assert_eq!(
            rendered["invite_link"],
            "/workspace-invitations/?invitation_id=22222222-2222-2222-2222-222222222222&slug=acme&token=tok"
        );
    }

    #[test]
    fn f24_02_join_request_shapes_survive_render() {
        let user_lite_row = lite_user::UserLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Other",
            last_name: "User",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "Other User",
        };
        let requester = lite_user::user_lite_to_representation(&user_lite_row);
        let join_row = ser_invite::JoinRequestRow {
            id: "33333333-3333-3333-3333-333333333333",
            workspace: None,
            requester: requester.clone(),
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
            deleted_at: None,
            admin_email: "admin@x.io",
            message: None,
            role: 15,
            status: "PENDING",
            responded_at: None,
            created_by: None,
            updated_by: None,
            responded_by: None,
        };
        let admin = ser_invite::join_request_to_representation(&join_row);
        let rendered = serde_json::to_value(&admin).expect("serializes");
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_invite::JOIN_REQUEST_WIRE_FIELDS);
        // A null workspace renders a present-but-null key (probed live).
        assert!(rendered.get("workspace").is_some_and(Value::is_null));
        let user_row = ser_invite::UserJoinRequestRow {
            id: "33333333-3333-3333-3333-333333333333",
            requester,
            admin_email: "admin@x.io",
            message: None,
            status: "PENDING",
            responded_at: None,
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
        };
        let user = ser_invite::user_join_request_to_representation(&user_row);
        let rendered = serde_json::to_value(&user).expect("serializes");
        let keys: Vec<&str> = rendered
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ser_invite::USER_JOIN_REQUEST_WIRE_FIELDS);
        // `workspace` is ABSENT, not null (anti-enumeration).
        assert!(!rendered
            .as_object()
            .expect("object")
            .contains_key("workspace"));
    }

    #[test]
    fn f24_10_predicates_match_handlers() {
        assert!(queries::join_token_denied("", "tok"));
        assert!(queries::join_token_denied("wrong", "tok"));
        assert!(!queries::join_token_denied("tok", "tok"));
        assert!(queries::join_request_pending_blocks("APPROVED"));
        assert!(queries::join_request_pending_blocks("DENIED"));
        assert!(!queries::join_request_pending_blocks("PENDING"));
        assert_eq!(queries::INVITE_BULK_BATCH_SIZE, 10);
        assert_eq!(queries::JOIN_REQUEST_DEFAULT_ROLE, 15);
    }

    #[test]
    fn f24_13_outcomes_and_invalidations_match() {
        use gates::{GateOutcome, InvalidateAction};
        assert_eq!(gates::outcome_body(GateOutcome::Allow), None);
        assert_eq!(
            gates::outcome_body(GateOutcome::DenyClass),
            Some(gates::CLASS_DENIED_BODY)
        );
        assert_eq!(
            gates::outcome_body(GateOutcome::Unauthenticated),
            Some(gates::ANON_BODY)
        );
        // Join busts 4 keys unconditionally; bulk-accept 2 + per-invite
        // directs; approve 3 after the owner check; deny none.
        assert_eq!(
            gates::invalidations_for(InvalidateAction::JoinPost).len(),
            4
        );
        assert_eq!(
            gates::invalidations_for(InvalidateAction::JoinBulkCreate).len(),
            2
        );
        assert_eq!(
            gates::invalidations_for(InvalidateAction::ApproveJoinRequest).len(),
            3
        );
        assert_eq!(
            gates::bulk_create_direct_key("acme"),
            "/api/workspaces/acme/members/"
        );
        let (key, multiple) = gates::invalidation_key(
            &gates::invalidations_for(InvalidateAction::JoinPost)[1],
            "acme",
            Some("user-1"),
        );
        assert!(multiple);
        assert_eq!(key, "/api/users/me/workspaces/:user-1");
    }

    #[test]
    fn f24_14_emit_shapes_match_call_sites() {
        let invited = tasks::user_invited_event("u", "w", "acme", 5, "now", "s@x.io");
        assert_eq!(invited.task_name(), tasks::TRACK_EVENT_TASK);
        let kwargs = invited.kwargs();
        let keys: Vec<&str> = kwargs.keys().map(String::as_str).collect();
        assert_eq!(keys, tasks::TRACK_EVENT_KWARG_ORDER);
        let props: Vec<&str> = invited
            .event_properties
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(props, tasks::USER_INVITED_PROPS_ORDER);
        let joined = tasks::user_joined_event("u", "w", "acme", 15, "now");
        assert_eq!(joined.event_name, tasks::USER_JOINED_WORKSPACE);
        let approved = tasks::join_request_approved_event("u", "w", "acme", 15, "now");
        assert_eq!(approved.event_name, tasks::USER_JOINED_WORKSPACE);
        let mail = tasks::workspace_invitation_emit("s@x.io", "w", "tok", "site", "me@x.io");
        assert_eq!(mail.task_name(), tasks::WORKSPACE_INVITATION_TASK);
        let arg_keys: Vec<&str> = mail.args_pairs().iter().map(|(k, _)| *k).collect();
        assert_eq!(arg_keys, tasks::WORKSPACE_INVITATION_ARG_ORDER);
    }

    #[test]
    fn positional_placeholder_rule_matches_sched() {
        assert_eq!(
            positional(
                "SELECT * FROM t WHERE a = :now AND b = :now2",
                &["now", "now2"]
            ),
            "SELECT * FROM t WHERE a = $1 AND b = $2"
        );
    }

    #[test]
    fn datetime_basic_week_dash_time_matches_django() {
        // PIDASHCONV-771: `-` after day-less basic `YYYYWww` is the
        // time separator when a valid time follows (Django 6.0.5 /
        // CPython 3.12.3: `2030W23-12:00:00` → 2030-06-03 12:00:00),
        // while a 1-char tail stays invalid (`2030W23-1` → None).
        let expected = ParsedDt::Naive(
            chrono::NaiveDate::from_ymd_opt(2030, 6, 3)
                .expect("date")
                .and_hms_micro_opt(12, 0, 0, 0)
                .expect("time"),
        );
        assert_eq!(parse_django_datetime("2030W23-12:00:00"), Some(expected));
        assert_eq!(parse_django_datetime("2030W23-1"), None);
    }
}
