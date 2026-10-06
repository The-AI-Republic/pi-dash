//! D-13 daemon authentication extractors (`runner/authentication.py:46-254`).
//!
//! Ports the three DRF authentication classes plus the consumed
//! `APIKeyAuthentication` behaviour as axum-layer resolver functions in the
//! pilot-2 `Gate` style (plain async resolvers like
//! `crate::v1_cycles_modules::module::actor` and
//! `crate::runner_runs::authenticate_daemon`, not `FromRequestParts` — the
//! resolvers need `&PgPool`, the secret key, the JWT ring, and the URL
//! `runner_id`, all of which handlers already hold).
//!
//! Fixture ids D13-F4 (`guards/authn.golden.json`) and D13-F7
//! (`handlers/endpoints.golden.json`): every failure code below matches
//! F4's `failure_codes_in_order` lists, and every denial renders the F4
//! `drf_failure_shapes` bytes.
//!
//! Units (in Python order):
//!
//! * [`authenticate_access_token`] — `RunnerAccessTokenAuthentication`
//!   (`authentication.py:56-180`): `mt_` machine-token path plus legacy
//!   JWT path, every failure code, in order.
//! * [`parse_refresh_token`] — `RunnerRefreshTokenAuthentication`
//!   (`authentication.py:183-202`): header parse only, `(None, None)`
//!   user semantics.
//! * [`authenticate_machine_token`] — `MachineTokenAuthentication`
//!   (`authentication.py:205-240`): `mt_`-only path with best-effort
//!   `last_used_at`.
//! * [`authenticate_api_key`] — `APIKeyAuthentication` behaviour
//!   (`api/middleware/api_authentication.py:20-76`, trace-only: the file
//!   is owned elsewhere): `mt_` machine branch plus `APIToken` branch,
//!   single opaque failure message.
//! * [`resolve_runner_for_run`] — the `design.md` §7.5 per-run predicate
//!   (`authentication.py:243-254`).
//!
//! Kernel reuse (all read-only): [`pidash_auth::token`] (classify, hash,
//! validators) and [`pidash_auth::jwt`] (decode + `KeyRing`). Row fetching
//! is this module's own SQL. The membership predicate is inlined as
//! `role.is_some()` — the exact body of the
//! `auth/src/permissions/membership.rs` kernel, which exists but is not
//! wired into `permissions.rs`, so it is unreachable (the D-20/D-21 ports
//! inline the same check); a foundation follow-up can wire the module and
//! this call site can switch to it with no behavior change.
//!
//! ## Denial statuses: port the view, not just the class
//!
//! DRF's `APIView.handle_exception` (`views.py:458-466`) coerces an
//! `AuthenticationFailed`/`NotAuthenticated` to **403** when the view's
//! *first* authenticator provides no `WWW-Authenticate` challenge, and
//! answers **401** with that challenge otherwise. (`NotAuthenticated`
//! never survives as 403: the project's `auth_exception_handler` forces it
//! back to 401 — see [`not_authenticated_response`].) `APIKeyAuthentication`
//! defines no `authenticate_header` (base returns `None`), so the same
//! `Given API token is not valid` failure is a 403 on the create endpoint
//! (`authentication_classes = [APIKeyAuthentication]`,
//! `enrollment.py:567`) and a 401 + `WWW-Authenticate: Bearer` on the
//! projects list (first authenticator `RunnerAccessTokenAuthentication`,
//! `projects.py:90-94`) — verified against the installed DRF
//! (`handle_exception` probe, 2026-10-03). F4's blanket "401" pin for the
//! API-key failure is therefore refined here per consuming view; the
//! contract suite already allows both
//! (`test_daemon_runner_create_requires_auth` asserts `(401, 403)`).
//! [`auth_failure_response`] takes the view's
//! `get_authenticate_header` output so handlers render exactly this.
//!
//! ## Intentional deviations from sibling ports (all closer to Python)
//!
//! * The `api_tokens` lookup keeps the manager's `deleted_at IS NULL`
//!   predicate (`AuditModel` → `SoftDeleteModel.objects`,
//!   `db/mixins.py:49-58`); the D-20 copy omits it.
//! * [`bearer_token`] splits the raw header bytes *before* UTF-8 decoding,
//!   mirroring `_bearer`'s short-circuit (`len(parts) != 2` returns `None`
//!   without decoding); a non-2-part non-UTF-8 header is anonymous, not a
//!   500. (Observable only for malformed non-UTF-8 headers.)
//! * 500s render the sibling-consistent JSON body; Django renders its HTML
//!   error page there, so only the status is contract-pinned (same position
//!   as `crate::runner_runs::SERVER_ERROR_BODY`).

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use pidash_auth::jwt::KeyRing;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Failure codes (`authentication.py`, in first-use order)
// ---------------------------------------------------------------------------

/// `mt_` path: unknown token hash (`authentication.py:125`).
pub const CODE_MACHINE_TOKEN_INVALID: &str = "machine_token_invalid";
/// `mt_` path: token row revoked (`authentication.py:127`).
pub const CODE_MACHINE_TOKEN_REVOKED: &str = "machine_token_revoked";
/// Either path: the bound dev-machine row revoked (`authentication.py:129,143`).
pub const CODE_DEV_MACHINE_REVOKED: &str = "dev_machine_revoked";
/// Either `mt_` path: owner left the workspace; the token is revoked first,
/// then denied (`authentication.py:132,229`).
pub const CODE_MEMBERSHIP_REVOKED: &str = "membership_revoked";
/// `mt_` path: neither URL `runner_id` nor `X-Runner-Id`
/// (`authentication.py:136`).
pub const CODE_RUNNER_ID_REQUIRED: &str = "runner_id_required";
/// No `Runner` row for the id (`authentication.py:94,140`).
pub const CODE_RUNNER_NOT_FOUND: &str = "runner_not_found";
/// `Runner.revoked_at` set — the per-request live check, `design.md` §5.4
/// (`authentication.py:100,142`).
pub const CODE_RUNNER_REVOKED: &str = "runner_revoked";
/// Owner/workspace/machine/host binding violated (`authentication.py:146,149,151`).
pub const CODE_RUNNER_NOT_BOUND_TO_MACHINE_TOKEN: &str = "runner_not_bound_to_machine_token";
/// JWT path: `rtg < runner.refresh_token_generation - 1`
/// (`authentication.py:106`).
pub const CODE_ACCESS_TOKEN_STALE_RTG: &str = "access_token_stale_rtg";
/// JWT path: `RunnerForceRefresh` row exists and `rtg < min_rtg`
/// (`authentication.py:110`).
pub const CODE_FORCE_REFRESH_REQUIRED: &str = "force_refresh_required";
/// JWT path: URL `runner_id` present and != token runner
/// (`authentication.py:114`).
pub const CODE_RUNNER_ID_MISMATCH: &str = "runner_id_mismatch";
/// `APIKeyAuthentication` single opaque failure
/// (`api_authentication.py:40,54,56,58,61`).
pub const CODE_GIVEN_API_TOKEN_NOT_VALID: &str = "Given API token is not valid";
/// DRF `NotAuthenticated.default_detail`, rendered when no credential is
/// presented on a guarded view.
pub const CODE_CREDENTIALS_NOT_PROVIDED: &str = "Authentication credentials were not provided.";

