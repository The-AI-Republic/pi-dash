#![forbid(unsafe_code)]

//! Module link / favorite / user-properties handlers (D-28, stage 5, PIDASHCONV-407).
//!
//! Ports `ModuleLinkViewSet`, `ModuleFavoriteViewSet` and
//! `ModuleUserPropertiesEndpoint` from
//! `apps/api/pi_dash/app/views/module/base.py:762-855`:
//!
//! - `ModuleLinkViewSet` (`:762-790`, `permission_classes =
//!   [ProjectEntityPermission]`): DRF default list/create/retrieve/update/
//!   partial_update/destroy over `ModuleLinkSerializer`
//!   (`serializers/module.py:156-203`) with `perform_create` stamping
//!   `project_id`/`module_id` from the URL. The queryset (`:774-790`) scopes
//!   to the tenant + module, requires an active project membership row and
//!   an unarchived project, orders `-created_at`, and dedupes.
//! - `ModuleFavoriteViewSet` (`:791-824`, `ProjectLitePermission`): custom
//!   `create` (204 empty, duplicate → 400) and `destroy` (204, missing →
//!   404); the default `list` 500s (ported bug — no `serializer_class`).
//! - `ModuleUserPropertiesEndpoint` (`:825-855`,
//!   `@allow_permission([ADMIN, MEMBER, GUEST])`): `get` auto-creates with
//!   model defaults (200), `patch` wholesale-replaces per key (201 quirk)
//!   and 404s without a prior row.
//!
//! Routes (`app/urls/module.py:57-89`): module-links collection
//! (`GET`/`POST`) and detail (`GET`/`PUT`/`PATCH`/`DELETE`),
//! user-favorite-modules collection (`GET`/`POST`) and detail
//! (`DELETE`), user-properties (`GET`/`PATCH`). Every other method on
//! those paths proxies to Django.
//!
//! Fixture ids: FX-MOD-06
//! (`rust-api/fixtures/app_modules/handlers/module_issues_links.golden.json`
//! for links, `favorites_userprops_archive.golden.json` for the rest);
//! link shapes via `pidash_services::app_modules::shape` (FX-MOD-02,
//! PIDASHCONV-330); gates via `super::gates` (FX-MOD-04, PIDASHCONV-379).
//!
//! # Ported bugs (translate, don't redesign — also listed in the PR)
//!
//! * `PATCH` (or `PUT`) without `url` answers 400 `{"error": "Invalid URL
//!   format."}`: `update()` validates unconditionally (`module.py:194-195`).
//! * Duplicate-`url` on update says "URL already exists for this Issue"
//!   (verbatim, `module.py:201`).
//! * `GET user-favorite-modules/` answers 500 after the gate: no
//!   `serializer_class` on the viewset (the queryset would also raise
//!   `FieldError` on `select_related("module")`).
//! * `PATCH` user-properties answers 201, not 200.
//! * Non-dict JSON bodies (and truthy non-string `url` values) die with
//!   `AttributeError` in `to_internal_value` (`module.py:170-176`) → 500.
//! * Favorite `create` accepts a dangling or null `module` id:
//!   `entity_identifier` is a plain nullable UUID, not a FK.
//! * `GET` user-properties for a missing module answers 400 `{"error":
//!   "The payload is not valid"}` (FK `IntegrityError` out of
//!   `get_or_create`), not 404.
//!
//! Ported from `01a93e17216faea7bfc156b0f864cbbe420d1c52`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Router;
use serde_json::{Map, Value};

use pidash_auth::permissions::project::ProjectFacts;
use pidash_auth::permissions::{ROLE_ADMIN, ROLE_MEMBER};
use pidash_services::app_modules::shape;
use pidash_types::{ProjectId, WorkspaceId};

use super::gates;
use super::handlers_modules::{
    actor_user_id, check_gate, empty_response, fetch_allow_facts, gate_roles, json_response,
    json_string, parse_uuid_or_invalid, pool_of, resolve_project_id, shape_link, shift_datetime,
    HandlerResult,
};
use crate::app_issues::Denial;
use crate::state::AppState;

/// Collection path in `app/urls/module.py:57-62` form.
pub const MODULE_LINKS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/";
/// Detail path in `app/urls/module.py:63-74` form.
pub const MODULE_LINK_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/";
/// Favorites collection path in `app/urls/module.py:75-79` form.
pub const FAVORITES_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/";
/// Favorites detail path in `app/urls/module.py:80-84` form.
pub const FAVORITE_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/{module_id}/";
/// User-properties path in `app/urls/module.py:85-89` form.
pub const USER_PROPS_PATH: &str =
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/user-properties/";