/// `authenticate_header` of the three `runner/authentication.py` classes
/// (`authentication.py:164,202,240`): the `WWW-Authenticate` challenge on
/// every 401 they produce.
pub const AUTHENTICATE_HEADER_BEARER: &str = "Bearer";

/// Render an `AuthenticationFailed(code)` denial exactly like DRF's
/// `APIView.handle_exception` + `exception_handler`
/// (`views.py:458-466,71-101`): `{"detail": code}` (lowercase `d`, compact
/// JSON), 401 plus `WWW-Authenticate` when the view's first authenticator
/// supplies a challenge, else coerced to 403 with no challenge header.
/// (`NotAuthenticated` differs — see [`not_authenticated_response`].)
///
/// Pass the consuming view's `get_authenticate_header` output: `Some("Bearer")`
/// for every view whose first class is one of the three
/// `runner/authentication.py` classes (self-revoke, refresh, machine-command
/// result, projects, and the D-14 bearer-first views), `None` for the create
/// endpoint (`[APIKeyAuthentication]`).
pub fn auth_failure_response(code: &str, first_authenticate_header: Option<&str>) -> Response {
    let body = serde_json::json!({ "detail": code }).to_string();
    match first_authenticate_header {
        Some(challenge) => Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::WWW_AUTHENTICATE, challenge)
            .body(axum::body::Body::from(body)),
        None => Response::builder()
            .status(StatusCode::FORBIDDEN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body)),
    }
    .expect("auth denial builds")
}

/// [`auth_failure_response`] for the bearer classes: all their consuming
/// views are bearer-first, so the challenge is always `Bearer`.
pub fn bearer_failure_response(code: &str) -> Response {
    auth_failure_response(code, Some(AUTHENTICATE_HEADER_BEARER))
}

/// The missing-credential denial (`NotAuthenticated` via `permission_denied`
/// when every authenticator returns `None`): ALWAYS 401, on every view. The
/// project's `auth_exception_handler` forces `response.status_code = 401`
/// for `NotAuthenticated` (`authentication/adapter/exception.py:22-24`),
/// undoing `handle_exception`'s 403 coercion — so unlike
/// [`auth_failure_response`], the status never depends on the view's first
/// authenticator. Only the `WWW-Authenticate` challenge is conditional
/// (present iff the first authenticator supplies one). F4 pins this
/// (`drf_failure_shapes.NotAuthenticated`: "custom handler forces 401").
pub fn not_authenticated_response(first_authenticate_header: Option<&str>) -> Response {
    let body = serde_json::json!({ "detail": CODE_CREDENTIALS_NOT_PROVIDED }).to_string();
    let mut builder = Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(challenge) = first_authenticate_header {
        builder = builder.header(header::WWW_AUTHENTICATE, challenge);
    }
    builder
        .body(axum::body::Body::from(body))
        .expect("auth denial builds")
}

/// Unhandled-failure escape (`UnicodeDecodeError` on the header,
/// `ValidationError` on a non-UUID id, any `sqlx::Error` outside the
/// best-effort bump): 500. Django renders its HTML error page here; only
/// the status is contract-pinned, and the body keeps the sibling-handler
/// JSON text (same position as `crate::runner_runs::SERVER_ERROR_BODY`).
pub fn server_error() -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(
            r#"{"error":"Something went wrong please try again later"}"#,
        ))
        .expect("server error builds")
}

// ---------------------------------------------------------------------------
// `Authorization` parsing (`_bearer`, `authentication.py:46-53`)
// ---------------------------------------------------------------------------

/// What `_bearer` found: no header/empty/malformed means "no credential"
/// (`Ok(None)`, exactly like Python returning `None`); undecodable bytes in
/// an otherwise two-part header are the source's `UnicodeDecodeError` (500).
#[allow(clippy::result_large_err)]
pub fn bearer_token(header_value: Option<&[u8]>) -> Result<Option<String>, Response> {
    let bytes = match header_value {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => return Ok(None),
    };
    let parts: Vec<&[u8]> = bytes
        .split(|b| b.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    // `len(parts) != 2` short-circuits before any decode (`:51`).
    if parts.len() != 2 {
        return Ok(None);
    }
    let scheme = std::str::from_utf8(parts[0]).map_err(|_| server_error())?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Ok(None);
    }
    let token = std::str::from_utf8(parts[1]).map_err(|_| server_error())?;
    Ok(Some(token.to_owned()))
}

/// `Authorization` header bytes from the request.
fn authorization_bytes(headers: &HeaderMap) -> Option<&[u8]> {
    headers.get(header::AUTHORIZATION).map(|v| v.as_bytes())
}

/// Runner identity for the `mt_` path (`_request_runner_id`,
/// `authentication.py:174-180`): the URL `runner_id` wins, else the
/// `X-Runner-Id` header stripped, blank meaning absent. Both inputs are
/// already decoded; callers holding raw headers use [`request_runner_id`],
/// which also keeps the source's laziness (URL wins without decoding).
pub fn select_runner_id(url_runner_id: Option<&str>, x_runner_id: Option<&str>) -> Option<String> {
    if let Some(url) = url_runner_id {
        return Some(url.to_owned());
    }
    let header = x_runner_id?;
    let trimmed = header.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Raw `X-Runner-Id` header value: missing means absent; present bytes must
/// decode (anything undecodable fails UUID prep in Python, hence 500).
#[allow(clippy::result_large_err)]
fn x_runner_id(headers: &HeaderMap) -> Result<Option<String>, Response> {
    match headers.get("x-runner-id") {
        None => Ok(None),
        Some(value) => std::str::from_utf8(value.as_bytes())
            .map(|raw| Some(raw.to_owned()))
            .map_err(|_| server_error()),
    }
}

/// Runner identity for the `mt_` branch with the source's laziness
/// (`_request_runner_id`, `authentication.py:174-180`): the URL id wins
/// WITHOUT reading the header at all, so non-UTF-8 `X-Runner-Id` bytes on
/// a URL-scoped route are ignored rather than a 500. Only when the route
/// carries no id is the header decoded and combined via
/// [`select_runner_id`].
#[allow(clippy::result_large_err)]
fn request_runner_id(
    url_runner_id: Option<&str>,
    headers: &HeaderMap,
) -> Result<Option<String>, Response> {
    if url_runner_id.is_some() {
        return Ok(select_runner_id(url_runner_id, None));
    }
    let header = x_runner_id(headers)?;
    Ok(select_runner_id(None, header.as_deref()))
}

// ---------------------------------------------------------------------------
// Row structs (the `select_related` loads, as data)
// ---------------------------------------------------------------------------

/// One `runner` row plus its dev-machine revocation bit: the
/// `Runner.objects.select_related("workspace", "pod", "dev_machine").get(...)`
/// loads (`authentication.py:92,138`). The related rows are never read past
/// their revocation bits on the auth paths, so only the bit is joined.
/// Column order follows F1 (`models/columns.json`, `runner`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RunnerAuthRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub workspace_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub pod_id: Uuid,
    pub name: String,
    pub host_label: String,
    pub provisioning: String,
    pub visibility: i16,
    pub refresh_token_hash: String,
    pub refresh_token_fingerprint: String,
    pub refresh_token_generation: i32,
    pub previous_refresh_token_hash: String,
    pub access_token_signing_key_version: i32,
    pub enrollment_token_hash: String,
    pub enrollment_token_fingerprint: String,
    pub enrolled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub capabilities: Value,
    pub status: String,
    pub os: String,
    pub arch: String,
    pub runner_version: String,
    pub dev_metadata: Value,
    pub protocol_version: i32,
    pub last_heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    pub free_worktrees: Option<i32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_reason: String,
    /// `dm.revoked_at IS NOT NULL` — the joined
    /// `runner.dev_machine.revoked_at` read (`authentication.py:101,143`).
    pub dev_machine_revoked: bool,
}