/// Owned methods per path (mirroring `urls/module.py`); every other
/// method proxies to Django.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            MODULE_LINKS_PATH,
            axum::routing::get(link_list)
                .post(link_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            MODULE_LINK_PATH,
            axum::routing::get(link_retrieve)
                .put(link_update)
                .patch(link_partial_update)
                .delete(link_destroy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            FAVORITES_PATH,
            axum::routing::get(favorite_list)
                .post(favorite_create)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            FAVORITE_PATH,
            axum::routing::delete(favorite_destroy)
                .get(crate::edge::proxy)
                .put(crate::edge::proxy)
                .patch(crate::edge::proxy)
                .post(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
        .route(
            USER_PROPS_PATH,
            axum::routing::get(user_props_get)
                .patch(user_props_patch)
                .put(crate::edge::proxy)
                .post(crate::edge::proxy)
                .delete(crate::edge::proxy)
                .options(crate::edge::proxy),
        )
}

fn gate_for(method: &str, path: &str) -> &'static gates::Gate {
    &gates::gate_for(method, path)
        .unwrap_or_else(|| panic!("D-28 gate for {method} {path}"))
        .gate
}

// ---------------------------------------------------------------------------
// Permission-class gates (`ProjectEntityPermission` / `ProjectLitePermission`)
// ---------------------------------------------------------------------------

/// Membership facts for one `(user, slug, project)` over the same rows the
/// permission classes read (`app/permissions/project.py:85-143`): the
/// active-row filters and the `workspace__slug=` / `project_id=` scoping
/// are this SQL; the scope check denies facts fetched for another tenant.
/// Shared with the archive handlers (same permission-class family).
pub(crate) async fn fetch_class_facts(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<ProjectFacts, Denial> {
    let project_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT pm.role FROM project_members pm
           JOIN workspaces w ON w.id = pm.workspace_id
           WHERE pm.member_id = $1 AND pm.project_id = $2 AND w.slug = $3
           AND pm.is_active AND pm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let workspace_role: Option<(i16,)> = sqlx::query_as(
        r#"SELECT wm.role FROM workspace_members wm
           JOIN workspaces w ON w.id = wm.workspace_id
           WHERE wm.member_id = $1 AND w.slug = $2
           AND wm.is_active AND wm.deleted_at IS NULL"#,
    )
    .bind(user_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let project_role = project_role.map(|(role,)| i32::from(role));
    let workspace_role = workspace_role.map(|(role,)| i32::from(role));
    Ok(ProjectFacts {
        workspace: WorkspaceId::from(slug),
        project_id: ProjectId::from(project_id.to_string()),
        authenticated: true,
        is_workspace_member: workspace_role.is_some(),
        has_workspace_admin_or_member: workspace_role
            .is_some_and(|role| role == ROLE_ADMIN || role == ROLE_MEMBER),
        is_workspace_admin: workspace_role.is_some_and(|role| role == ROLE_ADMIN),
        is_project_member: project_role.is_some(),
        is_project_admin: project_role.is_some_and(|role| role == ROLE_ADMIN),
        has_project_admin_or_member: project_role
            .is_some_and(|role| role == ROLE_ADMIN || role == ROLE_MEMBER),
        has_identifier_membership: false,
        has_project_identifier: false,
    })
}

/// Run one `super::gates` matrix row: `Allow` runs the body, `Deny`
/// answers [`gates::deny_body`] (DRF-default lowercase 403 for the
/// permission-class family, decorator body for `@allow_permission`).
/// Shared with the archive handlers.
#[allow(clippy::result_large_err)]
pub(crate) fn check_class_gate(
    gate: &gates::Gate,
    method: &str,
    slug: &str,
    facts: &ProjectFacts,
) -> Result<(), axum::response::Response> {
    match gates::decide_class_gate(gate, method, &gates::tenant_context(slug), facts) {
        gates::GateOutcome::Allow => Ok(()),
        gates::GateOutcome::Deny => Err(json_response(
            StatusCode::FORBIDDEN,
            gates::deny_body(gate).to_owned(),
        )),
        // Unreachable (auth always runs first); the merged 401 matches
        // live Django byte for byte.
        gates::GateOutcome::Unauthenticated => Err(json_response(
            StatusCode::UNAUTHORIZED,
            gates::ANON_BODY.to_owned(),
        )),
    }
}

/// [`actor_user_id`] with the merged 401 body (lowercase `detail`,
/// byte-identical to live Django).
#[allow(clippy::result_large_err)]
pub(crate) fn actor_or_401(
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<uuid::Uuid, axum::response::Response> {
    actor_user_id(extension).map_err(|denial| denial.into_response())
}

/// [`resolve_project_id`] with denials rendered as responses (the merged
/// lowercase rewrite-miss body matches live Django).
pub(crate) async fn resolve_project_or_404(
    pool: &sqlx::PgPool,
    slug: &str,
    raw: &str,
) -> Result<uuid::Uuid, axum::response::Response> {
    resolve_project_id(pool, slug, raw)
        .await
        .map_err(|denial| denial.into_response())
}

/// `handle_exception`'s `IntegrityError` branch
/// (`app/views/base.py:120-124`): 400 `{"error": "The payload is not
/// valid"}`.
fn payload_denial() -> Denial {
    Denial::BadError("The payload is not valid".to_owned())
}

/// Map a write error to the `IntegrityError` 400 when Postgres reports
/// an integrity-constraint violation (`23xxx`: not-null, FK, unique,
/// check — all surface as Django's `IntegrityError`); anything else is a
/// 500 like Django's uncaught database errors.
fn integrity_denial(error: &sqlx::Error) -> Denial {
    if let sqlx::Error::Database(db_error) = error {
        if db_error
            .code()
            .as_deref()
            .is_some_and(|code| code.starts_with("23"))
        {
            return payload_denial();
        }
    }
    Denial::ServerError
}

// ---------------------------------------------------------------------------
// Module links (`ModuleLinkViewSet`, `base.py:762-788`)
// ---------------------------------------------------------------------------

/// Fetch link rows over the viewset queryset (`base.py:774-788`): tenant +
/// module scope, live rows, an active project-membership row for the caller
/// (a JOIN in the Python, so without the soft-delete filter — redundant
/// past the gate, which is stricter), an unarchived project, `-created_at`
/// order. The Python's `.distinct()` only dedupes the membership fanout,
/// which the `EXISTS` form never produces: same rows.
async fn fetch_link_rows(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    pk: Option<&uuid::Uuid>,
) -> Result<Vec<Map<String, Value>>, Denial> {
    use sqlx::Row;
    let mut sql = String::from(
        "SELECT row_to_json(__r)::text AS __row FROM (
           SELECT ml.id, ml.created_at, ml.updated_at, ml.deleted_at, ml.title, ml.url,
                  ml.metadata, ml.created_by_id AS created_by, ml.updated_by_id AS updated_by,
                  ml.project_id AS project, ml.workspace_id AS workspace,
                  ml.module_id AS module
           FROM module_links ml
           JOIN workspaces w ON w.id = ml.workspace_id
           JOIN projects p ON p.id = ml.project_id
           WHERE w.slug = $1 AND ml.project_id = $2 AND ml.module_id = $3
           AND ml.deleted_at IS NULL AND p.archived_at IS NULL
           AND EXISTS (SELECT 1 FROM project_members pm
                       WHERE pm.project_id = ml.project_id
                       AND pm.member_id = $4 AND pm.is_active)",
    );
    if pk.is_some() {
        sql.push_str(" AND ml.id = $5");
    }
    sql.push_str(" ORDER BY ml.created_at DESC) AS __r");
    let mut query = sqlx::query(&sql)
        .bind(slug)
        .bind(project_id)
        .bind(module_id)
        .bind(user_id);
    if let Some(pk) = pk {
        query = query.bind(pk);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
        match value {
            Value::Object(map) => out.push(map),
            _ => return Err(Denial::ServerError),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Link field validation (`ModuleLinkSerializer`, `module.py:156-203`)
// ---------------------------------------------------------------------------

/// Writable link fields in serializer order (`module.py:156-168`): every
/// other `__all__` field is read-only and silently ignored on input.
/// `deleted_at` is writable (not in `read_only_fields`) and verified live.
const LINK_WRITE_FIELDS: &[&str] = &["deleted_at", "title", "url", "metadata"];

/// `models.URLField` default: DRF measures post-strip, in code points.
const LINK_URL_MAX_LENGTH: usize = 200;
/// `title` is `CharField(max_length=255, blank=True, null=True)`.
const LINK_TITLE_MAX_LENGTH: usize = 255;

/// Presence of one input key, mirroring DRF's `empty` / `None` / value split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence<'a> {
    Missing,
    Null,
    Value(&'a Value),
}

fn presence_of<'a>(body: &'a Map<String, Value>, field: &str) -> Presence<'a> {
    match body.get(field) {
        None => Presence::Missing,
        Some(Value::Null) => Presence::Null,
        Some(value) => Presence::Value(value),
    }
}

/// Python truthiness over JSON values, for the `to_internal_value` guard
/// (`module.py:173`): only truthy non-strings reach `.startswith`.
fn is_python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                int != 0
            } else if let Some(uint) = number.as_u64() {
                uint != 0
            } else {
                number.as_f64().is_some_and(|float| float != 0.0)
            }
        }
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Mirror of `to_internal_value`'s scheme step (`module.py:170-176`) over
/// the raw `url` input: missing/empty/unschemed handling plus the
/// `AttributeError` → 500 on truthy non-strings (no `.startswith`).
/// Returns the value field validation sees (`None` = key absent).
fn prefix_link_url(raw: Option<&Value>) -> Result<Option<Value>, Denial> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if let Value::String(text) = raw {
        return Ok(Some(Value::String(
            shape::normalize_link_url(Some(text)).expect("link url"),
        )));
    }
    if is_python_truthy(raw) {
        // `url.startswith(...)` on a non-string: `AttributeError` into the
        // generic 500 handler (`app/views/base.py:146-149`).
        return Err(Denial::ServerError);
    }
    Ok(Some(raw.clone()))
}

/// Django `URLValidator` verdict (Django 4.2 `validators.py` on Python
/// 3.12): scheme allow-list, `urlsplit` (which strict-validates
/// bracketed hosts itself, with or without userinfo), the host regex,
/// the IDN retry, then the 253-char hostname cap. The regex lookarounds
/// (`(?!-)` / `(?<!-)` around single dashes) are explicit first/last-char
/// checks — the `regex` crate has no lookaround.
fn django_url_valid(url: &str) -> bool {
    let scheme = url.split("://").next().unwrap_or("").to_lowercase();
    if !["http", "https", "ftp", "ftps"].contains(&scheme.as_str()) {
        return false;
    }
    // `unsafe_chars` (`\t\r\n`) are rejected before the regex runs.
    if url.chars().any(|c| matches!(c, '\t' | '\r' | '\n')) {
        return false;
    }
    if url_host_pattern_valid(url) {
        // First match: `urlsplit`'s bracket check (strict IP parse) plus
        // the trailing hostname cap.
        return url_brackets_strict_valid(url) && url_hostname_within_cap(url);
    }
    // IDN retry: `netloc.encode("idna")` over the whole netloc, then the
    // bare regex re-match (no strict bracket re-check — Django's
    // `else` is skipped on this path) plus the cap on the ORIGINAL host.
    // An all-ASCII netloc round-trips byte-identical (modulo scheme case,
    // which the case-insensitive regex ignores), so the retry is futile
    // there and skipped.
    idna_ace_url(url).is_some_and(|ace| url_host_pattern_valid(&ace))
        && url_hostname_within_cap(url)
}

/// The `URLValidator.regex` match over one URL string: optional
/// `user:pass@`, then IPv4 | IPv6 | hostname, optional `:port`, optional
/// path/query/fragment without whitespace.
fn url_host_pattern_valid(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    // Optional `user:pass@` (`[^\s:@/]+(?::[^\s:@/]*)?@`): the pattern
    // cannot cross `/`, so `@` past the first `/` starts the resource
    // (verified live: `/a@b` validates as a path).
    let host_part = match after_scheme.find('@') {
        Some(at) if after_scheme.find('/').is_none_or(|slash| at < slash) => {
            let (userinfo, rest) = after_scheme.split_at(at);
            let rest = &rest[1..];
            // `user` / `pass` carry no whitespace, `:` or `/`.
            let mut pieces = userinfo.splitn(2, ':');
            let user_ok = pieces
                .next()
                .is_some_and(|user| !user.is_empty() && user.chars().all(valid_userinfo_char));
            let pass_ok = pieces
                .next()
                .is_none_or(|pass| pass.chars().all(valid_userinfo_char));
            if !(user_ok && pass_ok) || rest.is_empty() {
                return false;
            }
            rest
        }
        _ => after_scheme,
    };
    // Split host from `:port` / path: brackets (IPv6) protect colons.
    let tail = if let Some(rest) = host_part.strip_prefix('[') {
        let Some((inside, tail)) = rest.split_once(']') else {
            return false;
        };
        if !valid_ipv6_loose(inside) {
            return false;
        }
        tail
    } else {
        let end = host_part
            .find([':', '/', '?', '#'])
            .unwrap_or(host_part.len());
        let (host, tail) = host_part.split_at(end);
        if !valid_ipv4(host) && !valid_host_name(host) {
            return false;
        }
        tail
    };
    // Optional `:port` (1-5 digits), then optional resource without whitespace.
    let mut tail = tail;
    if let Some(port) = tail.strip_prefix(':') {
        let digits = port
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if digits.is_empty() || digits.len() > 5 {
            return false;
        }
        tail = &port[digits.len()..];
    }
    if tail.is_empty() {
        return true;
    }
    let mut chars = tail.chars();
    match chars.next() {
        Some('/' | '?' | '#') => (),
        _ => return false,
    }
    !tail.chars().any(is_python_space)
}

fn valid_userinfo_char(c: char) -> bool {
    !is_python_space(c) && c != ':' && c != '@' && c != '/'
}

/// Django's `ipv4_re`: four dot-separated groups, each `0` (bare) or
/// 1-3 digits without a leading zero, 0-255.
fn valid_ipv4(host: &str) -> bool {
    let pieces: Vec<&str> = host.split('.').collect();
    if pieces.len() != 4 {
        return false;
    }
    pieces.iter().all(|piece| {
        !piece.is_empty()
            && piece.len() <= 3
            && piece.chars().all(|c| c.is_ascii_digit())
            && (piece.len() == 1 || !piece.starts_with('0'))
            && piece.parse::<u32>().is_ok_and(|n| n <= 255)
    })
}

/// Django's `ipv6_re` (`\[[0-9a-f:.]+\]`, matched case-insensitively):
/// the loose regex half — the brackets are stripped by the caller and
/// `url_brackets_strict_valid` applies the strict parse after the match.
fn valid_ipv6_loose(inside: &str) -> bool {
    !inside.is_empty()
        && inside
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
}

/// `urlsplit`'s bracket check (Python 3.12 `_check_bracketed_netloc`):
/// any `[` / `]` in the netloc must wrap exactly one strict IP literal
/// (optional userinfo before it is fine — it is stripped first).
fn url_brackets_strict_valid(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let netloc_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let netloc = &after_scheme[..netloc_end];
    if !netloc.contains(['[', ']']) {
        return true;
    }
    let host = match netloc.rfind('@') {
        Some(at) => &netloc[at + 1..],
        None => netloc,
    };
    let Some(bracketed) = host.strip_prefix('[') else {
        return false;
    };
    let Some((inside, tail)) = bracketed.split_once(']') else {
        return false;
    };
    if inside.parse::<std::net::Ipv6Addr>().is_err() {
        return false;
    }
    if tail.is_empty() {
        return true;
    }
    let Some(port) = tail.strip_prefix(':') else {
        return false;
    };
    !port.is_empty() && port.len() <= 5 && port.chars().all(|c| c.is_ascii_digit())
}

/// Django's trailing hostname cap: `urlsplit().hostname` (lowercased
/// host without userinfo, port or brackets) is present and at most 253
/// characters (code points, not bytes).
fn url_hostname_within_cap(url: &str) -> bool {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return false;
    };
    let netloc_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let mut host = &after_scheme[..netloc_end];
    if let Some(at) = host.rfind('@') {
        host = &host[at + 1..];
    }
    if let Some(bracketed) = host.strip_prefix('[') {
        host = bracketed.split(']').next().unwrap_or("");
    } else if let Some(at) = host.find(':') {
        host = &host[..at];
    }
    !host.is_empty() && host.chars().count() <= 253
}

/// Django's `host_re` (`hostname + domain + tld | localhost`): labels of
/// letters/digits/hyphens (the full Unicode `ul` range allowed — no
/// exclusions), dashes never leading/trailing a label, TLD of 2+
/// letters/hyphens (digits excluded) or a `xn--` punycode label,
/// optional trailing dot.
fn valid_host_name(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for (index, label) in labels.iter().enumerate() {
        if label.is_empty() || label.chars().count() > 63 {
            return false;
        }
        if !label.chars().all(valid_host_char) {
            return false;
        }
        let first = label.chars().next().expect("label char");
        let last = label.chars().last().expect("label char");
        if first == '-' || last == '-' {
            return false;
        }
        if index == labels.len() - 1 && !valid_top_label(label) {
            return false;
        }
    }
    true
}

/// Host label chars: ASCII letters/digits/hyphen plus Django's `ul`
/// (`\u00a1-\uffff`, the whole range).
fn valid_host_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || ('\u{00a1}'..='\u{ffff}').contains(&c)
}

/// TLD (`[a-z\ul-]{2,63}` — no digits — or `xn--` punycode): leading and
/// trailing dashes are already excluded by the caller.
fn valid_top_label(label: &str) -> bool {
    if label.chars().count() < 2 {
        return false;
    }
    if label.len() >= 4 && label.as_bytes()[..4].eq_ignore_ascii_case(b"xn--") {
        let rest = &label[4..];
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    }
    label
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c == '-' || ('\u{00a1}'..='\u{ffff}').contains(&c))
}

/// IDN retry input: `netloc.encode("idna")` over the whole netloc, like
/// Django's `punycode(netloc)` — per dot-label, ASCII labels passing
/// through untouched (even with `:`/`@`), non-ASCII labels gaining the
/// `xn--` punycode form — with the URL rebuilt around it. `None` when
/// the netloc is all ASCII (the retry would re-match the same string).
fn idna_ace_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let netloc_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (netloc, tail) = rest.split_at(netloc_end);
    if netloc.is_ascii() {
        return None;
    }
    let mut ace = String::new();
    for (index, label) in netloc.split('.').enumerate() {
        if index > 0 {
            ace.push('.');
        }
        if label.is_ascii() {
            ace.push_str(label);
        } else {
            // `idna` C-implements the codec's per-label punycode step;
            // a label it refuses is a `UnicodeError` there too.
            ace.push_str(&idna::domain_to_ascii(label).ok()?);
        }
    }
    Some(format!("{scheme}://{ace}{tail}"))
}

/// One field's message list, in writable-field order.
type FieldErrors = Vec<(String, Vec<String>)>;

fn push_error(errors: &mut FieldErrors, field: &str, message: String) {
    match errors.iter_mut().find(|(name, _)| name == field) {
        Some((_, messages)) => messages.push(message),
        None => errors.push((field.to_owned(), vec![message])),
    }
}

/// Render collected field errors: `{"field": ["msg", ...], ...}` in
/// writable-field order (DRF `ValidationError(errors)`).
fn render_field_errors(errors: &FieldErrors) -> String {
    let mut out = String::from("{");
    for (index, (field, messages)) in errors.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&json_string(field));
        out.push_str(":[");
        for (i, message) in messages.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&json_string(message));
        }
        out.push(']');
    }
    out.push('}');
    out
}

/// Validated link input: `None` = key absent (keep on update), `Some` =
/// provided value. `title` carries null-vs-string; `url` the normalized
/// string; `metadata` any JSON; `deleted_at` the resolved UTC instant.
#[derive(Debug)]
struct LinkInput {
    deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<Value>,
}

/// Validate one link body (`to_internal_value` + per-field `run_validation`
/// over the writable fields, then nothing object-level — the serializer
/// defines no `validate()`). `partial` skips absent keys (PATCH); otherwise
/// `url` is required. Errors answer 400; only the scheme-prefix step can
/// 500 (see [`prefix_link_url`]).
#[allow(clippy::result_large_err)]
fn validate_link_body(
    body: &Map<String, Value>,
    partial: bool,
    timezone: &chrono_tz::Tz,
) -> Result<LinkInput, axum::response::Response> {
    let bad_request =
        |errors: &FieldErrors| json_response(StatusCode::BAD_REQUEST, render_field_errors(errors));
    let mut errors: FieldErrors = Vec::new();
    // `deleted_at` input: a writable `DateTimeField(null, blank)`. Naive
    // inputs attach the request zone (DRF `enforce_timezone`); the invalid
    // message is DRF's verbatim input-format string.
    let deleted_at = match presence_of(body, "deleted_at") {
        Presence::Missing => None,
        Presence::Null => Some(None),
        Presence::Value(value) => match resolve_link_datetime_input(value, timezone) {
            Ok(stamp) => Some(Some(stamp)),
            Err(message) => {
                push_error(&mut errors, "deleted_at", message);
                None
            }
        },
    };
    // `title`: `CharField(max_length=255, blank=True, null=True)`,
    // `required=False`. Numerics stringify (Python `str()`); bools and
    // composites fail; the stored value is whitespace-trimmed.
    let title = match presence_of(body, "title") {
        Presence::Missing => None,
        Presence::Null => Some(None),
        Presence::Value(value) => match coerce_link_string(value) {
            None => {
                push_error(&mut errors, "title", "Not a valid string.".to_owned());
                None
            }
            Some(text) => {
                let trimmed = text.trim().to_owned();
                if trimmed.chars().count() > LINK_TITLE_MAX_LENGTH {
                    push_error(
                        &mut errors,
                        "title",
                        format!(
                            "Ensure this field has no more than {LINK_TITLE_MAX_LENGTH} characters."
                        ),
                    );
                }
                if trimmed.contains('\0') {
                    push_error(
                        &mut errors,
                        "title",
                        "Null characters are not allowed.".to_owned(),
                    );
                }
                Some(Some(trimmed))
            }
        },
    };
    // `url`: the scheme-prefix step first (it can 500), then
    // `URLField(max_length=200)` validation on the prefixed value.
    let prefixed = prefix_link_url(body.get("url")).map_err(|denial| denial.into_response())?;
    let url = match (&prefixed, presence_of(body, "url")) {
        (None, Presence::Missing) if partial => None,
        (None, _) => {
            push_error(&mut errors, "url", "This field is required.".to_owned());
            None
        }
        (Some(Value::Null), _) => {
            push_error(&mut errors, "url", "This field may not be null.".to_owned());
            None
        }
        // `CharField.to_internal_value`'s `fail('invalid')` renders
        // through `URLField`'s overridden `invalid` message, so
        // non-string inputs answer "Enter a valid URL.", never "Not a
        // valid string." (verified live).
        (Some(value), _) => match coerce_link_string(value) {
            None => {
                push_error(
                    &mut errors,
                    "url",
                    shape::DRF_INVALID_URL_MESSAGE.to_owned(),
                );
                None
            }
            Some(text) => {
                let trimmed = text.trim().to_owned();
                // `CharField.run_validation` fails blank input before
                // `to_internal_value`/validators run, so a blank url
                // reports only the blank error.
                if trimmed.is_empty() {
                    push_error(
                        &mut errors,
                        "url",
                        "This field may not be blank.".to_owned(),
                    );
                    None
                } else {
                    if trimmed.chars().count() > LINK_URL_MAX_LENGTH {
                        push_error(
                            &mut errors,
                            "url",
                            format!(
                                "Ensure this field has no more than {LINK_URL_MAX_LENGTH} characters."
                            ),
                        );
                    }
                    if trimmed.contains('\0') {
                        push_error(
                            &mut errors,
                            "url",
                            "Null characters are not allowed.".to_owned(),
                        );
                    }
                    if !django_url_valid(&trimmed) {
                        push_error(
                            &mut errors,
                            "url",
                            shape::DRF_INVALID_URL_MESSAGE.to_owned(),
                        );
                    }
                    Some(trimmed)
                }
            }
        },
    };
    // `metadata`: plain `JSONField(default=dict)` — any JSON value stands.
    let metadata = match presence_of(body, "metadata") {
        Presence::Missing => None,
        Presence::Null => {
            push_error(
                &mut errors,
                "metadata",
                "This field may not be null.".to_owned(),
            );
            None
        }
        Presence::Value(value) => Some(value.clone()),
    };
    if !errors.is_empty() {
        // Field order, not discovery order (`_writable_fields` iteration).
        errors.sort_by_key(|(field, _)| {
            LINK_WRITE_FIELDS
                .iter()
                .position(|name| name == field)
                .unwrap_or(usize::MAX)
        });
        return Err(bad_request(&errors));
    }
    Ok(LinkInput {
        deleted_at,
        title,
        url,
        metadata,
    })
}

/// DRF `CharField.to_internal_value` coercion: strings pass, ints/floats
/// stringify (Python `str()`), everything else fails.
fn coerce_link_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Some(int.to_string())
            } else if let Some(uint) = number.as_u64() {
                Some(uint.to_string())
            } else {
                number.as_f64().map(crate::paginator::py_float_str)
            }
        }
        _ => None,
    }
}

/// DRF `DateTimeField` invalid-input message, verbatim.
fn link_datetime_invalid_message() -> String {
    "Datetime has wrong format. Use one of these formats instead: YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z]."
        .to_owned()
}

/// DRF `DateTimeField` overflow message, verbatim: an aware instant
/// outside Python's representable range (`astimezone` raises).
fn link_datetime_overflow_message() -> String {
    "Datetime value out of range.".to_owned()
}

/// DRF `DateTimeField` input for `deleted_at`: Django's
/// `parse_datetime` (`iso-8601` input format — the only entry in the
/// default `DATETIME_INPUT_FORMATS`, unset in this project) +
/// `enforce_timezone`. `Ok` is the UTC instant; `Err` is the exact field
/// message (`invalid`, or `overflow` when the aware instant falls outside
/// Python's representable range).
fn resolve_link_datetime_input(
    value: &Value,
    timezone: &chrono_tz::Tz,
) -> Result<chrono::DateTime<chrono::Utc>, String> {
    use chrono::{Datelike, MappedLocalTime, TimeZone};
    let Value::String(text) = value else {
        return Err(link_datetime_invalid_message());
    };
    let Some((naive, offset)) = parse_link_datetime(text) else {
        return Err(link_datetime_invalid_message());
    };
    // Python datetimes start at year 1; chrono would also build year 0.
    if naive.date().year() < 1 {
        return Err(link_datetime_invalid_message());
    }
    match offset {
        Some(offset) => {
            // Aware inputs keep their instant (`astimezone`); an instant
            // outside `0001-01-01..9999-12-31` overflows there instead.
            let instant = naive.and_utc() - offset;
            if !(1..=9999).contains(&instant.date_naive().year()) {
                return Err(link_datetime_overflow_message());
            }
            Ok(instant)
        }
        // Naive inputs attach the request zone (`get_current_timezone`,
        // activated from `user_timezone` by `TimezoneMixin`). DRF's
        // `valid_datetime` gate is a no-op for zoneinfo zones (verified
        // against the shipped DRF: gap and fold wall times all pass), so
        // ambiguous times take fold 0 and gap times take the
        // pre-transition offset — exactly Python's `replace(tzinfo=...)`.
        None => match timezone.from_local_datetime(&naive) {
            MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) => {
                Ok(local.to_utc())
            }
            MappedLocalTime::None => Ok(pre_transition_instant(timezone, &naive)),
        },
    }
}

/// Instant for a wall time inside a DST gap: the pre-transition offset
/// applied to the wall time (Python's fold-0 `zoneinfo` attach). Walks
/// back in 15-minute steps to the last mappable wall time (gaps run well
/// under the ~3-day cap); falls back to UTC past the cap.
fn pre_transition_instant(
    timezone: &chrono_tz::Tz,
    naive: &chrono::NaiveDateTime,
) -> chrono::DateTime<chrono::Utc> {
    use chrono::{MappedLocalTime, TimeZone};
    let mut probe = *naive;
    for _ in 0..300 {
        probe = match probe.checked_sub_signed(chrono::Duration::minutes(15)) {
            Some(previous) => previous,
            None => break,
        };
        if let MappedLocalTime::Single(local) | MappedLocalTime::Ambiguous(local, _) =
            timezone.from_local_datetime(&probe)
        {
            let east = local.naive_local() - local.naive_utc();
            return naive.and_utc() - east;
        }
    }
    naive.and_utc()
}

/// Django `parse_datetime` (Django 4.2 on Python 3.12): `fromisoformat`
/// first (strict, full consumption), then the `datetime_re` fallback.
/// Returns the naive wall time plus the fixed UTC offset (`None` when the
/// input carries none). `ValueError`-vs-`None` is wire-invisible (both
/// render the `invalid` message), so both are `None` here.
fn parse_link_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    parse_fromiso_datetime(text).or_else(|| parse_regex_datetime(text))
}

/// Two ASCII digits as a number.
fn two_digits(text: &str) -> Option<u32> {
    if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// One or two ASCII digits as a number.
fn one_two_digits(text: &str) -> Option<u32> {
    if text.is_empty() || text.len() > 2 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Fractional seconds (`[.,]` + digits) as microseconds: the first six
/// digits, right-padded with zeros. `fromisoformat` takes any digit run;
/// the regex path caps the run before calling.
fn frac_micros(frac: &str) -> u32 {
    let mut digits: String = frac.chars().take(6).collect();
    while digits.len() < 6 {
        digits.push('0');
    }
    digits.parse().unwrap_or(0)
}

/// `fromisoformat` date head: extended `YYYY-MM-DD`, basic `YYYYMMDD`,
/// or ISO week `YYYY-Www[-D]` / `YYYYWww[D]` (day defaults to Monday).
/// Returns the calendar date plus the unconsumed tail.
fn parse_fromiso_date(text: &str) -> Option<(chrono::NaiveDate, &str)> {
    use chrono::Datelike;
    let bytes = text.as_bytes();
    if bytes.len() >= 8 && bytes[4] == b'-' && bytes[5] == b'W' {
        // Extended week date (`YYYY-Www[-D]`, day defaults to Monday).
        let year: i32 = text.get(..4)?.parse().ok()?;
        let week = two_digits(text.get(6..8)?)?;
        let (day, tail) = match text.as_bytes().get(8) {
            Some(b'-') => (text.get(9..10)?.parse::<u32>().ok()?, text.get(10..)?),
            _ => (1, text.get(8..)?),
        };
        if !(1..=7).contains(&day) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        let date = date.checked_add_signed(chrono::Duration::days(i64::from(day - 1)))?;
        // `from_isoywd_opt` accepts week 53 only in 53-week years;
        // the weekday shift cannot cross into another week numbering.
        if date.iso_week().week() != week {
            return None;
        }
        return Some((date, tail));
    }
    if bytes.len() >= 10 && bytes[4] == b'-' {
        let year: i32 = text.get(..4)?.parse().ok()?;
        let month = two_digits(text.get(5..7)?)?;
        if text.as_bytes().get(7) != Some(&b'-') {
            return None;
        }
        let day = two_digits(text.get(8..10)?)?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, text.get(10..)?));
    }
    if bytes.len() >= 5 && bytes[4] == b'W' {
        // Basic week date.
        let year: i32 = text.get(..4)?.parse().ok()?;
        let week = two_digits(text.get(5..7)?)?;
        let (day, tail) = match text.as_bytes().get(7) {
            Some(b) if b.is_ascii_digit() => (text.get(7..8)?.parse::<u32>().ok()?, text.get(8..)?),
            _ => (1, text.get(7..)?),
        };
        if !(1..=7).contains(&day) {
            return None;
        }
        let date = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)?;
        let date = date.checked_add_signed(chrono::Duration::days(i64::from(day - 1)))?;
        if date.iso_week().week() != week {
            return None;
        }
        return Some((date, tail));
    }
    if bytes.len() >= 8 && bytes[..8].iter().all(|b| b.is_ascii_digit()) {
        let year: i32 = text.get(..4)?.parse().ok()?;
        let month = two_digits(text.get(4..6)?)?;
        let day = two_digits(text.get(6..8)?)?;
        let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        return Some((date, text.get(8..)?));
    }
    None
}

/// `fromisoformat` time + zone tail (strict two-digit parts): extended
/// `HH[:MM[:SS]]`, basic `HHMM[SS]` / `HHMM` / `HH`, an optional
/// `[.,]`-fraction (any length — fractional seconds whatever the clock
/// shows), and an optional `Z` / numeric offset. Full consumption.
fn parse_fromiso_time(
    date: chrono::NaiveDate,
    text: &str,
) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    // Basic vs extended: a colon after the hour forces extended.
    let (hour, minute, second, tail) = if text.len() >= 3 && text.as_bytes()[2] == b':' {
        let hour = two_digits(text.get(..2)?)?;
        let minute = two_digits(text.get(3..5)?)?;
        let (second, tail) = if text.as_bytes().get(5) == Some(&b':') {
            (two_digits(text.get(6..8)?)?, text.get(8..)?)
        } else {
            (0, text.get(5..)?)
        };
        (hour, minute, second, tail)
    } else {
        let digits: usize = text.bytes().take_while(u8::is_ascii_digit).count();
        if !matches!(digits, 2 | 4 | 6) {
            return None;
        }
        let hour = two_digits(text.get(..2)?)?;
        let minute = if digits >= 4 {
            two_digits(text.get(2..4)?)?
        } else {
            0
        };
        let second = if digits == 6 {
            two_digits(text.get(4..6)?)?
        } else {
            0
        };
        (hour, minute, second, text.get(digits..)?)
    };
    let (micro, tail) = match tail.as_bytes().first() {
        Some(b'.' | b',') => {
            let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
            if frac == 0 {
                return None;
            }
            (frac_micros(tail.get(1..1 + frac)?), tail.get(1 + frac..)?)
        }
        _ => (0, tail),
    };
    let offset = parse_fromiso_offset(tail)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