/// One `machine_token` row plus the joined reads the auth paths and the
/// create endpoint need: the `MachineToken.objects.select_related("user",
/// "workspace", "dev_machine").get(token_hash=...)` load
/// (`authentication.py:123,220`, `api_authentication.py:50`) and the
/// `auth_machine_token.workspace.slug` read (`enrollment.py:598,603`).
/// Column order follows F1 (`models/columns.json`, `machine_token`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MachineTokenAuthRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub dev_machine_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub host_label: String,
    pub token_hash: String,
    pub token_fingerprint: String,
    pub label: String,
    pub is_service: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `workspaces.slug` (`SlugField`, never null; `None` only if the
    /// workspace row itself is gone, which the FK forbids).
    pub workspace_slug: Option<String>,
    /// `dm.revoked_at IS NOT NULL` — the joined
    /// `token.dev_machine.revoked_at` read (`authentication.py:128,225`,
    /// `api_authentication.py:57`).
    pub dev_machine_revoked: bool,
}

/// `request.auth_token_payload` for the `mt_` branch
/// (`authentication.py:156-161`): `{sub, uid, wid, machine_token}`, all
/// UUID strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineTokenPayload {
    pub sub: Uuid,
    pub uid: Uuid,
    pub wid: Uuid,
    pub machine_token: Uuid,
}

/// `request.auth_token_payload` as data: the `mt_` dict above, or the
/// decoded JWT claims verbatim (`authentication.py:117`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthTokenPayload {
    Machine(MachineTokenPayload),
    Jwt(pidash_auth::jwt::AccessClaims),
}

/// A verified `RunnerAccessTokenAuthentication` credential:
/// `request.auth_runner` + `request.auth_machine_token` (mt_ branch only) +
/// `request.auth_token_payload`, with the returned user id
/// (`(token.user, None)` / `(runner.owner, None)`).
#[derive(Debug, Clone)]
pub struct AccessAuth {
    pub user_id: Uuid,
    pub runner: RunnerAuthRow,
    pub machine_token: Option<MachineTokenAuthRow>,
    pub payload: AuthTokenPayload,
}

/// A verified `MachineTokenAuthentication` credential:
/// `request.auth_machine_token` plus the returned `(token.user, None)`
/// (`authentication.py:236-237`).
#[derive(Debug, Clone)]
pub struct MachineAuth {
    pub user_id: Uuid,
    pub token: MachineTokenAuthRow,
}

/// A verified `APIKeyAuthentication` credential: the returned user plus
/// `request.auth` — the raw presented string on the `mt_` branch, the
/// stored `api_token.token` on the `APIToken` branch
/// (`api_authentication.py:45,64`; the create endpoint passes it to
/// `deactivate_api_token`) — and `request.auth_machine_token` (mt_ branch
/// only).
#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    pub user_id: Uuid,
    pub auth_token: String,
    pub machine_token: Option<MachineTokenAuthRow>,
}

/// `APIKeyAuthentication.authenticate` outcome as data: `None` (no header)
/// stays anonymous for the permission layer; `Invalid` is the single opaque
/// `Given API token is not valid` denial. Rendering needs the consuming
/// view's first `authenticate_header` (403 on create, 401 + `Bearer` on
/// projects — see the module docs), so handlers map through
/// [`auth_failure_response`].
#[derive(Debug, Clone)]
pub enum ApiKeyOutcome {
    Authenticated(Box<ApiKeyAuth>),
    Missing,
    Invalid,
}

// ---------------------------------------------------------------------------
// Pure decision predicates (branch order stays at the call sites)
// ---------------------------------------------------------------------------

/// JWT freshness (`authentication.py:105`): `int(payload.rtg or 0) <
/// runner.refresh_token_generation - 1` denies — note the `-1` grace
/// window. The kernel guarantees `rtg` is present (`decode_access_token`
/// requires it), so the `or 0` is already applied.
pub fn rtg_fresh(rtg: i64, refresh_token_generation: i32) -> bool {
    rtg >= i64::from(refresh_token_generation) - 1
}

/// Force-refresh floor (`authentication.py:109`): no row passes; a row
/// denies when `rtg < min_rtg`.
pub fn force_refresh_ok(rtg: i64, min_rtg: Option<i32>) -> bool {
    match min_rtg {
        None => true,
        Some(floor) => rtg >= i64::from(floor),
    }
}

/// URL-runner match (`authentication.py:113`): plain string inequality of
/// `str(url_runner_id)` vs `str(runner.id)` — no UUID parsing (the `<uuid:>`
/// converter guarantees a canonical UUID or no match at all upstream).
/// Callers pass the canonical path segment.
pub fn url_runner_matches(url_runner_id: &str, runner_id: Uuid) -> bool {
    url_runner_id == runner_id.to_string()
}

/// Machine-token runner binding (`authentication.py:145-151`): owner and
/// workspace must match; then a machine-bound token requires the same
/// machine, while a legacy unbound (host-label) token requires no machine
/// and an equal `host_label`. Any violation is
/// `runner_not_bound_to_machine_token`. The eight parameters mirror the
/// Python condition inputs one by one.
#[allow(clippy::too_many_arguments)]
pub fn runner_bound_to_token(
    owner_id: Uuid,
    token_user_id: Uuid,
    runner_workspace_id: Uuid,
    token_workspace_id: Uuid,
    token_dev_machine_id: Option<Uuid>,
    token_host_label: &str,
    runner_dev_machine_id: Option<Uuid>,
    runner_host_label: &str,
) -> bool {
    if owner_id != token_user_id || runner_workspace_id != token_workspace_id {
        return false;
    }
    match token_dev_machine_id {
        Some(token_machine) => runner_dev_machine_id == Some(token_machine),
        None => runner_dev_machine_id.is_none() && runner_host_label == token_host_label,
    }
}

// ---------------------------------------------------------------------------
// Own SQL (one fetch per `select_related` load)
// ---------------------------------------------------------------------------