/// `fromisoformat` zone tail: `Z`, `±HH[:MM[:SS]]`, `±HHMM[SS]` or
/// `±HH`, with an optional `[.,]`-fraction that always means fractional
/// *seconds* — dropped when hour, minute and second are all zero (the
/// `timezone.utc` fast path; verified against CPython 3.12). Components
/// carry no range check; only the total must stay strictly inside a day.
fn parse_fromiso_offset(text: &str) -> Option<Option<chrono::Duration>> {
    if text.is_empty() {
        return Some(None);
    }
    if text == "Z" {
        return Some(Some(chrono::Duration::zero()));
    }
    let (sign, tail) = match text.as_bytes().first() {
        Some(b'+') => (1i64, text.get(1..)?),
        Some(b'-') => (-1i64, text.get(1..)?),
        _ => return None,
    };
    let hours = two_digits(tail.get(..2)?)?;
    let tail = tail.get(2..)?;
    let (minutes, seconds, tail) = if let Some(rest) = tail.strip_prefix(':') {
        let minutes = two_digits(rest.get(..2)?)?;
        let rest = rest.get(2..)?;
        if let Some(rest) = rest.strip_prefix(':') {
            (minutes, two_digits(rest.get(..2)?)?, rest.get(2..)?)
        } else {
            (minutes, 0, rest)
        }
    } else {
        let digits: usize = tail.bytes().take_while(u8::is_ascii_digit).count();
        if digits != 0 && digits != 2 && digits != 4 {
            return None;
        }
        let minutes = if digits >= 2 {
            two_digits(tail.get(..2)?)?
        } else {
            0
        };
        let seconds = if digits == 4 {
            two_digits(tail.get(2..4)?)?
        } else {
            0
        };
        (minutes, seconds, tail.get(digits..)?)
    };
    let (frac_us, tail) = match tail.as_bytes().first() {
        Some(b'.' | b',') => {
            let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
            if frac == 0 {
                return None;
            }
            (
                i64::from(frac_micros(tail.get(1..1 + frac)?)),
                tail.get(1 + frac..)?,
            )
        }
        _ => (0, tail),
    };
    if !tail.is_empty() {
        return None;
    }
    let mut total_us =
        (i64::from(hours) * 3600 + i64::from(minutes) * 60 + i64::from(seconds)) * 1_000_000;
    if hours != 0 || minutes != 0 || seconds != 0 {
        total_us += frac_us;
    }
    if total_us.abs() >= 86_400_000_000 {
        return None;
    }
    Some(Some(chrono::Duration::microseconds(sign * total_us)))
}

/// `fromisoformat` whole: date, then end-of-input (midnight) or exactly
/// one separator character — any character, even a digit — plus the time.
fn parse_fromiso_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    let (date, tail) = parse_fromiso_date(text)?;
    if tail.is_empty() {
        return Some((
            chrono::NaiveDateTime::new(date, chrono::NaiveTime::MIN),
            None,
        ));
    }
    let mut chars = tail.char_indices();
    chars.next()?;
    let time = match chars.next() {
        Some((at, _)) => tail.get(at..)?,
        None => "",
    };
    if time.is_empty() {
        return None;
    }
    parse_fromiso_time(date, time)
}

/// Python `re` `\s` over `str`: ASCII whitespace plus the Unicode spaces.
fn is_python_space(char: char) -> bool {
    char.is_whitespace() || matches!(char, '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{1f}')
}

/// The `datetime_re` fallback (`django/utils/dateparse.py`):
/// `YYYY-M-D[T ]H:M[:S[.,f{1,12}]]`, 1-2 digit parts, optional whitespace
/// before an optional `Z` / `±HH[[:]MM]` zone, then end (the `$` anchor
/// also tolerates one trailing newline). Values run through the
/// `datetime` constructor; the zone total must stay strictly inside a day.
fn parse_regex_datetime(text: &str) -> Option<(chrono::NaiveDateTime, Option<chrono::Duration>)> {
    let (date_part, clock) = text.split_once(['T', ' '])?;
    let mut date_pieces = date_part.split('-');
    let (year_s, month_s, day_s) = (
        date_pieces.next()?,
        date_pieces.next()?,
        date_pieces.next()?,
    );
    if date_pieces.next().is_some() {
        return None;
    }
    if year_s.len() != 4
        || !year_s.bytes().all(|b| b.is_ascii_digit())
        || month_s.is_empty()
        || month_s.len() > 2
        || day_s.is_empty()
        || day_s.len() > 2
    {
        return None;
    }
    let (year, month, day): (i32, u32, u32) = (
        year_s.parse().ok()?,
        month_s.parse().ok()?,
        day_s.parse().ok()?,
    );
    let (hour_min, mut tail) = match clock.split_once(':') {
        Some((hour_s, rest)) => {
            let hour = one_two_digits(hour_s)?;
            // Minute digits, then seconds / fraction / zone tail.
            let minute_len: usize = rest.bytes().take_while(u8::is_ascii_digit).count();
            if minute_len == 0 || minute_len > 2 {
                return None;
            }
            let minute: u32 = rest.get(..minute_len)?.parse().ok()?;
            ((hour, minute), rest.get(minute_len..)?)
        }
        None => return None,
    };
    let (hour, minute) = hour_min;
    let (second, micro) = if let Some(rest) = tail.strip_prefix(':') {
        let second_len: usize = rest.bytes().take_while(u8::is_ascii_digit).count();
        if second_len == 0 || second_len > 2 {
            return None;
        }
        let second: u32 = rest.get(..second_len)?.parse().ok()?;
        tail = rest.get(second_len..)?;
        match tail.as_bytes().first() {
            Some(b'.' | b',') => {
                let frac: usize = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
                if !(1..=12).contains(&frac) {
                    return None;
                }
                let micro = frac_micros(tail.get(1..1 + frac)?);
                tail = tail.get(1 + frac..)?;
                (second, micro)
            }
            _ => (second, 0),
        }
    } else {
        (0, 0)
    };
    let spaces: usize = tail.chars().take_while(|c| is_python_space(*c)).count();
    tail = tail.get(tail.chars().take(spaces).map(char::len_utf8).sum::<usize>()..)?;
    let offset = if tail.is_empty() || tail == "\n" {
        None
    } else if tail == "Z" || tail == "Z\n" {
        Some(chrono::Duration::zero())
    } else {
        let zone = tail.strip_suffix('\n').unwrap_or(tail);
        let (sign, digits) = match zone.as_bytes().first() {
            Some(b'+') => (1i64, zone.get(1..)?),
            Some(b'-') => (-1i64, zone.get(1..)?),
            _ => return None,
        };
        let (hours, minutes) = if let Some((hour_s, minute_s)) = digits.split_once(':') {
            (two_digits(hour_s)?, two_digits(minute_s)?)
        } else if digits.len() == 2 {
            (two_digits(digits)?, 0)
        } else if digits.len() == 4 {
            (two_digits(digits.get(..2)?)?, two_digits(digits.get(2..)?)?)
        } else {
            return None;
        };
        let total = sign * (i64::from(hours) * 60 + i64::from(minutes));
        if total.abs() >= 24 * 60 {
            return None;
        }
        Some(chrono::Duration::minutes(total))
    };
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, micro)?;
    Some((chrono::NaiveDateTime::new(date, time), offset))
}

/// `GET` module-links collection: bare JSON array, `-created_at`.
async fn link_list(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "GET", &slug, &facts) {
        return Ok(response);
    }
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let rows = fetch_link_rows(&pool, &slug, &project_id, &module_id, &user_id, None).await?;
    let mut out = String::from("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&shape_link(row, &timezone));
    }
    out.push(']');
    Ok(json_response(StatusCode::OK, out))
}

/// Shared create/update write preparation: the caller's project row must
/// exist (post-gate it always does) to stamp `workspace_id` like
/// `ProjectBaseModel.save`.
async fn link_project_workspace(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
) -> Result<uuid::Uuid, Denial> {
    let row: Option<(uuid::Uuid,)> =
        sqlx::query_as(r#"SELECT workspace_id FROM projects WHERE id = $1"#)
            .bind(project_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| Denial::ServerError)?;
    row.map(|(id,)| id).ok_or(Denial::ServerError)
}

/// Render one freshly-written link row from known values (the serializer
/// renders the saved instance, not a re-read).
#[allow(clippy::too_many_arguments)]
fn render_link_written(
    id: &uuid::Uuid,
    stamp: &str,
    deleted_at: Option<&str>,
    title: Option<&str>,
    url: &str,
    metadata: &Value,
    created_by: Option<&uuid::Uuid>,
    updated_by: Option<&uuid::Uuid>,
    project_id: &uuid::Uuid,
    workspace_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    timezone: &chrono_tz::Tz,
) -> String {
    let mut row = Map::new();
    row.insert("id".to_owned(), Value::String(id.to_string()));
    row.insert("created_at".to_owned(), Value::String(stamp.to_owned()));
    row.insert("updated_at".to_owned(), Value::String(stamp.to_owned()));
    row.insert(
        "deleted_at".to_owned(),
        deleted_at.map_or(Value::Null, |text| Value::String(text.to_owned())),
    );
    row.insert(
        "title".to_owned(),
        title.map_or(Value::Null, |text| Value::String(text.to_owned())),
    );
    row.insert("url".to_owned(), Value::String(url.to_owned()));
    row.insert("metadata".to_owned(), metadata.clone());
    row.insert(
        "created_by".to_owned(),
        created_by.map_or(Value::Null, |id| Value::String(id.to_string())),
    );
    row.insert(
        "updated_by".to_owned(),
        updated_by.map_or(Value::Null, |id| Value::String(id.to_string())),
    );
    row.insert("project".to_owned(), Value::String(project_id.to_string()));
    row.insert(
        "workspace".to_owned(),
        Value::String(workspace_id.to_string()),
    );
    row.insert("module".to_owned(), Value::String(module_id.to_string()));
    shape_link(&row, timezone)
}

/// Current UTC stamp in the `row_to_json` text form (`RFC 3339`), the
/// single instant standing in for Django's per-field `auto_now` stamps
/// (indistinguishable on the wire — see `handlers_modules`).
fn utc_stamp_text(now: &chrono::DateTime<chrono::Utc>) -> String {
    now.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// `POST` module-links collection: 201 with the link row.
async fn link_create(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "POST",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "POST", &slug, &facts) {
        return Ok(response);
    }
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let Some(map) = body.as_object() else {
        // `data.get("url", "")` on a non-dict: `AttributeError` → 500.
        return Err(Denial::ServerError);
    };
    let input = match validate_link_body(map, false, &timezone) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    let url = input.url.expect("required link url");
    let title = input.title.flatten();
    let deleted_at = input.deleted_at.flatten();
    // `create()` re-validates the (already field-valid) url — it cannot
    // fail here — then rejects per-module duplicates (`module.py:188-192`).
    let taken: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM module_links
           WHERE url = $1 AND module_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(&url)
    .bind(module_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if taken.is_some() {
        let body = shape::duplicate_link_body();
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&body).expect("duplicate link body"),
        ));
    }
    let workspace_id = link_project_workspace(&pool, &project_id).await?;
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let stamp = utc_stamp_text(&now);
    let metadata = input.metadata.unwrap_or_else(|| Value::Object(Map::new()));
    let insert = sqlx::query(
        r#"INSERT INTO module_links
           (id, created_at, updated_at, deleted_at, title, url, metadata,
            created_by_id, updated_by_id, project_id, workspace_id, module_id)
           VALUES ($1, $2, $2, $3, $4, $5, $6, $7, NULL, $8, $9, $10)"#,
    )
    .bind(id)
    .bind(now)
    .bind(deleted_at)
    .bind(title.clone())
    .bind(&url)
    .bind(sqlx::types::Json(metadata.clone()))
    .bind(user_id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(module_id)
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        // Missing module row (FK): `IntegrityError` → 400 (`module.py:192`
        // has no module check — the database raises).
        return Err(integrity_denial(&error));
    }
    let deleted_at_text = deleted_at.map(|stamp| utc_stamp_text(&stamp));
    Ok(json_response(
        StatusCode::CREATED,
        render_link_written(
            &id,
            &stamp,
            deleted_at_text.as_deref(),
            title.as_deref(),
            &url,
            &metadata,
            Some(&user_id),
            None,
            &project_id,
            &workspace_id,
            &module_id,
            &timezone,
        ),
    ))
}

/// DRF `get_object()` miss over the link queryset: `Http404` rendered by
/// DRF's default handler (lowercase `detail`, verified live).
const LINK_NOT_FOUND_BODY: &str = r#"{"detail":"No ModuleLink matches the given query."}"#;

/// Shared detail lookup: the scoped row or the DRF 404 body.
async fn link_detail_row(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
    pk: &uuid::Uuid,
) -> Result<Map<String, Value>, axum::response::Response> {
    let rows = fetch_link_rows(pool, slug, project_id, module_id, user_id, Some(pk))
        .await
        .map_err(|denial| denial.into_response())?;
    rows.into_iter()
        .next()
        .ok_or_else(|| json_response(StatusCode::NOT_FOUND, LINK_NOT_FOUND_BODY.to_owned()))
}

/// `GET` module-link detail.
async fn link_retrieve(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "GET", &slug, &facts) {
        return Ok(response);
    }
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    let row = match link_detail_row(&pool, &slug, &project_id, &module_id, &user_id, &pk).await {
        Ok(row) => row,
        Err(response) => return Ok(response),
    };
    Ok(json_response(StatusCode::OK, shape_link(&row, &timezone)))
}