/// `MachineToken.objects.select_related("user", "workspace",
/// "dev_machine").get(token_hash=...)`: hash match by lookup, one row, plus
/// the workspace slug and the dev-machine revocation bit.
async fn fetch_machine_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<MachineTokenAuthRow>, sqlx::Error> {
    sqlx::query_as::<_, MachineTokenAuthRow>(
        r#"SELECT mt."id", mt."user_id", mt."dev_machine_id", mt."workspace_id",
                  mt."host_label", mt."token_hash", mt."token_fingerprint", mt."label",
                  mt."is_service", mt."created_at", mt."last_used_at", mt."revoked_at",
                  w."slug" AS "workspace_slug",
                  dm."revoked_at" IS NOT NULL AS "dev_machine_revoked"
           FROM "machine_token" mt
           LEFT OUTER JOIN "workspaces" w ON w."id" = mt."workspace_id"
           LEFT OUTER JOIN "dev_machine" dm ON dm."id" = mt."dev_machine_id"
           WHERE mt."token_hash" = $1"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

/// `Runner.objects.select_related("workspace", "pod",
/// "dev_machine").get(id=...)`: full row plus the dev-machine revocation
/// bit. No soft-delete predicate: `Runner` is a plain `models.Model`.
async fn fetch_runner_for_auth(
    pool: &PgPool,
    runner_id: Uuid,
) -> Result<Option<RunnerAuthRow>, sqlx::Error> {
    sqlx::query_as::<_, RunnerAuthRow>(
        r#"SELECT r."id", r."owner_id", r."workspace_id", r."dev_machine_id", r."pod_id",
                  r."name", r."host_label", r."provisioning", r."visibility",
                  r."refresh_token_hash", r."refresh_token_fingerprint",
                  r."refresh_token_generation", r."previous_refresh_token_hash",
                  r."access_token_signing_key_version",
                  r."enrollment_token_hash", r."enrollment_token_fingerprint",
                  r."enrolled_at", r."capabilities", r."status", r."os", r."arch",
                  r."runner_version", r."dev_metadata", r."protocol_version",
                  r."last_heartbeat_at", r."free_worktrees",
                  r."created_at", r."updated_at", r."revoked_at", r."revoked_reason",
                  dm."revoked_at" IS NOT NULL AS "dev_machine_revoked"
           FROM "runner" r
           LEFT OUTER JOIN "dev_machine" dm ON dm."id" = r."dev_machine_id"
           WHERE r."id" = $1"#,
    )
    .bind(runner_id)
    .fetch_optional(pool)
    .await
}

/// `is_workspace_member(user, workspace_id)` (`core/permissions.py:28-34`):
/// an active, non-soft-deleted `WorkspaceMember` row (the default manager
/// filters `deleted_at IS NULL`). Fetches the role so the membership kernel
/// decides, exactly like the Python `.exists()` (row presence).
async fn workspace_member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<i32>, sqlx::Error> {
    // `role` is `smallint`: decode as `i16` (sqlx does not widen), then
    // widen for the kernel-shaped return.
    let role: Option<i16> = sqlx::query_scalar(
        r#"SELECT "role" FROM "workspace_members"
           WHERE "workspace_id" = $1 AND "member_id" = $2
             AND "is_active" AND "deleted_at" IS NULL
           LIMIT 1"#,
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(role.map(i32::from))
}

/// `MachineToken.revoke()` (`models.py:865-870`): stamp `revoked_at`
/// (`update_fields=["revoked_at"]`). Callers only reach this for unrevoked
/// rows (revoked is denied earlier), so the `is not None` early return never
/// fires here.
async fn revoke_machine_token(pool: &PgPool, token_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE "machine_token" SET "revoked_at" = $1 WHERE "machine_token"."id" = $2"#)
        .bind(chrono::Utc::now())
        .bind(token_id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// `MachineToken.objects.filter(pk=...).update(last_used_at=...)`: no
/// `updated_at` touch (`QuerySet.update` writes only the named column).
async fn bump_machine_token_last_used(pool: &PgPool, token_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(r#"UPDATE "machine_token" SET "last_used_at" = $1 WHERE "machine_token"."id" = $2"#)
        .bind(chrono::Utc::now())
        .bind(token_id)
        .execute(pool)
        .await
        .map(|_| ())
}

/// Membership-or-revoke shared by the three `mt_` paths
/// (`authentication.py:130-132,227-229`, `api_authentication.py:59-61`):
/// `Ok(true)` for members; non-members get `revoke()` first, then
/// `Ok(false)` and the caller denies (`membership_revoked`, or the opaque
/// API-key message).
#[allow(clippy::result_large_err)]
async fn member_or_revoke(pool: &PgPool, token: &MachineTokenAuthRow) -> Result<bool, Response> {
    let role = workspace_member_role(pool, token.workspace_id, token.user_id)
        .await
        .map_err(|_| server_error())?;
    // `is_workspace_member`: an active membership row exists — the exact
    // `membership.rs` kernel body, inlined (see the module docs).
    if role.is_some() {
        return Ok(true);
    }
    revoke_machine_token(pool, token.id)
        .await
        .map_err(|_| server_error())?;
    Ok(false)
}

// ---------------------------------------------------------------------------
// `RunnerAccessTokenAuthentication` (`authentication.py:56-180`)
// ---------------------------------------------------------------------------

/// `RunnerAccessTokenAuthentication.authenticate` as data
/// (`authentication.py:79-118`): `Ok(None)` when no Bearer [REDACTED] is
/// presented (the view's permission layer decides); `Err(response)` carries
/// the 401 + `WWW-Authenticate: Bearer` denial — every consuming view is
/// bearer-first — or the 500 escape.
///
/// `url_runner_id` is the canonical `<uuid:runner_id>` path segment when the
/// route has one (`None` on unscoped routes): it wins over `X-Runner-Id` on
/// the `mt_` branch and gates the JWT match. `ring` verifies legacy JWTs
/// (handlers build it from settings; stock settings carry no
/// `RUNNER_ACCESS_TOKEN_KEYS`, i.e. the derived `"default"` key).
#[allow(clippy::result_large_err)]
pub async fn authenticate_access_token(
    pool: &PgPool,
    secret_key: &[u8],
    ring: &KeyRing,
    headers: &HeaderMap,
    url_runner_id: Option<&str>,
) -> Result<Option<AccessAuth>, Response> {
    let raw = bearer_token(authorization_bytes(headers))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.starts_with(pidash_auth::token::MACHINE_TOKEN_PREFIX) {
        return authenticate_access_machine_token(pool, secret_key, headers, &raw, url_runner_id)
            .await;
    }
    authenticate_access_jwt(pool, ring, &raw, url_runner_id).await
}

/// Legacy per-runner JWT path (`authentication.py:85-118`), in order:
/// kernel decode (codes re-raised verbatim), runner load, live revocation
/// checks, rtg window, force-refresh floor, URL-runner match.
#[allow(clippy::result_large_err)]
async fn authenticate_access_jwt(
    pool: &PgPool,
    ring: &KeyRing,
    raw: &str,
    url_runner_id: Option<&str>,
) -> Result<Option<AccessAuth>, Response> {
    let payload = match pidash_auth::jwt::decode_access_token(raw, ring) {
        Ok(claims) => claims,
        Err(error) => return Err(bearer_failure_response(error.code())),
    };
    // `Runner.objects...get(id=sub)` (`:91-94`): a non-UUID `sub` is the
    // source's `ValidationError` (500); a missing row is `runner_not_found`.
    let runner_id: Uuid = payload.sub.parse().map_err(|_| server_error())?;
    let row = fetch_runner_for_auth(pool, runner_id)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(bearer_failure_response(CODE_RUNNER_NOT_FOUND));
    };
    if row.revoked_at.is_some() {
        return Err(bearer_failure_response(CODE_RUNNER_REVOKED));
    }
    if row.dev_machine_id.is_some() && row.dev_machine_revoked {
        return Err(bearer_failure_response(CODE_DEV_MACHINE_REVOKED));
    }
    if !rtg_fresh(payload.rtg, row.refresh_token_generation) {
        return Err(bearer_failure_response(CODE_ACCESS_TOKEN_STALE_RTG));
    }
    // `RunnerForceRefresh.objects.filter(runner=runner).first()` (`:108`):
    // PK lookup, at most one row, so no ordering/limit is needed.
    let min_rtg: Option<i32> = sqlx::query_scalar(
        r#"SELECT "min_rtg" FROM "runner_force_refresh" WHERE "runner_id" = $1"#,
    )
    .bind(row.id)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    if !force_refresh_ok(payload.rtg, min_rtg) {
        return Err(bearer_failure_response(CODE_FORCE_REFRESH_REQUIRED));
    }
    if let Some(url) = url_runner_id {
        if !url_runner_matches(url, row.id) {
            return Err(bearer_failure_response(CODE_RUNNER_ID_MISMATCH));
        }
    }
    Ok(Some(AccessAuth {
        user_id: row.owner_id,
        runner: row,
        machine_token: None,
        payload: AuthTokenPayload::Jwt(payload),
    }))
}

/// Shared MachineToken path (`authentication.py:120-162`), in order: hash →
/// row → revoked → machine-revoked → membership-or-revoke → runner-id
/// required → runner load → runner/machine revoked → binding → `last_used_at`
/// bump (plain update, NOT best-effort — a DB error escapes to 500) →
/// payload.
#[allow(clippy::result_large_err)]
async fn authenticate_access_machine_token(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
    raw: &str,
    url_runner_id: Option<&str>,
) -> Result<Option<AccessAuth>, Response> {
    let token_hash = pidash_auth::token::hash_token(raw, secret_key);
    let token = fetch_machine_token(pool, &token_hash)
        .await
        .map_err(|_| server_error())?;
    let Some(token) = token else {
        return Err(bearer_failure_response(CODE_MACHINE_TOKEN_INVALID));
    };
    if token.revoked_at.is_some() {
        return Err(bearer_failure_response(CODE_MACHINE_TOKEN_REVOKED));
    }
    if token.dev_machine_id.is_some() && token.dev_machine_revoked {
        return Err(bearer_failure_response(CODE_DEV_MACHINE_REVOKED));
    }
    if !member_or_revoke(pool, &token).await? {
        return Err(bearer_failure_response(CODE_MEMBERSHIP_REVOKED));
    }
    let runner_raw = request_runner_id(url_runner_id, headers)?;
    let Some(runner_raw) = runner_raw else {
        return Err(bearer_failure_response(CODE_RUNNER_ID_REQUIRED));
    };
    // `Runner.objects...get(id=runner_id)` (`:137-140`): a non-UUID id
    // (only reachable via `X-Runner-Id`; the URL converter 404s first) is
    // the source's `ValidationError` (500).
    let runner_id: Uuid = runner_raw.parse().map_err(|_| server_error())?;
    let row = fetch_runner_for_auth(pool, runner_id)
        .await
        .map_err(|_| server_error())?;
    let Some(row) = row else {
        return Err(bearer_failure_response(CODE_RUNNER_NOT_FOUND));
    };
    if row.revoked_at.is_some() {
        return Err(bearer_failure_response(CODE_RUNNER_REVOKED));
    }
    if row.dev_machine_id.is_some() && row.dev_machine_revoked {
        return Err(bearer_failure_response(CODE_DEV_MACHINE_REVOKED));
    }
    if !runner_bound_to_token(
        row.owner_id,
        token.user_id,
        row.workspace_id,
        token.workspace_id,
        token.dev_machine_id,
        &token.host_label,
        row.dev_machine_id,
        &row.host_label,
    ) {
        return Err(bearer_failure_response(
            CODE_RUNNER_NOT_BOUND_TO_MACHINE_TOKEN,
        ));
    }
    bump_machine_token_last_used(pool, token.id)
        .await
        .map_err(|_| server_error())?;
    let payload = MachineTokenPayload {
        sub: row.id,
        uid: token.user_id,
        wid: token.workspace_id,
        machine_token: token.id,
    };
    Ok(Some(AccessAuth {
        user_id: token.user_id,
        runner: row,
        machine_token: Some(token),
        payload: AuthTokenPayload::Machine(payload),
    }))
}

// ---------------------------------------------------------------------------
// `RunnerRefreshTokenAuthentication` (`authentication.py:183-202`)
// ---------------------------------------------------------------------------

/// Parse the Bearer [REDACTED] into `request.auth_refresh_token` as data:
/// missing/empty/malformed means `None` (the refresh view then answers 401
/// `missing_refresh_token` itself); otherwise the raw token with NO user and
/// NO validation (`(None, None)` — the row-locked algorithm lives in the
/// view). Undecodable two-part bytes are the source's `UnicodeDecodeError`
/// (500), exactly like [`bearer_token`].
#[allow(clippy::result_large_err)]
pub fn parse_refresh_token(headers: &HeaderMap) -> Result<Option<String>, Response> {
    bearer_token(authorization_bytes(headers))
}

// ---------------------------------------------------------------------------
// `MachineTokenAuthentication` (`authentication.py:205-240`)
// ---------------------------------------------------------------------------

/// `MachineTokenAuthentication.authenticate` as data: `Ok(None)` when no
/// Bearer [REDACTED] is presented or it is not an `mt_` token (falls through
/// to the next class or the permission layer); `Err(response)` carries the
/// 401 + `WWW-Authenticate: Bearer` denial or the 500 escape.
///
/// The `last_used_at` bump is best-effort (`:230-235`): failures are
/// swallowed with a debug log and the request continues — unlike the plain
/// update on the access-token `mt_` branch.
#[allow(clippy::result_large_err)]
pub async fn authenticate_machine_token(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<Option<MachineAuth>, Response> {
    let raw = bearer_token(authorization_bytes(headers))?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    if !raw.starts_with(pidash_auth::token::MACHINE_TOKEN_PREFIX) {
        return Ok(None);
    }
    let token_hash = pidash_auth::token::hash_token(&raw, secret_key);
    let token = fetch_machine_token(pool, &token_hash)
        .await
        .map_err(|_| server_error())?;
    let Some(token) = token else {
        return Err(bearer_failure_response(CODE_MACHINE_TOKEN_INVALID));
    };
    if token.revoked_at.is_some() {
        return Err(bearer_failure_response(CODE_MACHINE_TOKEN_REVOKED));
    }
    if token.dev_machine_id.is_some() && token.dev_machine_revoked {
        return Err(bearer_failure_response(CODE_DEV_MACHINE_REVOKED));
    }
    if !member_or_revoke(pool, &token).await? {
        return Err(bearer_failure_response(CODE_MEMBERSHIP_REVOKED));
    }
    if let Err(error) = bump_machine_token_last_used(pool, token.id).await {
        tracing::debug!(%error, "runner_enroll.auth: last_used_at bump failed; continuing");
    }
    Ok(Some(MachineAuth {
        user_id: token.user_id,
        token,
    }))
}

// ---------------------------------------------------------------------------
// `APIKeyAuthentication` (`api/middleware/api_authentication.py:20-76`)
// ---------------------------------------------------------------------------

/// `X-Api-Key` header value as data: missing/empty means anonymous
/// (`authenticate` returns `None`, `:68-69`). Decoding is lossy on purpose:
/// Python works on the latin-1 header string and any non-matching value —
/// including undecodable bytes — falls out of the token lookups into the
/// opaque `Invalid`, never a 500.
fn api_key_value(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(pidash_auth::token::API_KEY_HEADER)?;
    let raw = String::from_utf8_lossy(value.as_bytes()).into_owned();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

/// `APIKeyAuthentication.authenticate` as data (`:66-76`): routes on the
/// `mt_` prefix (kernel [`pidash_auth::token::classify_token`]) into the
/// machine branch or the `APIToken` branch. Every rejection collapses to
/// [`ApiKeyOutcome::Invalid`] — the single opaque `Given API token is not
/// valid` denial, deliberately indistinguishable across unknown token,
/// revoked token/machine, non-member (revoked first), and expired/inactive
/// `APIToken`. Handlers render it through [`auth_failure_response`] with
/// their view's first `authenticate_header` (403 on create, 401 + `Bearer`
/// on projects).
#[allow(clippy::result_large_err)]
pub async fn authenticate_api_key(
    pool: &PgPool,
    secret_key: &[u8],
    headers: &HeaderMap,
) -> Result<ApiKeyOutcome, Response> {
    let Some(raw) = api_key_value(headers) else {
        return Ok(ApiKeyOutcome::Missing);
    };
    match pidash_auth::token::classify_token(&raw) {
        None => Ok(ApiKeyOutcome::Missing),
        Some(pidash_auth::token::TokenKind::Machine) => {
            authenticate_api_key_machine(pool, secret_key, &raw).await
        }
        Some(pidash_auth::token::TokenKind::Api) => authenticate_api_key_token(pool, &raw).await,
    }
}

/// `validate_machine_token` (`api_authentication.py:47-64`): static kernel
/// check over the fetched row, membership-or-revoke, plain `last_used_at`
/// bump (NOT best-effort here — errors escape to 500), `request.auth` is
/// the raw presented string.
#[allow(clippy::result_large_err)]
async fn authenticate_api_key_machine(
    pool: &PgPool,
    secret_key: &[u8],
    raw: &str,
) -> Result<ApiKeyOutcome, Response> {
    let presented_hash = pidash_auth::token::hash_token(raw, secret_key);
    let token = fetch_machine_token(pool, &presented_hash)
        .await
        .map_err(|_| server_error())?;
    let Some(token) = token else {
        return Ok(ApiKeyOutcome::Invalid);
    };
    let static_row = pidash_auth::token::MachineTokenRow {
        token_hash: token.token_hash.clone(),
        revoked_at_unix: token.revoked_at.map(|dt| dt.timestamp()),
        dev_machine_revoked: token.dev_machine_id.is_some() && token.dev_machine_revoked,
    };
    // Hash match is implied by the lookup; the kernel checks the
    // revocation arms. Every arm maps to the one opaque message.
    if pidash_auth::token::validate_machine_token_static(Some(&static_row), &presented_hash)
        .is_err()
    {
        return Ok(ApiKeyOutcome::Invalid);
    }
    if !member_or_revoke(pool, &token).await? {
        return Ok(ApiKeyOutcome::Invalid);
    }
    bump_machine_token_last_used(pool, token.id)
        .await
        .map_err(|_| server_error())?;
    Ok(ApiKeyOutcome::Authenticated(Box::new(ApiKeyAuth {
        user_id: token.user_id,
        auth_token: raw.to_owned(),
        machine_token: Some(token),
    })))
}

/// Decoded `api_tokens` row for [`authenticate_api_key_token`].
type ApiTokenLookup = (
    Uuid,
    String,
    bool,
    Option<chrono::DateTime<chrono::Utc>>,
    Uuid,
);

/// `validate_api_token` (`api_authentication.py:32-45`): exact token match
/// plus `is_active` plus strictly-future-or-null `expired_at`, over the
/// manager's live rows (`deleted_at IS NULL`, `db/mixins.py:49-58`).
/// `request.auth` is the stored `api_token.token`, and `last_used` is
/// stamped (`save(update_fields=["last_used"])`).
#[allow(clippy::result_large_err)]
async fn authenticate_api_key_token(
    pool: &PgPool,
    presented: &str,
) -> Result<ApiKeyOutcome, Response> {
    let row: Option<ApiTokenLookup> = sqlx::query_as(
        r#"SELECT "id", "token", "is_active", "expired_at", "user_id"
               FROM "api_tokens" WHERE "token" = $1 AND "deleted_at" IS NULL"#,
    )
    .bind(presented)
    .fetch_optional(pool)
    .await
    .map_err(|_| server_error())?;
    let Some((id, token, is_active, expired_at, user_id)) = row else {
        return Ok(ApiKeyOutcome::Invalid);
    };
    let kernel_row = pidash_auth::token::ApiTokenRow {
        token: token.clone(),
        is_active,
        expired_at_unix: expired_at.map(|dt| dt.timestamp()),
    };
    if pidash_auth::token::validate_api_token(
        Some(&kernel_row),
        presented,
        chrono::Utc::now().timestamp(),
    )
    .is_err()
    {
        return Ok(ApiKeyOutcome::Invalid);
    }
    sqlx::query(r#"UPDATE "api_tokens" SET "last_used" = $1 WHERE "api_tokens"."id" = $2"#)
        .bind(chrono::Utc::now())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|_| server_error())?;
    Ok(ApiKeyOutcome::Authenticated(Box::new(ApiKeyAuth {
        user_id,
        auth_token: token,
        machine_token: None,
    })))
}

// ---------------------------------------------------------------------------
// `resolve_runner_for_run` (`authentication.py:243-254`)
// ---------------------------------------------------------------------------

/// Per-run authorization (`design.md` §7.5): the run is owned by the
/// authenticated runner. No credential or a runnerless run is `False`.
/// Python normalizes through `UUID(str(...))`; typed `Uuid` inputs make
/// that normalization a no-op, so this is a plain comparison.
pub fn resolve_runner_for_run(run_runner_id: Option<Uuid>, auth_runner_id: Option<Uuid>) -> bool {
    match (run_runner_id, auth_runner_id) {
        (Some(run), Some(auth)) => run == auth,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    fn bearer_headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(value).expect("header"),
        );
        headers
    }

    /// `_bearer` matrix (`authentication.py:46-53`): missing/empty/malformed
    /// is anonymous; the scheme match is case-insensitive; surrounding and
    /// inner whitespace collapses like Python's `split()`.
    #[test]
    fn bearer_extraction_matrix() {
        assert_eq!(bearer_token(None).unwrap(), None);
        assert_eq!(bearer_token(Some(b"")).unwrap(), None);
        assert_eq!(bearer_token(Some(b"   ")).unwrap(), None);
        assert_eq!(
            bearer_token(Some(b"Bearer abc123")).unwrap(),
            Some("abc123".to_owned())
        );
        assert_eq!(
            bearer_token(Some(b"bearer abc123")).unwrap(),
            Some("abc123".to_owned())
        );
        assert_eq!(
            bearer_token(Some(b"BEARER abc123")).unwrap(),
            Some("abc123".to_owned())
        );
        assert_eq!(
            bearer_token(Some(b"  Bearer   abc123  ")).unwrap(),
            Some("abc123".to_owned())
        );
        assert_eq!(bearer_token(Some(b"Bearer")).unwrap(), None);
        assert_eq!(bearer_token(Some(b"Bearer a b")).unwrap(), None);
        assert_eq!(bearer_token(Some(b"Basic abc123")).unwrap(), None);
        assert_eq!(bearer_token(Some(b"Token abc123")).unwrap(), None);
    }

    /// Decode order mirrors `_bearer`'s short-circuit: a non-two-part
    /// non-UTF-8 header returns `None` without decoding (`len(parts) != 2`
    /// fires first), while undecodable bytes in a two-part header are the
    /// 500 (`UnicodeDecodeError`).
    #[test]
    fn bearer_decode_short_circuits_on_part_count() {
        // Three parts, non-UTF-8 present: anonymous, not a 500.
        assert_eq!(bearer_token(Some(b"\xff Bearer x")).unwrap(), None);
        assert_eq!(bearer_token(Some(b"Bearer \xff extra")).unwrap(), None);
        // Two parts, undecodable scheme or token: 500.
        assert!(bearer_token(Some(b"\xff abc")).is_err());
        assert!(bearer_token(Some(b"Bearer \xff")).is_err());
    }

    /// `_request_runner_id` (`authentication.py:174-180`): the URL id wins
    /// even when the header is present; otherwise the stripped header;
    /// blank header means absent.
    #[test]
    fn runner_id_selection_prefers_url() {
        assert_eq!(
            select_runner_id(Some("url-id"), Some("hdr-id")),
            Some("url-id".to_owned())
        );
        assert_eq!(
            select_runner_id(None, Some("hdr-id")),
            Some("hdr-id".to_owned())
        );
        assert_eq!(
            select_runner_id(None, Some("  hdr-id  ")),
            Some("hdr-id".to_owned())
        );
        assert_eq!(select_runner_id(None, Some("   ")), None);
        assert_eq!(select_runner_id(None, Some("")), None);
        assert_eq!(select_runner_id(None, None), None);
        assert_eq!(
            select_runner_id(Some("url-id"), None),
            Some("url-id".to_owned())
        );
    }

    /// The URL runner id wins WITHOUT reading `X-Runner-Id`
    /// (`_request_runner_id`, `authentication.py:174-180`): non-UTF-8
    /// header bytes on a URL-scoped route are ignored, not a 500. Only a
    /// header-only request decodes the header (undecodable bytes are the
    /// source's `ValidationError`, i.e. 500).
    #[test]
    fn url_runner_id_skips_header_decode() {
        let mut poisoned = HeaderMap::new();
        poisoned.insert(
            "x-runner-id",
            header::HeaderValue::from_bytes(b"\xff\xfe").expect("obs-text header"),
        );
        // URL present: the header is never touched.
        assert_eq!(
            request_runner_id(Some("url-id"), &poisoned).unwrap(),
            Some("url-id".to_owned())
        );
        // Header-only: undecodable bytes are a 500.
        assert!(request_runner_id(None, &poisoned).is_err());
        // Header-only, well-formed: the stripped value.
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-runner-id",
            header::HeaderValue::from_static("  hdr-id  "),
        );
        assert_eq!(
            request_runner_id(None, &headers).unwrap(),
            Some("hdr-id".to_owned())
        );
    }

    /// URL-runner match is the plain string inequality
    /// (`authentication.py:113`): canonical equal matches, anything else
    /// (including a differently-cased but equal UUID) mismatches.
    #[test]
    fn url_runner_match_is_string_compare() {
        let id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert!(url_runner_matches(
            "11111111-2222-3333-4444-555555555555",
            id
        ));
        assert!(!url_runner_matches(
            "11111111-2222-3333-4444-555555555556",
            id
        ));
        assert!(!url_runner_matches(
            "11111111-2222-3333-4444-555555555555 ",
            id
        ));
        assert!(!url_runner_matches(
            "11111111-2222-3333-4444-55555555555",
            id
        ));
        assert!(!url_runner_matches("not-a-uuid", id));
    }

    /// `access_token_stale_rtg` window (`authentication.py:105`): deny iff
    /// `rtg < generation - 1` — the `-1` grace is load-bearing.
    #[test]
    fn rtg_window_has_minus_one_grace() {
        assert!(rtg_fresh(7, 7));
        assert!(rtg_fresh(6, 7));
        assert!(!rtg_fresh(5, 7));
        assert!(rtg_fresh(0, 0));
        assert!(rtg_fresh(0, 1));
        assert!(!rtg_fresh(0, 2));
    }

    /// Force-refresh floor (`authentication.py:109`): no row passes; a row
    /// denies only below `min_rtg`.
    #[test]
    fn force_refresh_floor() {
        assert!(force_refresh_ok(5, None));
        assert!(force_refresh_ok(5, Some(5)));
        assert!(force_refresh_ok(6, Some(5)));
        assert!(!force_refresh_ok(4, Some(5)));
    }

    /// Binding matrix (`authentication.py:145-151`): owner + workspace must
    /// match; machine-bound tokens pin the machine; unbound tokens pin
    /// machine-less + equal `host_label`.
    #[test]
    fn binding_matrix() {
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        let ws = Uuid::new_v4();
        let ws2 = Uuid::new_v4();
        let m1 = Uuid::new_v4();
        let m2 = Uuid::new_v4();
        // Bound token, same machine.
        assert!(runner_bound_to_token(
            owner,
            owner,
            ws,
            ws,
            Some(m1),
            "h",
            Some(m1),
            "other-host"
        ));
        // Bound token, different machine / runner machineless.
        assert!(!runner_bound_to_token(
            owner,
            owner,
            ws,
            ws,
            Some(m1),
            "h",
            Some(m2),
            "h"
        ));
        assert!(!runner_bound_to_token(
            owner,
            owner,
            ws,
            ws,
            Some(m1),
            "h",
            None,
            "h"
        ));
        // Unbound token: machineless runner + equal host label.
        assert!(runner_bound_to_token(
            owner, owner, ws, ws, None, "h", None, "h"
        ));
        assert!(!runner_bound_to_token(
            owner, owner, ws, ws, None, "h", None, "other"
        ));
        assert!(!runner_bound_to_token(
            owner,
            owner,
            ws,
            ws,
            None,
            "h",
            Some(m1),
            "h"
        ));
        // Owner / workspace mismatch always fails.
        assert!(!runner_bound_to_token(
            other,
            owner,
            ws,
            ws,
            Some(m1),
            "h",
            Some(m1),
            "h"
        ));
        assert!(!runner_bound_to_token(
            owner,
            owner,
            ws,
            ws2,
            Some(m1),
            "h",
            Some(m1),
            "h"
        ));
        assert!(!runner_bound_to_token(
            owner, owner, ws, ws2, None, "h", None, "h"
        ));
    }

    /// `RunnerRefreshTokenAuthentication` (`authentication.py:192-199`):
    /// missing means `None`; anything well-formed passes through raw with no
    /// validation (even non-`mt_` strings — the view decides).
    #[test]
    fn refresh_parse_passes_raw_through() {
        assert_eq!(parse_refresh_token(&HeaderMap::new()).unwrap(), None);
        let headers = bearer_headers("Bearer rt_sometoken");
        assert_eq!(
            parse_refresh_token(&headers).unwrap(),
            Some("rt_sometoken".to_owned())
        );
        let headers = bearer_headers("Bearer anything-at-all");
        assert_eq!(
            parse_refresh_token(&headers).unwrap(),
            Some("anything-at-all".to_owned())
        );
        let headers = bearer_headers("Basic abc");
        assert_eq!(parse_refresh_token(&headers).unwrap(), None);
    }

    async fn response_parts(response: Response) -> (StatusCode, HeaderMap, String) {
        let (mut parts, body) = response.into_parts();
        let headers = std::mem::take(&mut parts.headers);
        let bytes = to_bytes(body, usize::MAX).await.expect("body");
        (
            parts.status,
            headers,
            String::from_utf8(bytes.to_vec()).expect("utf8"),
        )
    }

    /// Denial bytes (`views.py:71-101`, F4 `drf_failure_shapes`): lowercase-`d`
    /// `{"detail": code}`, compact JSON, `application/json`; 401 plus the
    /// challenge when the view's first authenticator supplies one, else the
    /// 403 coercion with no challenge.
    #[tokio::test]
    async fn denial_status_follows_first_authenticate_header() {
        let (status, headers, body) =
            response_parts(bearer_failure_response("machine_token_invalid")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers.get(header::WWW_AUTHENTICATE).unwrap(), "Bearer");
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(body, r#"{"detail":"machine_token_invalid"}"#);

        let (status, headers, body) =
            response_parts(auth_failure_response(CODE_GIVEN_API_TOKEN_NOT_VALID, None)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
        assert_eq!(body, r#"{"detail":"Given API token is not valid"}"#);

        // Same API-key failure on a bearer-first view (projects): 401.
        let (status, headers, body) = response_parts(auth_failure_response(
            CODE_GIVEN_API_TOKEN_NOT_VALID,
            Some("Bearer"),
        ))
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers.get(header::WWW_AUTHENTICATE).unwrap(), "Bearer");
        assert_eq!(body, r#"{"detail":"Given API token is not valid"}"#);
    }

    /// Missing-credential denial is always 401: the project's
    /// `auth_exception_handler` forces the status for `NotAuthenticated`
    /// (`authentication/adapter/exception.py:22-24`), undoing the 403
    /// coercion `handle_exception` applies when the first authenticator
    /// supplies no challenge. Only the `WWW-Authenticate` header follows
    /// the first authenticator (F4 `NotAuthenticated`).
    #[tokio::test]
    async fn not_authenticated_is_always_401() {
        let (status, headers, body) =
            response_parts(not_authenticated_response(Some("Bearer"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers.get(header::WWW_AUTHENTICATE).unwrap(), "Bearer");
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );

        // The create endpoint's `[APIKeyAuthentication]`: still 401 (the
        // adapter override), just without a challenge.
        let (status, headers, body) = response_parts(not_authenticated_response(None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(headers.get(header::WWW_AUTHENTICATE).is_none());
        assert_eq!(
            body,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
    }

    /// The 500 escape keeps the sibling JSON text (status-only contract).
    #[tokio::test]
    async fn server_error_is_json_500() {
        let (status, headers, body) = response_parts(server_error()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(
            body,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    /// `resolve_runner_for_run` (`authentication.py:243-254`): false without
    /// a credential or a runnerless run, else the id comparison.
    #[test]
    fn run_ownership_predicate() {
        let id = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert!(resolve_runner_for_run(Some(id), Some(id)));
        assert!(!resolve_runner_for_run(Some(id), Some(other)));
        assert!(!resolve_runner_for_run(None, Some(id)));
        assert!(!resolve_runner_for_run(Some(id), None));
        assert!(!resolve_runner_for_run(None, None));
    }

    /// Anonymous fall-through touches no database: no `Authorization` header
    /// resolves without a pool, and a non-`mt_` Bearer [REDACTED] through the
    /// machine-token class (the unreachable pool proves no query runs).
    #[tokio::test]
    async fn anonymous_paths_need_no_database() {
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("pool");
        let ring = KeyRing::dev_from_secret(b"secret");
        let headers = HeaderMap::new();
        assert!(
            authenticate_access_token(&pool, b"secret", &ring, &headers, None)
                .await
                .expect("anonymous")
                .is_none()
        );
        assert!(authenticate_machine_token(&pool, b"secret", &headers)
            .await
            .expect("anonymous")
            .is_none());
        assert!(matches!(
            authenticate_api_key(&pool, b"secret", &headers)
                .await
                .expect("missing"),
            ApiKeyOutcome::Missing
        ));
        // Non-mt_ Bearer [REDACTED] through the machine-token class untouched.
        let headers = bearer_headers("Bearer some-jwt");
        assert!(authenticate_machine_token(&pool, b"secret", &headers)
            .await
            .expect("fall-through")
            .is_none());
    }

    /// Missing/empty `X-Api-Key` is anonymous (`api_authentication.py:68-69`)
    /// — unreachable pool, so this proves the missing/empty arms never query.
    #[tokio::test]
    async fn api_key_missing_needs_no_database() {
        let pool = PgPool::connect_lazy("postgres://127.0.0.1:1/unused").expect("pool");
        let mut empty = HeaderMap::new();
        empty.insert(
            pidash_auth::token::API_KEY_HEADER,
            header::HeaderValue::from_static(""),
        );
        assert!(matches!(
            authenticate_api_key(&pool, b"secret", &empty)
                .await
                .expect("missing"),
            ApiKeyOutcome::Missing
        ));
    }
}