/// `PUT` / `PATCH` module-link detail: full validation, then `update()` —
/// which re-validates `url` unconditionally (`module.py:194-195`), so a
/// PATCH omitting it 400s — then the per-module clash check excluding
/// self, then the save. 200 with the link row.
#[allow(clippy::too_many_arguments)]
async fn link_write(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    module_raw: &str,
    pk_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: &str,
    body: &Value,
) -> HandlerResult {
    let pool = pool_of(state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, slug, project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        method,
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
    );
    let facts = fetch_class_facts(&pool, slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, method, slug, &facts) {
        return Ok(response);
    }
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(module_raw)?;
    let pk = parse_uuid_or_invalid(pk_raw)?;
    let row = match link_detail_row(&pool, slug, &project_id, &module_id, &user_id, &pk).await {
        Ok(row) => row,
        Err(response) => return Ok(response),
    };
    let Some(map) = body.as_object() else {
        return Err(Denial::ServerError);
    };
    let partial = method == "PATCH";
    let input = match validate_link_body(map, partial, &timezone) {
        Ok(input) => input,
        Err(response) => return Ok(response),
    };
    // `update()` (`module.py:194-203`): unconditional `validate_url` on the
    // (possibly absent) url, then the clash check excluding self.
    let url = match input.url {
        Some(url) => url,
        None => {
            let body = shape::invalid_url_body();
            return Ok(json_response(
                StatusCode::BAD_REQUEST,
                serde_json::to_string(&body).expect("invalid url body"),
            ));
        }
    };
    let clash: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM module_links
           WHERE url = $1 AND module_id = $2 AND id != $3 AND deleted_at IS NULL"#,
    )
    .bind(&url)
    .bind(module_id)
    .bind(pk)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    if clash.is_some() {
        let body = shape::duplicate_link_update_body();
        return Ok(json_response(
            StatusCode::BAD_REQUEST,
            serde_json::to_string(&body).expect("duplicate link update body"),
        ));
    }
    // `super().update()`: only provided keys move; `updated_at` /
    // `updated_by` always do (`auto_now`, `BaseModel.save`).
    let now = chrono::Utc::now();
    apply_link_update(
        &pool,
        &pk,
        &user_id,
        &now,
        &url,
        input.deleted_at,
        input.title.clone(),
        input.metadata.clone(),
    )
    .await?;
    // The response renders the saved instance: the pre-update row with the
    // written keys applied.
    let mut updated = row;
    updated.insert("url".to_owned(), Value::String(url));
    match input.deleted_at {
        Some(Some(stamp)) => {
            updated.insert(
                "deleted_at".to_owned(),
                Value::String(utc_stamp_text(&stamp)),
            );
        }
        Some(None) => {
            updated.insert("deleted_at".to_owned(), Value::Null);
        }
        None => {}
    }
    if let Some(title) = input.title {
        updated.insert("title".to_owned(), title.map_or(Value::Null, Value::String));
    }
    if let Some(metadata) = input.metadata {
        updated.insert("metadata".to_owned(), metadata);
    }
    updated.insert("updated_at".to_owned(), Value::String(utc_stamp_text(&now)));
    updated.insert("updated_by".to_owned(), Value::String(user_id.to_string()));
    Ok(json_response(
        StatusCode::OK,
        shape_link(&updated, &timezone),
    ))
}

/// One `UPDATE module_links` bind value.
enum LinkBind {
    Text(String),
    TextOpt(Option<String>),
    TimestamptzOpt(Option<chrono::DateTime<chrono::Utc>>),
    Json(Value),
}

impl LinkBind {
    fn apply<'a>(
        self,
        query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
        match self {
            LinkBind::Text(text) => query.bind(text),
            LinkBind::TextOpt(text) => query.bind(text),
            LinkBind::TimestamptzOpt(stamp) => query.bind(stamp),
            LinkBind::Json(value) => query.bind(sqlx::types::Json(value)),
        }
    }
}

/// Apply one link `UPDATE`: the url always moves (it is required, or the
/// PATCH-missing case bug-400'd above); the other keys move only when
/// provided. `$1` is the link id; every SET column takes the next
/// placeholder.
#[allow(clippy::too_many_arguments)]
async fn apply_link_update(
    pool: &sqlx::PgPool,
    pk: &uuid::Uuid,
    user_id: &uuid::Uuid,
    now: &chrono::DateTime<chrono::Utc>,
    url: &str,
    deleted_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    title: Option<Option<String>>,
    metadata: Option<Value>,
) -> Result<(), Denial> {
    let mut parts: Vec<String> = Vec::new();
    let mut binds: Vec<LinkBind> = Vec::new();
    parts.push("url = $2".to_owned());
    binds.push(LinkBind::Text(url.to_owned()));
    if let Some(deleted_at) = deleted_at {
        parts.push(format!("deleted_at = ${}", binds.len() + 2));
        binds.push(LinkBind::TimestamptzOpt(deleted_at));
    }
    if let Some(title) = title {
        parts.push(format!("title = ${}", binds.len() + 2));
        binds.push(LinkBind::TextOpt(title));
    }
    if let Some(metadata) = metadata {
        parts.push(format!("metadata = ${}", binds.len() + 2));
        binds.push(LinkBind::Json(metadata));
    }
    parts.push(format!("updated_at = ${}", binds.len() + 2));
    let now_bind = *now;
    parts.push(format!("updated_by_id = ${}", binds.len() + 3));
    let sql = format!("UPDATE module_links SET {} WHERE id = $1", parts.join(", "));
    let mut query = sqlx::query(&sql).bind(pk);
    for bind in binds {
        query = bind.apply(query);
    }
    query = query.bind(now_bind).bind(user_id);
    query.execute(pool).await.map_err(|_| Denial::ServerError)?;
    Ok(())
}

async fn link_update(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    link_write(
        &state,
        &slug,
        &project_raw,
        &module_raw,
        &pk_raw,
        extension,
        "PUT",
        &body,
    )
    .await
}

async fn link_partial_update(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    link_write(
        &state,
        &slug,
        &project_raw,
        &module_raw,
        &pk_raw,
        extension,
        "PATCH",
        &body,
    )
    .await
}

/// `DELETE` module-link detail: DRF default destroy — instance
/// `delete()` is a soft delete (`deleted_at` stamp through `save()`, so
/// `updated_at`/`updated_by` move too). 204 empty.
async fn link_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw, pk_raw)): Path<(String, String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/module-links/<pk>/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "DELETE", &slug, &facts) {
        return Ok(response);
    }
    // `TimezoneMixin` activation runs in `initial` (bad zone → 500),
    // before the body — even without rendered datetimes.
    let _ = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    let pk = parse_uuid_or_invalid(&pk_raw)?;
    if let Err(response) =
        link_detail_row(&pool, &slug, &project_id, &module_id, &user_id, &pk).await
    {
        return Ok(response);
    }
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE module_links SET deleted_at = $2, updated_at = $2, updated_by_id = $3
           WHERE id = $1"#,
    )
    .bind(pk)
    .bind(now)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// Favorites (`ModuleFavoriteViewSet`, `base.py:791-824`)
// ---------------------------------------------------------------------------

/// `GET` user-favorite-modules collection: the ported 500. DRF's default
/// `list` needs `serializer_class` (absent → `AssertionError`) before the
/// queryset's `select_related("module")` `FieldError` could even fire —
/// either way the generic 500 branch. The `ProjectLitePermission` gate
/// still runs first.
async fn favorite_list(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "GET",
        "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "GET", &slug, &facts) {
        return Ok(response);
    }
    Err(Denial::ServerError)
}

/// Parse the favorite `module` input like `UUIDField.get_prep_value`:
/// missing/null → `None` (stored NULL — the field is nullable); strings go
/// through `uuid.UUID(hex=...)` (braces/`urn:` accepted, garbage →
/// `ValidationError`); ints (incl. bools) are `uuid.UUID(int=...)`
/// (negative → `ValidationError`); floats and composites fail.
fn parse_favorite_module(value: Option<&Value>) -> Result<Option<uuid::Uuid>, Denial> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(text) => text
            .parse::<uuid::Uuid>()
            .map(Some)
            .map_err(|_| Denial::BadError(INVALID_DETAIL_MSG.to_owned())),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                if int < 0 {
                    return Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned()));
                }
                Ok(Some(uuid::Uuid::from_u128(int as u128)))
            } else if let Some(uint) = number.as_u64() {
                Ok(Some(uuid::Uuid::from_u128(uint as u128)))
            } else {
                Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned()))
            }
        }
        // `bool` is an `int` subclass in Python: `True` → 1, `False` → 0.
        Value::Bool(flag) => Ok(Some(uuid::Uuid::from_u128(u128::from(*flag as u8)))),
        Value::Array(_) | Value::Object(_) => Err(Denial::BadError(INVALID_DETAIL_MSG.to_owned())),
    }
}

/// Badly-formed-identifier message, shared with `handlers_modules`
/// (`parse_uuid_or_invalid`): the `ValidationError` branch
/// (`app/views/base.py:126-130`).
const INVALID_DETAIL_MSG: &str = "Please provide valid detail";

/// `POST` user-favorite-modules collection: insert the favorite row
/// (204 empty). `UserFavorite.save` stamps `sequence = MAX + 10000` and
/// `WorkspaceBaseModel.save` fills `workspace_id` from the project; a
/// taken `(entity_type, entity_identifier, user)` triple violates the
/// partial unique index → 400.
async fn favorite_create(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "POST",
        "workspaces/<slug>/projects/<project_id>/user-favorite-modules/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "POST", &slug, &facts) {
        return Ok(response);
    }
    // `TimezoneMixin` activation (bad zone → 500) runs in `initial`,
    // before the body reads `request.data`.
    let _ = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let Some(map) = body.as_object() else {
        // `request.data.get("module")` on a non-dict: `AttributeError` → 500.
        return Err(Denial::ServerError);
    };
    let module = parse_favorite_module(map.get("module"))?;
    let workspace_id = link_project_workspace(&pool, &project_id).await?;
    // `UserFavorite.save` (`db/models/favorite.py:52-66`): largest live
    // workspace sequence + 10000, else the 65535 default.
    let largest: Option<(Option<f64>,)> = sqlx::query_as(
        r#"SELECT MAX(sequence) FROM user_favorites
           WHERE workspace_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(workspace_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let sequence = largest
        .and_then(|(max,)| max)
        .map_or(65535.0, |max| max + 10000.0);
    let now = chrono::Utc::now();
    let insert = sqlx::query(
        r#"INSERT INTO user_favorites
           (id, created_at, updated_at, deleted_at, created_by_id, updated_by_id,
            workspace_id, project_id, user_id, entity_type, entity_identifier,
            name, is_folder, sequence, parent_id)
           VALUES ($1, $2, $2, NULL, $3, NULL, $4, $5, $3, 'module', $6,
                   NULL, FALSE, $7, NULL)"#,
    )
    .bind(uuid::Uuid::new_v4())
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(module)
    .bind(sequence)
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        return Err(integrity_denial(&error));
    }
    Ok(empty_response(StatusCode::NO_CONTENT))
}

/// `DELETE` user-favorite-modules detail: hard delete (`soft=False`) of
/// the caller's row, 204 empty; a miss is the `DoesNotExist` 404.
async fn favorite_destroy(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let pool = pool_of(&state)?;
    let user_id = match actor_or_401(extension) {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let project_id = match resolve_project_or_404(&pool, &slug, &project_raw).await {
        Ok(id) => id,
        Err(response) => return Ok(response),
    };
    let gate = gate_for(
        "DELETE",
        "workspaces/<slug>/projects/<project_id>/user-favorite-modules/<module_id>/",
    );
    let facts = fetch_class_facts(&pool, &slug, &project_id, &user_id).await?;
    if let Err(response) = check_class_gate(gate, "DELETE", &slug, &facts) {
        return Ok(response);
    }
    // `TimezoneMixin` activation (bad zone → 500).
    let _ = super::handlers_modules::actor_timezone(&pool, &user_id).await?;
    let module_id = parse_uuid_or_invalid(&module_raw)?;
    // `.get(...)` (`base.py:814-820`) over live rows, workspace-scoped.
    let found: Option<(uuid::Uuid,)> = sqlx::query_as(
        r#"SELECT uf.id FROM user_favorites uf
           JOIN workspaces w ON w.id = uf.workspace_id
           WHERE uf.project_id = $1 AND uf.user_id = $2 AND w.slug = $3
           AND uf.entity_type = 'module' AND uf.entity_identifier = $4
           AND uf.deleted_at IS NULL"#,
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&slug)
    .bind(module_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some((id,)) = found else {
        return Err(Denial::NotFound);
    };
    sqlx::query(r#"DELETE FROM user_favorites WHERE id = $1"#)
        .bind(id)
        .execute(&pool)
        .await
        .map_err(|_| Denial::ServerError)?;
    Ok(empty_response(StatusCode::NO_CONTENT))
}

// ---------------------------------------------------------------------------
// User properties (`ModuleUserPropertiesEndpoint`, `base.py:825-855`)
// ---------------------------------------------------------------------------

/// Serializer wire order: model field order (`id`, audit stamps,
/// `deleted_at`, the four JSON columns, audit users, project/workspace,
/// then the local FKs `module`/`user`) — the `BaseSerializer` render order
/// (`module.py:276-280`), verified live against `PROPS_KEYS`.
const PROPS_KEY_ORDER: &[&str] = &[
    "id",
    "created_at",
    "updated_at",
    "deleted_at",
    "filters",
    "display_filters",
    "display_properties",
    "rich_filters",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "module",
    "user",
];

/// `get_default_filters` (`db/models/module.py:14-25`), key order included.
fn default_filters() -> Value {
    Value::Object(
        [
            "priority",
            "state",
            "state_group",
            "assignees",
            "created_by",
            "labels",
            "start_date",
            "target_date",
            "subscriber",
        ]
        .into_iter()
        .map(|key| (key.to_owned(), Value::Null))
        .collect(),
    )
}

/// `get_default_display_filters` (`db/models/module.py:28-37`).
fn default_display_filters() -> Value {
    Value::Object(
        [
            ("group_by", Value::Null),
            ("order_by", Value::String("-created_at".to_owned())),
            ("type", Value::Null),
            ("sub_issue", Value::Bool(true)),
            ("show_empty_groups", Value::Bool(true)),
            ("layout", Value::String("list".to_owned())),
            ("calendar_date_range", Value::String(String::new())),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect(),
    )
}

/// `get_default_display_properties` (`db/models/module.py:40-55`).
fn default_display_properties() -> Value {
    Value::Object(
        [
            "assignee",
            "attachment_count",
            "created_on",
            "due_date",
            "estimate",
            "key",
            "labels",
            "link",
            "priority",
            "start_date",
            "state",
            "sub_issue_count",
            "updated_on",
        ]
        .into_iter()
        .map(|key| (key.to_owned(), Value::Bool(true)))
        .collect(),
    )
}

/// Render one user-properties row: `PROPS_KEY_ORDER`, datetimes in the
/// request zone (serializer path).
fn shape_user_props(row: &Map<String, Value>, timezone: &chrono_tz::Tz) -> String {
    let mut out = String::from("{");
    for (index, field) in PROPS_KEY_ORDER.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(field);
        out.push_str("\":");
        let value = row.get(*field).unwrap_or(&Value::Null);
        if matches!(*field, "created_at" | "updated_at" | "deleted_at") {
            out.push_str(&shift_datetime(value, timezone));
        } else {
            out.push_str(&serde_json::to_string(value).unwrap_or("null".to_owned()));
        }
    }
    out.push('}');
    out
}

/// Fetch one live user-properties row (`user`, `project`, `module`,
/// workspace slug), or `None`.
async fn fetch_user_props(
    pool: &sqlx::PgPool,
    slug: &str,
    project_id: &uuid::Uuid,
    module_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<Option<Map<String, Value>>, Denial> {
    use sqlx::Row;
    let row: Option<sqlx::postgres::PgRow> = sqlx::query(
        r#"SELECT row_to_json(__r)::text AS __row FROM (
             SELECT mup.id, mup.created_at, mup.updated_at, mup.deleted_at,
                    mup.filters, mup.display_filters, mup.display_properties,
                    mup.rich_filters, mup.created_by_id AS created_by,
                    mup.updated_by_id AS updated_by, mup.project_id AS project,
                    mup.workspace_id AS workspace, mup.module_id AS module,
                    mup.user_id AS "user"
             FROM module_user_properties mup
             JOIN workspaces w ON w.id = mup.workspace_id
             WHERE mup.user_id = $1 AND mup.project_id = $2 AND mup.module_id = $3
             AND w.slug = $4 AND mup.deleted_at IS NULL) AS __r"#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(module_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let text: String = row.try_get("__row").map_err(|_| Denial::ServerError)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| Denial::ServerError)?;
    match value {
        Value::Object(map) => Ok(Some(map)),
        _ => Err(Denial::ServerError),
    }
}

/// Shared user-properties preamble: auth → rewrite → the
/// ADMIN/MEMBER/GUEST allow-gate → zone → module id.
struct UserPropsContext {
    pool: sqlx::PgPool,
    project_id: uuid::Uuid,
    module_id: uuid::Uuid,
    user_id: uuid::Uuid,
    timezone: chrono_tz::Tz,
}

async fn user_props_context(
    state: &AppState,
    slug: &str,
    project_raw: &str,
    module_raw: &str,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    method: &str,
) -> Result<UserPropsContext, axum::response::Response> {
    let pool = pool_of(state).map_err(|denial| denial.into_response())?;
    let user_id = actor_or_401(extension)?;
    let project_id = resolve_project_or_404(&pool, slug, project_raw).await?;
    let gate = gate_for(
        method,
        "workspaces/<slug>/projects/<project_id>/modules/<module_id>/user-properties/",
    );
    let facts = fetch_allow_facts(&pool, slug, &project_id, &user_id, gate_roles(gate))
        .await
        .map_err(|denial| denial.into_response())?;
    check_gate(gate, slug, &facts).map_err(|denial| denial.into_response())?;
    let timezone = super::handlers_modules::actor_timezone(&pool, &user_id)
        .await
        .map_err(|denial| denial.into_response())?;
    let module_id = parse_uuid_or_invalid(module_raw).map_err(|denial| denial.into_response())?;
    Ok(UserPropsContext {
        pool,
        project_id,
        module_id,
        user_id,
        timezone,
    })
}

/// `GET` user-properties: `get_or_create` with the model defaults, 200
/// with the row. A missing module row fails the FK on insert →
/// `IntegrityError` → 400 (the `get_or_create` retry re-raises).
async fn user_props_get(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> HandlerResult {
    let context = match user_props_context(
        &state,
        &slug,
        &project_raw,
        &module_raw,
        extension,
        "GET",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let UserPropsContext {
        pool,
        project_id,
        module_id,
        user_id,
        timezone,
    } = context;
    if let Some(row) = fetch_user_props(&pool, &slug, &project_id, &module_id, &user_id).await? {
        return Ok(json_response(
            StatusCode::OK,
            shape_user_props(&row, &timezone),
        ));
    }
    let workspace_id = link_project_workspace(&pool, &project_id).await?;
    let id = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let filters = default_filters();
    let display_filters = default_display_filters();
    let display_properties = default_display_properties();
    let insert = sqlx::query(
        r#"INSERT INTO module_user_properties
           (id, created_at, updated_at, deleted_at, created_by_id, updated_by_id,
            workspace_id, project_id, module_id, user_id,
            filters, display_filters, display_properties, rich_filters)
           VALUES ($1, $2, $2, NULL, $3, NULL, $4, $5, $6, $3, $7, $8, $9, '{}')"#,
    )
    .bind(id)
    .bind(now)
    .bind(user_id)
    .bind(workspace_id)
    .bind(project_id)
    .bind(module_id)
    .bind(sqlx::types::Json(filters.clone()))
    .bind(sqlx::types::Json(display_filters.clone()))
    .bind(sqlx::types::Json(display_properties.clone()))
    .execute(&pool)
    .await;
    if let Err(error) = insert {
        // A lost `get_or_create` race re-reads the winner (`get_or_create`
        // retries the `get`); a missing module row (FK) 400s.
        if let sqlx::Error::Database(db_error) = &error {
            if db_error.code().as_deref() == Some("23505") {
                if let Some(row) =
                    fetch_user_props(&pool, &slug, &project_id, &module_id, &user_id).await?
                {
                    return Ok(json_response(
                        StatusCode::OK,
                        shape_user_props(&row, &timezone),
                    ));
                }
            }
        }
        return Err(integrity_denial(&error));
    }
    // The response renders the saved instance, not a re-read: the default
    // dicts keep insertion order on this path (a re-read would show
    // `jsonb` normalization instead — verified live).
    let stamp = utc_stamp_text(&now);
    let mut row = Map::new();
    row.insert("id".to_owned(), Value::String(id.to_string()));
    row.insert("created_at".to_owned(), Value::String(stamp.clone()));
    row.insert("updated_at".to_owned(), Value::String(stamp));
    row.insert("deleted_at".to_owned(), Value::Null);
    row.insert("filters".to_owned(), filters);
    row.insert("display_filters".to_owned(), display_filters);
    row.insert("display_properties".to_owned(), display_properties);
    row.insert("rich_filters".to_owned(), Value::Object(Map::new()));
    row.insert("created_by".to_owned(), Value::String(user_id.to_string()));
    row.insert("updated_by".to_owned(), Value::Null);
    row.insert("project".to_owned(), Value::String(project_id.to_string()));
    row.insert(
        "workspace".to_owned(),
        Value::String(workspace_id.to_string()),
    );
    row.insert("module".to_owned(), Value::String(module_id.to_string()));
    row.insert("user".to_owned(), Value::String(user_id.to_string()));
    Ok(json_response(
        StatusCode::OK,
        shape_user_props(&row, &timezone),
    ))
}

/// `PATCH` user-properties: wholesale per-key replace over
/// `filters`/`rich_filters`/`display_filters`/`display_properties`
/// (missing keys keep, explicit nulls 400 on the `NOT NULL` columns),
/// then save. 201 with the row (the status quirk); without a prior row,
/// `.get()` 404s.
async fn user_props_patch(
    State(state): State<AppState>,
    Path((slug, project_raw, module_raw)): Path<(String, String, String)>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
    body: axum::Json<Value>,
) -> HandlerResult {
    let context = match user_props_context(
        &state,
        &slug,
        &project_raw,
        &module_raw,
        extension,
        "PATCH",
    )
    .await
    {
        Ok(context) => context,
        Err(response) => return Ok(response),
    };
    let UserPropsContext {
        pool,
        project_id,
        module_id,
        user_id,
        timezone,
    } = context;
    let Some(map) = body.as_object() else {
        return Err(Denial::ServerError);
    };
    let row = fetch_user_props(&pool, &slug, &project_id, &module_id, &user_id)
        .await?
        .ok_or(Denial::NotFound)?;
    let id = row
        .get("id")
        .and_then(|value| value.as_str())
        .and_then(|text| text.parse::<uuid::Uuid>().ok())
        .ok_or(Denial::ServerError)?;
    // `request.data.get(key, current)` per key (`base.py:835-838`):
    // wholesale replace, unknown keys ignored. An explicit null hits the
    // `NOT NULL` column on save → `IntegrityError` → 400 (verified live:
    // the `I5` probe).
    let mut updated = row;
    for key in [
        "filters",
        "rich_filters",
        "display_filters",
        "display_properties",
    ] {
        if let Some(value) = map.get(key) {
            if value.is_null() {
                return Err(payload_denial());
            }
            updated.insert(key.to_owned(), value.clone());
        }
    }
    let now = chrono::Utc::now();
    sqlx::query(
        r#"UPDATE module_user_properties
           SET filters = $2, rich_filters = $3, display_filters = $4,
               display_properties = $5, updated_at = $6, updated_by_id = $7
           WHERE id = $1"#,
    )
    .bind(id)
    .bind(sqlx::types::Json(
        updated.get("filters").cloned().unwrap_or(Value::Null),
    ))
    .bind(sqlx::types::Json(
        updated.get("rich_filters").cloned().unwrap_or(Value::Null),
    ))
    .bind(sqlx::types::Json(
        updated
            .get("display_filters")
            .cloned()
            .unwrap_or(Value::Null),
    ))
    .bind(sqlx::types::Json(
        updated
            .get("display_properties")
            .cloned()
            .unwrap_or(Value::Null),
    ))
    .bind(now)
    .bind(user_id)
    .execute(&pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    updated.insert("updated_at".to_owned(), Value::String(utc_stamp_text(&now)));
    updated.insert("updated_by".to_owned(), Value::String(user_id.to_string()));
    Ok(json_response(
        StatusCode::CREATED,
        shape_user_props(&updated, &timezone),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    fn validate(body: &Map<String, Value>, partial: bool) -> Result<LinkInput, String> {
        match validate_link_body(body, partial, &chrono_tz::UTC) {
            Ok(input) => Ok(input),
            Err(_) => Err("response".to_owned()),
        }
    }

    #[test]
    fn link_field_error_bodies_match_drf() {
        // Required / null / blank / type ladders, verbatim.
        let missing = validate(&body(&[]), false).unwrap_err();
        assert_eq!(missing, "response");
        let errors: FieldErrors =
            vec![("url".to_owned(), vec!["This field is required.".to_owned()])];
        assert_eq!(
            render_field_errors(&errors),
            r#"{"url":["This field is required."]}"#
        );
        assert!(validate(&body(&[]), true).is_ok());
        assert!(validate(&body(&[("url", Value::Null)]), true).is_err());
        let input =
            validate(&body(&[("url", json!("http://example.com/x"))]), false).expect("valid url");
        assert_eq!(input.url.as_deref(), Some("http://example.com/x"));
    }

    #[test]
    fn blank_url_reports_only_the_blank_error() {
        // `CharField.run_validation` fails blank input before validators
        // run (verified live).
        assert!(validate(&body(&[("url", json!(""))]), false).is_err());
    }

    #[test]
    fn url_field_never_says_not_a_valid_string() {
        // `fail('invalid')` renders through `URLField`'s overridden
        // message (verified live: G05/G33/G34).
        for value in [json!(false), json!([]), json!({}), json!(0)] {
            let prefixed = prefix_link_url(Some(&value)).expect("no 500");
            assert!(prefixed.is_some());
            assert!(validate(&body(&[("url", value)]), false).is_err());
        }
        for value in [
            json!(5),
            json!(4.5),
            json!(true),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert!(prefix_link_url(Some(&value)).is_err());
        }
    }

    #[test]
    fn link_error_keys_follow_writable_field_order() {
        let errors = vec![
            ("metadata".to_owned(), vec!["m".to_owned()]),
            ("url".to_owned(), vec!["u".to_owned()]),
            ("title".to_owned(), vec!["t".to_owned()]),
            ("deleted_at".to_owned(), vec!["d".to_owned()]),
        ];
        let mut sorted = errors;
        sorted.sort_by_key(|(field, _)| {
            LINK_WRITE_FIELDS
                .iter()
                .position(|name| name == field)
                .unwrap_or(usize::MAX)
        });
        assert_eq!(
            render_field_errors(&sorted),
            r#"{"deleted_at":["d"],"title":["t"],"url":["u"],"metadata":["m"]}"#
        );
    }

    #[test]
    fn title_numerics_stringify_bools_fail() {
        assert_eq!(coerce_link_string(&json!(5)).as_deref(), Some("5"));
        assert_eq!(coerce_link_string(&json!(4.5)).as_deref(), Some("4.5"));
        assert!(coerce_link_string(&json!(true)).is_none());
        assert!(coerce_link_string(&json!([1])).is_none());
        assert!(coerce_link_string(&json!({"a": 1})).is_none());
    }

    #[test]
    fn django_url_corpus_matches_validator() {
        // Every verdict below was recorded from Django 4.2's real
        // `URLValidator.__call__` (`/tmp/url_corpus.py` ground truth).
        for (url, expected) in [
            ("http://example.com/", true),
            ("https://example.com/specs", true),
            ("http://example.com/docs", true),
            ("http://localhost:8000/x", true),
            ("http://127.0.0.1/", true),
            ("http://[::1]/", true),
            ("http://user:pass@example.com/", true),
            ("http://example.com:8080/a?b=c#d", true),
            ("ftp://example.com/f", true),
            ("HTTP://EXAMPLE.COM/", true),
            ("http://example.com./", true),
            ("http://xn--nxasmq6b.example/", true),
            ("http://not a url at all !!", false),
            ("not a url at all !!", false),
            ("http://", false),
            ("http://nodot", false),
            ("http://-bad.example/", false),
            ("http://bad-.example/", false),
            ("http://bad..example/", false),
            ("http://example.com:999999/", false),
            ("http://example.com:port/", false),
            ("gopher://example.com/", false),
            ("//example.com/", false),
            ("", false),
            ("http://exa_mple.com/", false),
            ("http://example.com/a b", false),
            ("http://m\u{fc}nchen.de/", true),
            ("http://example.com", true),
            ("http://a.bc/", true),
            ("http://user@example.com/", true),
            ("http://user:@example.com/", true),
            ("http://example.com:0/", true),
            ("http://example.com#frag", true),
            ("http://example.com?query", true),
            ("http://example.com/path", true),
            ("http://ab--cd.ef/", true),
            ("http://example.co/", true),
            ("http://[2001:db8::1]/", true),
            ("http://[2001:db8::1]:8080/x", true),
            ("ftps://example.com/", true),
            ("FTP://EXAMPLE.COM/X", true),
            ("http://example.com./x", true),
            ("http://example.com/\0", true),
            ("http://a.b/", false),
            ("http://1.2.3.4.5/", false),
            ("http://256.1.1.1/", false),
            ("http://1.2.3/", false),
            ("http://[::1/", false),
            ("http://[gggg]/", false),
            ("http://:pass@example.com/", false),
            ("http://us er@example.com/", false),
            ("http://example.com:/", false),
            ("mailto:a@example.com", false),
            ("http:///example.com/", false),
            ("http:///x", false),
            ("http:/example.com/", false),
            ("http://-a.bc/", false),
            ("http://a-.bc/", false),
            ("http://a..b/", false),
            ("http://example.123/", false),
            ("http://example.c/", false),
            ("http://example.com:123456/", false),
            ("http://example.com:12a/", false),
            ("http://example.com:", false),
            ("http://example.com/\t", false),
            ("http://foo_bar.example/", false),
            ("http://exam ple.com/", false),
            ("http://w..com/", false),
            ("http://.example.com/", false),
            ("http://u:p@u:p@example.com/", false),
            ("http://toolonglabelxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.example/", false),
            ("http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.example/", false),
            ("http://u@[::ffff:1.2.3.999]/", false),
            ("http://[::ffff:1.2.3.999]/", false),
            ("http://u@[::1]/", true),
            ("http://u@m\u{fc}nchen.de:8080/x", true),
            ("http://m\u{fc}nchen.de:8080/x", true),
            ("http://u:p@m\u{fc}nchen.de/", true),
            ("http://example.com/a@b", true),
            ("http://00.1.2.3/", false),
            ("http://01.02.03.04/", false),
            ("http://example.a1/", false),
            ("http://XN--NXASMQ6B.example/", true),
            ("http://XN--ab_c.example/", false),
            ("http://a\u{1c}b@example.com/", false),
            ("http://example.com/a\u{1f}b", false),
            ("http://[::ffff:1.2.3.4]/", true),
            ("http://u@\u{1f4a9}.de/", false),
            ("http://u@\u{1f4a9}.de:8080/x", false),
            ("http://\u{1f4a9}.de:8080/x", true),
            ("http://u@exam_ple.de/", false),
            ("http://exam_ple.de/", false),
            ("http://a[b/", false),
            ("http://a]b/", false),
            ("http://a[b]c/", false),
            ("http://[::1]extra/", false),
            ("http://[[::1]]/", false),
            ("http://[::1", false),
            ("http://x|y/", false),
            ("http://x^y/", false),
            ("http://x`y/", false),
            ("http://x\\y/", false),
            ("http://%41.com/", false),
            ("http://\u{df}.de/", true),
            ("http://\u{3c2}.de/", true),
            ("http://\u{df}.com/", true),
            ("http://a\u{200b}b.com/", true),
            ("http://\u{feff}abc.com/", true),
            ("http://abc\u{feff}.com/", true),
            ("http://example.com./", true),
            ("http://.com/", false),
            ("http://com/", false),
            ("http://a-.com/", false),
            ("http://-a.com/", false),
            ("http://aa.com/", true),
            ("http://a.bb/", true),
            ("http://192.168.0.1/", true),
            ("http://0.0.0.0/", true),
            ("http://00.0.0.0/", false),
            ("http://[0:0:0:0:0:0:0:1]/", true),
            ("http://[0:0:0:0:0:0:0:1", false),
            ("http://u@[1:2:3:4:5:6:7:8]/", true),
            ("http://u[x@host/", false),
            ("http://[127.0.0.1]/", false),
            ("http://[127.0.0.1]/", false),
            ("http://[::]/", true),
            ("http://[1::2::3]/", false),
            ("http://[fe80::1%25eth0]/", false),
            ("http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.example/", true),
            ("http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.ddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.example/", false),
            ("http://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.dddddddddddddddddddddddddddddddddddddddddddddddddddddddddd.example/", false),
            ("http://example.com:65535/", true),
            ("http://example.com:00001/", true),
            ("http://example.com././x", true),
            ("http://?query", false),
            ("http://#frag", false),
            ("http://@example.com/", false),
            ("http://:@example.com/", false),
            ("http://u@h@x.example/", false),
            ("http://u:p:q@example.com/", false),
            ("http://example.com/\n", false),
            ("http://example.com/\r", false),
            ("http://\u{7f}example.com/", false),
            ("http://\u{1}example.com/", false),
            ("http://caf\u{e9}.example/", true),
            ("http://\u{65e5}\u{672c}.jp/", true),
            ("http://\u{65e5}\u{672c}/", false),
            ("http://a\u{65e5}\u{672c}b.example/", true),
            ("http://\u{1f4a9}.de/", true),
            ("http://x\u{1f4a9}y.de/", true),
            ("http://-\u{1f4a9}.de/", true),
            ("http://\u{1f4a9}-.de/", true),
            ("http://a..\u{1f4a9}.de/", false),
            ("https://", false),
            ("https:///x", false),
            ("ftp://u:p@h.io:21/f", true),
            ("ftps://[::1]:990/", true),
            ("http://example.com:80:80/", false),
            ("http://example.com/%41", true),
            ("http://example.com/%zz", true),
            ("HTTP://u:P@EXAMPLE.COM:80/A?B#C", true),
            ("http://localhost/", true),
            ("http://LOCALHOST/", true),
            ("http://localhost./", false),
            ("http://a.localhost/", true),
            ("http://127.0.0.1:80/", true),
            ("http://1.2.3.4/a@b/c@d", true),
            ("http://a@b@c/", false),
            ("http://a/b@c/d@e", false),
        ] {
            assert_eq!(django_url_valid(url), expected, "{url:?}");
        }
    }

    #[test]
    fn link_datetime_corpus_matches_django() {
        // Every (input, expected) pair below was recorded from Django 4.2
        // `parse_datetime` + DRF `enforce_timezone` under a UTC request zone
        // (`/tmp/dt_corpus.py` ground truth; naive inputs attach UTC).
        let utc = &chrono_tz::UTC;
        for (input, expected) in [
            ("2024-01-02", "2024-01-02T00:00:00+00:00"),
            ("20240102", "2024-01-02T00:00:00+00:00"),
            ("2024-01-02X03:04", "2024-01-02T03:04:00+00:00"),
            ("2024-01-02T03", "2024-01-02T03:00:00+00:00"),
            ("2024-01-02 03:04", "2024-01-02T03:04:00+00:00"),
            (
                "2024-01-02T03:04:05,123",
                "2024-01-02T03:04:05.123000+00:00",
            ),
            (
                "2024-01-02T03:04:05.123456789",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            ("2024-01-02T030405", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+05:30:00", "2024-01-01T21:34:05+00:00"),
            ("2024-01-02T03:04:05+0530", "2024-01-01T21:34:05+00:00"),
            ("2024-01-02T03:04:05+05", "2024-01-01T22:04:05+00:00"),
            ("2024-01-02T03:04:05z", "INVALID"),
            ("2024-01-02T03:04:05 ", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+00:99", "2024-01-02T01:25:05+00:00"),
            ("2024-01-02T03:04:05+24:00", "INVALID"),
            ("2024-01-02T24:00:00", "INVALID"),
            ("2024-13-02T03:04:05", "INVALID"),
            ("2024-01-02T03:04:05\n", "2024-01-02T03:04:05+00:00"),
            (
                "2024-01-02T03:04:05.1234567",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            ("2024-02-30T00:00:00", "INVALID"),
            ("24-01-02T03:04:05", "INVALID"),
            ("2024-1-2T3:4:5", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05.  ", "INVALID"),
            (" 2024-01-02T03:04:05", "INVALID"),
            ("2024-01-02XX03:04", "INVALID"),
            ("20240102T030405", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T0304", "2024-01-02T03:04:00+00:00"),
            ("2024-01-02T03Z", "2024-01-02T03:00:00+00:00"),
            ("2024-01-02T03:04:05+053025", "2024-01-01T21:33:40+00:00"),
            (
                "2024-01-02T03:04:05+05:30:45.5",
                "2024-01-01T21:33:19.500000+00:00",
            ),
            ("2024-01-02Z", "INVALID"),
            ("2024-01-02T03:04:05ZZ", "INVALID"),
            ("2024-01-02T03:04:05+05:30 ", "INVALID"),
            ("2024-01-02T03:04:05 +05:30", "2024-01-01T21:34:05+00:00"),
            (
                "2024-01-02T03:04:05.123Z",
                "2024-01-02T03:04:05.123000+00:00",
            ),
            ("2024-W01-1", "2024-01-01T00:00:00+00:00"),
            ("2024-002", "INVALID"),
            ("2024-01-02T3:4:5,1234567890123", "INVALID"),
            ("2024-01-02T03:04:60", "INVALID"),
            ("2024-01-02T03:04:05+23:59", "2024-01-01T03:05:05+00:00"),
            ("2024-01-02T03:04:05-23:59", "2024-01-03T03:03:05+00:00"),
            ("2024-01-02T03:04:05+00:00", "2024-01-02T03:04:05+00:00"),
            ("0001-01-01T00:00:00", "0001-01-01T00:00:00+00:00"),
            ("9999-12-31T23:59:59", "9999-12-31T23:59:59+00:00"),
            ("2024-01-02TT03:04", "INVALID"),
            ("2024-01-02T", "INVALID"),
            ("2024-01-02T03:", "INVALID"),
            ("2024-01-02T:04:05", "INVALID"),
            ("2024W011", "2024-01-01T00:00:00+00:00"),
            ("2024-W01", "2024-01-01T00:00:00+00:00"),
            ("2024W01-1", "INVALID"),
            ("2024-01-02003:04", "2024-01-02T03:04:00+00:00"),
            ("20240102 0304", "2024-01-02T03:04:00+00:00"),
            ("2024-01-02T030", "INVALID"),
            (
                "2024-01-02T03:04:05.1234567890",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T03:04:05.12345678901",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T03:04:05.123456789012",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            ("2024-01-02T03:04:05Z\n", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+05:30\n", "2024-01-01T21:34:05+00:00"),
            ("2024-01-02T03:04:05\t", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05\u{0b}", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+5:30", "INVALID"),
            ("2024-01-02T03:04:05+053", "INVALID"),
            ("2024/01/02T03:04:05", "INVALID"),
            ("2024-0102T03:04:05", "INVALID"),
            ("2024-01-02T03:04:05+05:30:99", "2024-01-01T21:32:26+00:00"),
            ("2024-01-02T03:04:05+99:99", "INVALID"),
            ("2024-01-02T03:04:05+1", "INVALID"),
            ("2024-01-02T03:04:05,1", "2024-01-02T03:04:05.100000+00:00"),
            (
                "2024-01-02T03:04:05,123456",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T03:04:05,1234567",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            ("2024-1-02T030405", "INVALID"),
            ("2024-1-02T03Z", "INVALID"),
            ("2024-01-02T3Z", "INVALID"),
            ("2024-1-2", "INVALID"),
            ("2024-W01-1T03:04", "2024-01-01T03:04:00+00:00"),
            ("2024W01", "2024-01-01T00:00:00+00:00"),
            ("2024002", "INVALID"),
            ("2024-01-02T03:04:05+05:3045", "INVALID"),
            (
                "2024-01-02T03:04:05+05:30:45,5",
                "2024-01-01T21:33:19.500000+00:00",
            ),
            ("2024-01-02T03:04:05.", "INVALID"),
            ("2024-01-02T03:04:05,", "INVALID"),
            ("2024-01-02T03:04:05+24", "INVALID"),
            ("2024-W54-1", "INVALID"),
            ("2023-W52-8", "INVALID"),
            ("2024-W00-1", "INVALID"),
            ("2024-01-02t03:04:05", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05Z ", "INVALID"),
            ("2024-01-02T03:04:05\r\n", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05 +05:30 ", "INVALID"),
            ("2024-01-02T03:04:05+053045", "2024-01-01T21:33:20+00:00"),
            ("2024-13-45T99:99:99", "INVALID"),
            ("abcd-01-02T03:04:05", "INVALID"),
            ("2024-01-02T03:04:05+05:3", "INVALID"),
            ("2024-01-02T03:04:05+053:45", "INVALID"),
            ("20240402T03:04", "2024-04-02T03:04:00+00:00"),
            ("+2024-01-02T03:04:05", "INVALID"),
            ("2024-01-02T3", "INVALID"),
            ("2024-01-02T3:04", "2024-01-02T03:04:00+00:00"),
            ("2024-01-02T03:4", "2024-01-02T03:04:00+00:00"),
            ("2024-01-02T3:4", "2024-01-02T03:04:00+00:00"),
            (
                "2024-01-02T03:04:05+05:30:45.1234567",
                "2024-01-01T21:33:19.876544+00:00",
            ),
            ("2024W011T03:04", "2024-01-01T03:04:00+00:00"),
            ("2024-W01T03:04:05Z", "2024-01-01T03:04:05+00:00"),
            ("2024-01-02T030405,5", "2024-01-02T03:04:05.500000+00:00"),
            (
                "2024-01-02T030405.1234567",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T030405,1234567",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            ("2024-01-02T03:04:05+05:", "INVALID"),
            ("2024-01-02T03:04:05+0534", "2024-01-01T21:30:05+00:00"),
            ("2024-01-02T03:04:05+05:30:4", "INVALID"),
            ("2024-01-02T03:04:05+05:30:456", "INVALID"),
            ("2024-01-02T03:04:05+05:30:45.", "INVALID"),
            ("2024-01-02T03:04:05+05:30:45,", "INVALID"),
            ("2024-01-02T03:04:05+013:45", "INVALID"),
            ("2024-01-02T03:04:05+00", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05-00", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+0000", "2024-01-02T03:04:05+00:00"),
            (
                "2024-01-02T03:04:05+00:00:00.000001",
                "2024-01-02T03:04:05+00:00",
            ),
            (
                "2024-01-02T03:04:05.1234567890123",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T030405.1234567890123",
                "2024-01-02T03:04:05.123456+00:00",
            ),
            (
                "2024-01-02T03:04:05+05:30:45.1234567890123",
                "2024-01-01T21:33:19.876544+00:00",
            ),
            ("2024-W011", "INVALID"),
            ("2020-W53-7", "2021-01-03T00:00:00+00:00"),
            ("2021-W53-1", "INVALID"),
            ("2024-01-02T0304:05", "INVALID"),
            (
                "2024-01-02T03:04:05+05:30.5",
                "2024-01-01T21:34:04.500000+00:00",
            ),
            ("2024-01-02TZ", "INVALID"),
            ("2024-01-02T03+05:30", "2024-01-01T21:30:00+00:00"),
            ("2024-01-02T0304+0530", "2024-01-01T21:34:00+00:00"),
            (
                "2024-01-02T03:04:05+05:30:45.123456789012345678901234567890",
                "2024-01-01T21:33:19.876544+00:00",
            ),
            ("2019-W01-1", "2018-12-31T00:00:00+00:00"),
            (
                "2024-01-02T03:04:05+05:30:45,12345678901234567890",
                "2024-01-01T21:33:19.876544+00:00",
            ),
            (
                "2024-01-02T03:04:05+05.5",
                "2024-01-01T22:04:04.500000+00:00",
            ),
            (
                "2024-01-02T03:04:05+0530.5",
                "2024-01-01T21:34:04.500000+00:00",
            ),
            (
                "2024-01-02T03:04:05+05,5",
                "2024-01-01T22:04:04.500000+00:00",
            ),
            (
                "2024-01-02T03:04:05+0530,5",
                "2024-01-01T21:34:04.500000+00:00",
            ),
            (
                "2024-01-02T03:04:05-05.5",
                "2024-01-02T08:04:05.500000+00:00",
            ),
            ("2024-01-02T03:04:05+00.000001", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03+05.5", "2024-01-01T21:59:59.500000+00:00"),
            (
                "2024-01-02T03:04:05+23.999999",
                "2024-01-01T04:04:04.000001+00:00",
            ),
            ("2024-01-02T03:04:05+00.000002", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+00.000010", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+00.100000", "2024-01-02T03:04:05+00:00"),
            (
                "2024-01-02T03:04:05.000001",
                "2024-01-02T03:04:05.000001+00:00",
            ),
            (
                "2024-01-02T03:04:05.000009",
                "2024-01-02T03:04:05.000009+00:00",
            ),
            (
                "2024-01-02T03:04:05+00:00:00.000002",
                "2024-01-02T03:04:05+00:00",
            ),
            (
                "2024-01-02T03:04:05+00:00:01.000001",
                "2024-01-02T03:04:03.999999+00:00",
            ),
            ("2024-01-02T03:04:05.0000005", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05+00:00.5", "2024-01-02T03:04:05+00:00"),
            (
                "2024-01-02T03:04:05+01.000001",
                "2024-01-02T02:04:04.999999+00:00",
            ),
            (
                "2024-01-02T03:04:05+00:01.000001",
                "2024-01-02T03:03:04.999999+00:00",
            ),
            (
                "2024-01-02T03:04:05+00:00:00.5",
                "2024-01-02T03:04:05+00:00",
            ),
            (
                "2024-01-02T03:04:05+00:00:00.0000001",
                "2024-01-02T03:04:05+00:00",
            ),
            (
                "2024-01-02T03:04:05+0100.000001",
                "2024-01-02T02:04:04.999999+00:00",
            ),
            ("2024-01-02+05:30", "2024-01-02T05:30:00+00:00"),
            ("2024-01-02T03.5", "2024-01-02T03:00:00.500000+00:00"),
            ("2024-01-02T03:04.5", "2024-01-02T03:04:00.500000+00:00"),
            ("2024-01-02T03:04:05-00.5", "2024-01-02T03:04:05+00:00"),
            ("20240102T3:04", "INVALID"),
            ("2024-01-02T0304.5", "2024-01-02T03:04:00.500000+00:00"),
            ("2024-W01-1T03:04+05:30", "2023-12-31T21:34:00+00:00"),
            ("9999-12-31T23:59:59-14:00", "OVERFLOW"),
            ("0001-01-01T00:00:00+14:00", "OVERFLOW"),
            (
                "2024-01-02T03:04:05.12+05:30",
                "2024-01-01T21:34:05.120000+00:00",
            ),
            (
                "2024-01-02T03:04:05,12+05:30",
                "2024-01-01T21:34:05.120000+00:00",
            ),
            ("2024-06-10T02:30:00", "2024-06-10T02:30:00+00:00"),
            ("2024-06-10", "2024-06-10T00:00:00+00:00"),
            ("nope", "INVALID"),
            ("", "INVALID"),
            ("5", "INVALID"),
            ("2024-01-02T03:04:05+14:00", "2024-01-01T13:04:05+00:00"),
            ("2024-01-02T03:04:05-14:00", "2024-01-02T17:04:05+00:00"),
            ("2024-01-02T03:04:05+13:59", "2024-01-01T13:05:05+00:00"),
            ("2024-01-02T03:04:05+00:60", "2024-01-02T02:04:05+00:00"),
            ("2024-01-02 03:04:05", "2024-01-02T03:04:05+00:00"),
            ("2024-1-2 3:4:5", "2024-01-02T03:04:05+00:00"),
            ("2024-01-02T03:04:05\u{a0}", "2024-01-02T03:04:05+00:00"),
            (
                "2024-01-02T03:04:05\u{2003}+05:30",
                "2024-01-01T21:34:05+00:00",
            ),
            ("2024-W01-1 ", "INVALID"),
            ("0000-01-01", "INVALID"),
            ("0000-01-01T00:00:00Z", "INVALID"),
            ("0000-W01-1", "INVALID"),
            ("00000101", "INVALID"),
            ("10000-01-01T00:00:00", "INVALID"),
        ] {
            let got = resolve_link_datetime_input(&json!(input), utc);
            match expected {
                "INVALID" => assert_eq!(
                    got.err().as_deref(),
                    Some(link_datetime_invalid_message().as_str()),
                    "{input}",
                ),
                "OVERFLOW" => assert_eq!(
                    got.err().as_deref(),
                    Some(link_datetime_overflow_message().as_str()),
                    "{input}",
                ),
                instant => {
                    let want = chrono::DateTime::parse_from_rfc3339(instant)
                        .expect("corpus instant")
                        .timestamp_micros();
                    assert_eq!(
                        got.ok().map(|stamp| stamp.timestamp_micros()),
                        Some(want),
                        "{input}",
                    );
                }
            }
        }
    }

    #[test]
    fn favorite_module_inputs_follow_uuid_field() {
        assert_eq!(parse_favorite_module(None).unwrap(), None);
        assert_eq!(parse_favorite_module(Some(&Value::Null)).unwrap(), None);
        let id = uuid::Uuid::new_v4();
        assert_eq!(
            parse_favorite_module(Some(&json!(id.to_string()))).unwrap(),
            Some(id)
        );
        // `uuid.UUID(hex=...)` forms (verified live: braced 204).
        for text in [
            id.as_simple().to_string(),
            id.as_braced().to_string(),
            id.as_urn().to_string(),
            id.to_string().to_uppercase(),
        ] {
            assert_eq!(
                parse_favorite_module(Some(&json!(text))).unwrap(),
                Some(id),
                "{text}"
            );
        }
        assert_eq!(
            parse_favorite_module(Some(&json!(5))).unwrap(),
            Some(uuid::Uuid::from_u128(5))
        );
        assert_eq!(
            parse_favorite_module(Some(&json!(true))).unwrap(),
            Some(uuid::Uuid::from_u128(1))
        );
        for bad in [
            json!("xyz"),
            json!(-3),
            json!(1.5),
            json!([1]),
            json!({"a": 1}),
        ] {
            assert!(parse_favorite_module(Some(&bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn user_props_defaults_match_model() {
        assert_eq!(
            default_filters(),
            json!({
                "priority": null, "state": null, "state_group": null,
                "assignees": null, "created_by": null, "labels": null,
                "start_date": null, "target_date": null, "subscriber": null,
            })
        );
        assert_eq!(
            default_display_filters(),
            json!({
                "group_by": null, "order_by": "-created_at", "type": null,
                "sub_issue": true, "show_empty_groups": true,
                "layout": "list", "calendar_date_range": "",
            })
        );
        let props = default_display_properties();
        assert_eq!(props.as_object().expect("object").len(), 13);
        assert!(props
            .as_object()
            .expect("object")
            .values()
            .all(|v| *v == json!(true)));
        assert_eq!(
            PROPS_KEY_ORDER,
            &[
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "filters",
                "display_filters",
                "display_properties",
                "rich_filters",
                "created_by",
                "updated_by",
                "project",
                "workspace",
                "module",
                "user",
            ]
        );
    }
}
