//! Space public-API permission guards (D-02, stage 4).
//!
//! Port of the anchor-resolution, method-matrix, enabled-flag and
//! creator-ownership checks in `apps/api/pi_dash/space/views/` plus the base
//! envelopes in `views/base.py`. Every fallible check returns the exact DRF
//! error body and status the Python view returns; the `#[cfg(test)]` suite
//! asserts them against
//! `rust-api/fixtures/space/guards/permissions.golden.json` (recorded by
//! PIDASHCONV-135, traced in `rust-api/fixtures/space/TRACE.md`).
//!
//! The module is pure: DeployBoard reads are caller inputs ([`BoardView`]),
//! never queries. Tenant isolation is enforced by resolving every row through
//! the board's scope ([`BoardScope`]); a row outside the scope behaves as if
//! missing, exactly like the Python `workspace_id=` / `project_id=` filters.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-dispatch (`views/base.py:115-117`, `:198-200`): `dispatch` computes
//!   `response = self.handle_exception(exc)` and then `return exc` — the
//!   mapped response is discarded and the exception object itself is
//!   returned. [`dispatch_except`] models this: the computed response is
//!   returned alongside, but the live return value is the exception.
//! * BUG-vote-write-ungated (`views/issue.py:543-591`): vote `create` and
//!   `destroy` never check `is_votes_enabled` (only the list queryset does).
//!   [`vote_write_gated`] returns `false` to pin that.
//! * BUG-intake-auth (`views/intake.py:31-35`): the intake viewset declares
//!   no permission override, so the public-URL list/retrieve require
//!   authentication. [`requires_auth`] returns `true` for the intake routes.
//! * BUG-reaction-list-auth (`views/issue.py:342-519`): neither reaction
//!   viewset overrides permissions, so even list requires auth.
//!   [`requires_auth`] returns `true` there too.

use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Error responses
// ---------------------------------------------------------------------------

/// An exact DRF error response: HTTP status plus JSON body.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    /// HTTP status code.
    pub status: u16,
    /// Byte-exact JSON body.
    pub body: Value,
}

impl ErrorBody {
    fn new(status: u16, body: Value) -> Self {
        Self { status, body }
    }
}

/// `{"error": "Project is not published"}`, 404.
///
/// `views/project.py:23` settings (via `DoesNotExist`, see
/// [`resolve_board_get`] note), `views/issue.py:82` issue list,
/// `views/asset.py:73,140,161,180,198` post/patch/delete/restore/bulk,
/// `views/meta.py:23,29` meta (explicit `try/except DoesNotExist`).
pub fn project_not_published() -> ErrorBody {
    ErrorBody::new(404, json!({"error": "Project is not published"}))
}

/// `{"error": "Invalid anchor"}`, 404.
///
/// `views/project.py:71-74` members; identical in `views/cycle.py:21`,
/// `views/module.py:21`, `views/state.py:24`, `views/label.py:21`.
pub fn invalid_anchor() -> ErrorBody {
    ErrorBody::new(404, json!({"error": "Invalid anchor"}))
}

/// `{"error": "Requested resource could not be found."}`, 404 (note the
/// trailing period).
///
/// Asset GET with an unknown anchor only (`views/asset.py:39-42`).
pub fn requested_resource_not_found() -> ErrorBody {
    ErrorBody::new(
        404,
        json!({"error": "Requested resource could not be found."}),
    )
}

/// `{"error": "The requested asset could not be found."}`, 404.
///
/// Asset GET for a not-uploaded asset (`views/asset.py:56-59`); identical in
/// bulk when `assets.first()` is `None` (`views/asset.py:217-220`).
pub fn requested_asset_not_found() -> ErrorBody {
    ErrorBody::new(
        404,
        json!({"error": "The requested asset could not be found."}),
    )
}

/// `{"error": "Comments are not enabled for this project"}`, 400.
///
/// Comment create/partial_update/destroy
/// (`views/issue.py:261-264,300-303,324-327`).
pub fn comments_not_enabled() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Comments are not enabled for this project"}),
    )
}

/// `{"error": "Reactions are not enabled for this project board"}`, 400.
///
/// Issue-reaction create/destroy (`views/issue.py:370-373,407-410`; note
/// `project board`).
pub fn issue_reactions_not_enabled() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Reactions are not enabled for this project board"}),
    )
}

/// `{"error": "Reactions are not enabled for this board"}`, 400.
///
/// Comment-reaction create/destroy (`views/issue.py:455-458,491-494`; note
/// bare `board` — deliberately different from the issue-reaction wording).
pub fn comment_reactions_not_enabled() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Reactions are not enabled for this board"}),
    )
}

/// `{"error": "Intake is not enabled for this Project Board"}`, 400.
///
/// Intake list/create/partial_update/retrieve/destroy when
/// `board.intake is None`
/// (`views/intake.py:59-62,110-113,178-181,239-242,260-263`).
pub fn intake_not_enabled() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Intake is not enabled for this Project Board"}),
    )
}

/// `{"error": "Name is required"}`, 400 (intake create,
/// `views/intake.py:116`).
pub fn intake_name_required() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "Name is required"}))
}

/// `{"error": "Invalid priority"}`, 400 (intake create,
/// `views/intake.py:126`).
pub fn intake_invalid_priority() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "Invalid priority"}))
}

/// `{"error": "You cannot edit intake issues"}`, 400 (intake
/// partial_update by a non-creator, `views/intake.py:191-194`).
pub fn intake_edit_forbidden() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "You cannot edit intake issues"}))
}

/// `{"error": "You cannot delete intake issue"}`, 400 (intake destroy by a
/// non-creator, `views/intake.py:274-277`; note `delete` vs `edit`).
pub fn intake_delete_forbidden() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "You cannot delete intake issue"}))
}

/// `{"error": "Invalid entity type.", "status": false}`, 400 (asset post,
/// `views/asset.py:84-87`; the extra `status: false` and trailing period
/// are part of the wire shape).
pub fn invalid_entity_type() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Invalid entity type.", "status": false}),
    )
}

/// `{"error": "Invalid file type. ...", "status": false}`, 400 (asset post,
/// `views/asset.py:98-104`).
pub fn invalid_file_type() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({
            "error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
            "status": false,
        }),
    )
}

/// `{"error": "No asset ids provided."}`, 400 (bulk with empty
/// `asset_ids`, `views/asset.py:204`).
pub fn no_asset_ids() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "No asset ids provided."}))
}

/// `{"error": "Group by and sub group by cannot have same parameters"}`,
/// 400 (`views/issue.py:143-146`).
pub fn same_group_by() -> ErrorBody {
    ErrorBody::new(
        400,
        json!({"error": "Group by and sub group by cannot have same parameters"}),
    )
}

// ---------------------------------------------------------------------------
// Base envelopes (views/base.py handle_exception + get_queryset fallback)
// ---------------------------------------------------------------------------

/// The Django/DRF failure kinds `handle_exception` maps
/// (`views/base.py:65-103` ViewSet, `:149-186` APIView; both spellings map
/// identically — only the `KeyError` logging differs, see
/// [`handle_exception`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionKind {
    /// `django.db.IntegrityError` (`:74-78` / `:158-162`).
    IntegrityError,
    /// `django.core.exceptions.ValidationError` (`:80-84` / `:164-168`).
    ValidationError,
    /// `django.core.exceptions.ObjectDoesNotExist` (`:86-90` / `:170-174`).
    ObjectDoesNotExist,
    /// `KeyError` (`:92-97` / `:176-180`).
    KeyError,
    /// Anything else → 500 fallback (`:99-103` / `:182-186`).
    Other,
}

/// Port of `BaseViewSet.handle_exception` / `BaseAPIView.handle_exception`.
///
/// DRF-known exceptions keep their DRF mapping via
/// `super().handle_exception(exc)` (returned directly when it handles them);
/// this models the `except` branch, which runs when DRF re-raises. The
/// ViewSet `KeyError` arm logs via `log_exception` (`:92-97`); the APIView
/// arm does not (`:176-180`) — the response is identical either way, so
/// `log_key_error` only records whether the caller must log.
///
/// NOTE: under [`dispatch_except`] this computed response is discarded
/// (BUG-dispatch) — it is observable only if a future caller returns it.
pub fn handle_exception(kind: ExceptionKind) -> ErrorBody {
    match kind {
        ExceptionKind::IntegrityError => {
            ErrorBody::new(400, json!({"error": "The payload is not valid"}))
        }
        ExceptionKind::ValidationError => {
            ErrorBody::new(400, json!({"error": "Please provide valid detail"}))
        }
        ExceptionKind::ObjectDoesNotExist => {
            ErrorBody::new(404, json!({"error": "The required object does not exist."}))
        }
        ExceptionKind::KeyError => {
            ErrorBody::new(400, json!({"error": "The required key does not exist."}))
        }
        ExceptionKind::Other => ErrorBody::new(
            500,
            json!({"error": "Something went wrong please try again later"}),
        ),
    }
}

/// Whether the `KeyError` arm logs via `log_exception`: `true` for the
/// ViewSet base (`views/base.py:92-97`), `false` for the APIView base
/// (`:176-180`).
pub fn key_error_logs(is_viewset: bool) -> bool {
    is_viewset
}

/// `BaseViewSet.get_queryset` fallback (`views/base.py:58-63`): any failure
/// is logged and raised as `APIException("Please check the view", 400)`.
pub fn queryset_fallback() -> ErrorBody {
    ErrorBody::new(400, json!({"error": "Please check the view"}))
}

/// Outcome of `dispatch`'s `except` path (`views/base.py:105-117` ViewSet,
/// `:188-200` APIView).
///
/// BUG-dispatch: Python computes `response = self.handle_exception(exc)`
/// and then `return exc` — the mapped response is discarded and the
/// exception object itself propagates. Both bases share the bug.
pub struct DispatchOutcome {
    /// The mapped response `handle_exception` computed (discarded).
    pub computed_response: ErrorBody,
    /// What `dispatch` actually returns: always the exception, never the
    /// response (`return exc`, `:117` / `:200`).
    pub returned_is_exception: bool,
}

/// Port of the `dispatch` `except` path for either base class.
pub fn dispatch_except(kind: ExceptionKind) -> DispatchOutcome {
    DispatchOutcome {
        computed_response: handle_exception(kind),
        returned_is_exception: true,
    }
}

// ---------------------------------------------------------------------------
// TimezoneMixin (views/base.py:31-42)
// ---------------------------------------------------------------------------

/// What `TimezoneMixin.initial` does (`views/base.py:37-42`; mixed into
/// BOTH bases, `:45` and `:133`, so every space view runs it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimezoneAction {
    /// Authenticated: `timezone.activate(ZoneInfo(request.user.user_timezone))`
    /// (`:39-40`). No `try/except` — an invalid `user_timezone` raises
    /// `ZoneInfoNotFoundError`; callers must not pre-validate (port: pass
    /// the stored string through verbatim).
    Activate(String),
    /// Anonymous: `timezone.deactivate()` (`:42`).
    Deactivate,
}

/// Port of `TimezoneMixin.initial` after `super().initial(...)`
/// (`views/base.py:37-42`).
pub fn timezone_action(is_authenticated: bool, user_timezone: Option<&str>) -> TimezoneAction {
    if is_authenticated {
        TimezoneAction::Activate(user_timezone.unwrap_or_default().to_string())
    } else {
        TimezoneAction::Deactivate
    }
}

// ---------------------------------------------------------------------------
// Board inputs
// ---------------------------------------------------------------------------

/// The DeployBoard columns the guards read (`db/models/deploy_board.py:19-57`;
/// column list in `db/src/space/columns.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct BoardView {
    /// `anchor` (`:32`, globally unique).
    pub anchor: String,
    /// `entity_name` (`:30`; space reads scope to `"project"` where noted).
    pub entity_name: Option<String>,
    /// `workspace_id` — every row lookup is scoped to it (tenant isolation).
    pub workspace_id: String,
    /// `project_id` — write/delete/bulk lookups additionally scope to it.
    pub project_id: Option<String>,
    /// `is_comments_enabled` (`:33`, default `false`).
    pub is_comments_enabled: bool,
    /// `is_reactions_enabled` (`:34`, default `false`).
    pub is_reactions_enabled: bool,
    /// `intake` FK (`:35`, nullable): `None` disables every intake action.
    pub intake_id: Option<String>,
    /// `is_votes_enabled` (`:36`, default `false`).
    pub is_votes_enabled: bool,
}

/// Tenant scope derived from the resolved board: candidate rows must match
/// it, or they behave as missing (the Python `workspace_id=` /
/// `project_id=` filters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardScope {
    /// Required `workspace_id`.
    pub workspace_id: String,
    /// Required `project_id` (`None` = the board has none; any candidate
    /// project fails the check).
    pub project_id: Option<String>,
}

impl From<&BoardView> for BoardScope {
    fn from(board: &BoardView) -> Self {
        Self {
            workspace_id: board.workspace_id.clone(),
            project_id: board.project_id.clone(),
        }
    }
}

/// Workspace scoping: a candidate row from another workspace is invisible
/// (the Python `.get(workspace_id=board.workspace_id)` raises
/// `DoesNotExist` → [`ExceptionKind::ObjectDoesNotExist`] envelope).
/// Returns `Ok(())` on match, else the 404 envelope.
pub fn check_workspace_scope(
    scope: &BoardScope,
    candidate_workspace_id: &str,
) -> Result<(), ErrorBody> {
    if scope.workspace_id == candidate_workspace_id {
        Ok(())
    } else {
        Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
    }
}

/// Workspace+project scoping for delete/bulk paths
/// (`views/asset.py:163,207-211`: `.get(id, workspace, project_id)`).
pub fn check_workspace_project_scope(
    scope: &BoardScope,
    candidate_workspace_id: &str,
    candidate_project_id: Option<&str>,
) -> Result<(), ErrorBody> {
    check_workspace_scope(scope, candidate_workspace_id)?;
    if scope.project_id.as_deref() == candidate_project_id {
        Ok(())
    } else {
        Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
    }
}

// ---------------------------------------------------------------------------
// Anchor resolution
// ---------------------------------------------------------------------------

/// How Python resolves the board for each endpoint shape:
///
/// * `FilterFirst` — `DeployBoard.objects.filter(...).first()`; a miss
///   returns the endpoint's own 404 body inline.
/// * `Get` — `DeployBoard.objects.get(...)` with no `try/except`; a miss
///   raises `DeployBoard.DoesNotExist` (an `ObjectDoesNotExist`), which
///   `handle_exception` maps to the 404 envelope (and `dispatch` then
///   discards per BUG-dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorLookup {
    /// `filter(anchor=...).first()`; miss → [`requested_resource_not_found`].
    /// Asset GET only (`views/asset.py:36-42`).
    FilterFirstAssetGet,
    /// `filter(...).first()` (with `entity_name="project"` where the view
    /// passes it); miss → [`project_not_published`]. Issue list
    /// (`views/issue.py:80-82`), asset post/patch (`views/asset.py:70-73`,
    /// `:137-140`, no `entity_name` filter — PORT), asset
    /// delete/restore/bulk (`:158-161,:177-180,:195-198`, with
    /// `entity_name="project"`).
    FilterFirstNotPublished,
    /// `filter(anchor=...).first()` with no `entity_name` filter; miss →
    /// [`invalid_anchor`]. Members (`views/project.py:69-74`), cycles
    /// (`views/cycle.py:19-21`), modules (`views/module.py:19-21`), states
    /// (`views/state.py:21-24`), labels (`views/label.py:21-24`).
    FilterFirstInvalidAnchor,
    /// `get(anchor=..., entity_name="project")` with an explicit
    /// `try/except DoesNotExist` → [`project_not_published`]. Meta only
    /// (`views/meta.py:22-25,29-32`, for both the board and the project
    /// lookup).
    GetMeta,
    /// Bare `.get(...)` with no handler: a miss raises through to
    /// [`handle_exception`] (`ObjectDoesNotExist` envelope). Settings
    /// (`views/project.py:23`, `get(anchor, entity_name="project")` —
    /// PORT: NOT the "not published" body despite the golden source note),
    /// issue retrieve (`views/issue.py:598`, `get(anchor=...)` with NO
    /// `entity_name` filter — PORT), intake
    /// (`views/intake.py:57,108,176,237,259`), comments
    /// (`views/issue.py:230,258,297,321`), reactions
    /// (`:348,367,404,436,452,489`) and votes (`:528-529`, which passes the
    /// anchor as `workspace__slug` — BUG, owned by the queries layer).
    GetRaises,
}

/// Resolve the anchor gate. `board_found` is the DeployBoard read result.
/// Returns `Ok(())` when the request may proceed, else the exact error to
/// return (or, for [`AnchorLookup::GetRaises` on a miss, the envelope
/// `handle_exception` computes before `dispatch` discards it).
pub fn resolve_anchor(lookup: AnchorLookup, board_found: bool) -> Result<(), ErrorBody> {
    if board_found {
        return Ok(());
    }
    Err(match lookup {
        AnchorLookup::FilterFirstAssetGet => requested_resource_not_found(),
        AnchorLookup::FilterFirstNotPublished => project_not_published(),
        AnchorLookup::FilterFirstInvalidAnchor => invalid_anchor(),
        AnchorLookup::GetMeta => project_not_published(),
        AnchorLookup::GetRaises => handle_exception(ExceptionKind::ObjectDoesNotExist),
    })
}

// ---------------------------------------------------------------------------
// Method matrix
// ---------------------------------------------------------------------------

/// HTTP methods that reach the space routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

/// Space route families (URL table in `space/urls/*.py`; permission source
/// per variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET anchor/<anchor>/meta/` (`views/meta.py:17`).
    Meta,
    /// `GET anchor/<anchor>/settings/` (`views/project.py:20`).
    Settings,
    /// `GET anchor/<anchor>/issues/` (`views/issue.py:74`).
    IssueList,
    /// `GET anchor/<anchor>/issues/<issue_id>/` (`views/issue.py:595`).
    IssueRetrieve,
    /// `GET workspaces/<slug>/projects/<project_id>/anchor/`
    /// (`views/project.py:55`).
    Anchor,
    /// `GET workspaces/<slug>/project-boards/` (`views/project.py:29`).
    ProjectBoards,
    /// `GET anchor/<anchor>/cycles/` (`views/cycle.py:16`).
    Cycles,
    /// `GET anchor/<anchor>/modules/` (`views/module.py:16`).
    Modules,
    /// `GET anchor/<anchor>/states/` (`views/state.py:19`).
    States,
    /// `GET anchor/<anchor>/labels/` (`views/label.py:15`).
    Labels,
    /// `GET anchor/<anchor>/members/` (`views/project.py:66`).
    Members,
    /// Intake collection + detail (`views/intake.py:31-35`: no override —
    /// base `IsAuthenticated` default; PORT the public URL requiring auth).
    Intake,
    /// Comment collection + detail (`views/issue.py:220-226`:
    /// `get_permissions` switches on `action`: list/retrieve → `AllowAny`,
    /// else `IsAuthenticated`).
    Comments,
    /// Issue-reaction collection + destroy: no override, base default
    /// (`views/issue.py:342-427`).
    IssueReactions,
    /// Comment-reaction collection + destroy: no override, base default
    /// (`views/issue.py:430-519`).
    CommentReactions,
    /// Vote collection + destroy: no override, base default
    /// (`views/issue.py:522-591`).
    Votes,
    /// `EntityAssetEndpoint` (`views/asset.py:27-32`: `get_permissions`
    /// switches on `request.method`: GET → `AllowAny`, else
    /// `IsAuthenticated`).
    EntityAsset,
    /// Restore + bulk (`views/asset.py:172-226`: inherit the `BaseAPIView`
    /// default, `views/base.py:134`).
    AssetWrite,
}

/// Whether the route+method requires authentication.
///
/// Base default is `IsAuthenticated` (`views/base.py:48,134`, via
/// `BaseSessionAuthentication`, `:52,142`); each `AllowAny` below names its
/// override line. `action_is_list_or_retrieve` feeds the comment
/// `get_permissions` action switch (`views/issue.py:220-226`); it is ignored
/// for every other route.
pub fn requires_auth(route: Route, method: Method, action_is_list_or_retrieve: bool) -> bool {
    match route {
        Route::Meta
        | Route::Settings
        | Route::IssueList
        | Route::IssueRetrieve
        | Route::Anchor
        | Route::ProjectBoards
        | Route::Cycles
        | Route::Modules
        | Route::States
        | Route::Labels
        | Route::Members => false,
        Route::Intake => true,
        Route::Comments => !action_is_list_or_retrieve,
        Route::IssueReactions | Route::CommentReactions | Route::Votes => true,
        Route::EntityAsset => method != Method::Get,
        Route::AssetWrite => true,
    }
}

// ---------------------------------------------------------------------------
// Enabled flags
// ---------------------------------------------------------------------------

/// Comment write gate (`views/issue.py:260,299,323`): writes require
/// `board.is_comments_enabled`. Reads list through the queryset instead
/// (`.none()` when disabled, `views/issue.py:231-255` — queries layer).
pub fn check_comments_enabled(board: &BoardView) -> Result<(), ErrorBody> {
    if board.is_comments_enabled {
        Ok(())
    } else {
        Err(comments_not_enabled())
    }
}

/// Issue-reaction write gate (`views/issue.py:369,406`).
pub fn check_issue_reactions_enabled(board: &BoardView) -> Result<(), ErrorBody> {
    if board.is_reactions_enabled {
        Ok(())
    } else {
        Err(issue_reactions_not_enabled())
    }
}

/// Comment-reaction write gate (`views/issue.py:454,490`).
pub fn check_comment_reactions_enabled(board: &BoardView) -> Result<(), ErrorBody> {
    if board.is_reactions_enabled {
        Ok(())
    } else {
        Err(comment_reactions_not_enabled())
    }
}

/// Vote writes are NOT gated (`views/issue.py:543-591`: neither `create`
/// nor `destroy` checks `is_votes_enabled`; only the list queryset does,
/// `:531`). Pinned `false` so a future "fix" cannot silently add a gate.
pub fn vote_write_gated() -> bool {
    false
}

/// Intake gate for all five actions (`views/intake.py:58,109,177,238,260`):
/// `board.intake is None` → 400.
pub fn check_intake_enabled(board: &BoardView) -> Result<(), ErrorBody> {
    if board.intake_id.is_some() {
        Ok(())
    } else {
        Err(intake_not_enabled())
    }
}

// ---------------------------------------------------------------------------
// Creator ownership (intake partial_update/destroy)
// ---------------------------------------------------------------------------

/// Intake partial_update ownership (`views/intake.py:190-194`):
/// `str(intake_issue.created_by_id) != str(request.user.id)` → 400.
/// Both sides render through `str()`, so comparison is on the rendered
/// strings (canonical UUID text both sides).
pub fn check_intake_edit_owner(
    created_by_id: &str,
    request_user_id: &str,
) -> Result<(), ErrorBody> {
    if created_by_id == request_user_id {
        Ok(())
    } else {
        Err(intake_edit_forbidden())
    }
}

/// Intake destroy ownership (`views/intake.py:273-277`; `delete` wording).
pub fn check_intake_delete_owner(
    created_by_id: &str,
    request_user_id: &str,
) -> Result<(), ErrorBody> {
    if created_by_id == request_user_id {
        Ok(())
    } else {
        Err(intake_delete_forbidden())
    }
}

// ---------------------------------------------------------------------------
// Asset write pre-checks (views/asset.py post/bulk)
// ---------------------------------------------------------------------------

/// MIME allowlist for asset post (`views/asset.py:90-96`).
pub const ALLOWED_ASSET_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/jpg",
    "image/gif",
];

/// Entity-type allowlist probe: `entity_type not in
/// FileAsset.EntityTypeContext.values` → 400 (`views/asset.py:83-87`).
/// `is_known` is the `in values` test; the enum itself lives in the models
/// layer.
pub fn check_entity_type(is_known: bool) -> Result<(), ErrorBody> {
    if is_known {
        Ok(())
    } else {
        Err(invalid_entity_type())
    }
}

/// File-type check (`views/asset.py:97-104`).
pub fn check_file_type(mime: &str) -> Result<(), ErrorBody> {
    if ALLOWED_ASSET_TYPES.contains(&mime) {
        Ok(())
    } else {
        Err(invalid_file_type())
    }
}

/// Bulk requires a non-empty `asset_ids` (`views/asset.py:200-204`).
pub fn check_asset_ids_present(count: usize) -> Result<(), ErrorBody> {
    if count > 0 {
        Ok(())
    } else {
        Err(no_asset_ids())
    }
}

/// Intake create field pre-checks: name present (`views/intake.py:115-116`),
/// then priority in the five-way list (`:119-126`).
pub fn check_intake_create(name_present: bool, priority: &str) -> Result<(), ErrorBody> {
    if !name_present {
        return Err(intake_name_required());
    }
    match priority {
        "low" | "medium" | "high" | "urgent" | "none" => Ok(()),
        _ => Err(intake_invalid_priority()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn golden() -> Value {
        let path = format!(
            "{}/../../fixtures/space/guards/permissions.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn golden_error(action: &str) -> (u16, Value) {
        let errors = golden()
            .get("errors")
            .and_then(Value::as_array)
            .expect("errors array")
            .clone();
        let entry = errors
            .iter()
            .find(|e| {
                e.get("source")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains(action))
            })
            .unwrap_or_else(|| panic!("golden lacks error case {action}"));
        (
            entry.get("status").and_then(Value::as_u64).unwrap() as u16,
            entry.get("body").expect("body").clone(),
        )
    }

    fn assert_error(actual: ErrorBody, golden_action: &str) {
        let (status, body) = golden_error(golden_action);
        assert_eq!(actual.status, status, "status for {golden_action}");
        assert_eq!(actual.body, body, "body for {golden_action}");
    }

    fn board() -> BoardView {
        BoardView {
            anchor: "a".to_string(),
            entity_name: Some("project".to_string()),
            workspace_id: "ws-1".to_string(),
            project_id: Some("pr-1".to_string()),
            is_comments_enabled: true,
            is_reactions_enabled: true,
            intake_id: Some("in-1".to_string()),
            is_votes_enabled: true,
        }
    }

    // -- anchor gates: the three 404 bodies (+ the .get envelope) ----------

    #[test]
    fn anchor_bodies_match_golden() {
        assert_error(requested_resource_not_found(), "asset.py:39-42");
        // The "Project is not published" entry's source note names the
        // settings site, not the body text — match it by body.
        let (status, body) = golden_error("meta.py:23");
        assert_eq!(project_not_published().status, status);
        assert_eq!(project_not_published().body, body);
        assert_error(invalid_anchor(), "project.py:71-74");
        assert_error(requested_asset_not_found(), "asset.py:56-59");
    }

    #[test]
    fn resolve_anchor_routes_each_miss_to_its_body() {
        assert_eq!(
            resolve_anchor(AnchorLookup::FilterFirstAssetGet, false),
            Err(requested_resource_not_found())
        );
        assert_eq!(
            resolve_anchor(AnchorLookup::FilterFirstNotPublished, false),
            Err(project_not_published())
        );
        assert_eq!(
            resolve_anchor(AnchorLookup::FilterFirstInvalidAnchor, false),
            Err(invalid_anchor())
        );
        assert_eq!(
            resolve_anchor(AnchorLookup::GetMeta, false),
            Err(project_not_published())
        );
        // Bare .get() misses surface the ObjectDoesNotExist envelope
        // (handle_exception computes it; dispatch discards it).
        assert_eq!(
            resolve_anchor(AnchorLookup::GetRaises, false),
            Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
        );
        for lookup in [
            AnchorLookup::FilterFirstAssetGet,
            AnchorLookup::FilterFirstNotPublished,
            AnchorLookup::FilterFirstInvalidAnchor,
            AnchorLookup::GetMeta,
            AnchorLookup::GetRaises,
        ] {
            assert_eq!(resolve_anchor(lookup, true), Ok(()));
        }
    }

    // -- method matrix vs golden routes --------------------------------------

    #[test]
    fn allow_any_get_routes_need_no_auth() {
        for route in [
            Route::Meta,
            Route::Settings,
            Route::IssueList,
            Route::IssueRetrieve,
            Route::Anchor,
            Route::ProjectBoards,
            Route::Cycles,
            Route::Modules,
            Route::States,
            Route::Labels,
            Route::Members,
        ] {
            assert!(
                !requires_auth(route, Method::Get, false),
                "{route:?} must be AllowAny"
            );
        }
    }

    #[test]
    fn auth_matrix_write_side() {
        // Intake: authenticated even for public-URL reads (no override).
        assert!(requires_auth(Route::Intake, Method::Get, true));
        assert!(requires_auth(Route::Intake, Method::Post, false));
        // Comments: list/retrieve open, writes closed.
        assert!(!requires_auth(Route::Comments, Method::Get, true));
        assert!(requires_auth(Route::Comments, Method::Post, false));
        assert!(requires_auth(Route::Comments, Method::Patch, false));
        assert!(requires_auth(Route::Comments, Method::Delete, false));
        // Reactions + votes: closed even for list (no override).
        for route in [Route::IssueReactions, Route::CommentReactions, Route::Votes] {
            assert!(requires_auth(route, Method::Get, true), "{route:?} list");
            assert!(requires_auth(route, Method::Post, false), "{route:?} write");
        }
        // Entity assets: GET open, everything else closed; restore/bulk closed.
        assert!(!requires_auth(Route::EntityAsset, Method::Get, false));
        assert!(requires_auth(Route::EntityAsset, Method::Post, false));
        assert!(requires_auth(Route::EntityAsset, Method::Patch, false));
        assert!(requires_auth(Route::EntityAsset, Method::Delete, false));
        assert!(requires_auth(Route::AssetWrite, Method::Post, false));
    }

    #[test]
    fn denied_permission_probes() {
        // Anonymous caller on auth-required routes is denied on every method.
        let denied = [
            (Route::Intake, Method::Get, true),
            (Route::Intake, Method::Post, false),
            (Route::Comments, Method::Post, false),
            (Route::IssueReactions, Method::Get, true),
            (Route::CommentReactions, Method::Delete, false),
            (Route::Votes, Method::Get, true),
            (Route::EntityAsset, Method::Post, false),
            (Route::EntityAsset, Method::Patch, false),
            (Route::EntityAsset, Method::Delete, false),
            (Route::AssetWrite, Method::Post, false),
        ];
        for (route, method, list) in denied {
            assert!(
                requires_auth(route, method, list),
                "anonymous must be denied on {route:?} {method:?}"
            );
        }
        // Anonymous caller on AllowAny GETs is admitted.
        assert!(!requires_auth(Route::IssueList, Method::Get, false));
        assert!(!requires_auth(Route::EntityAsset, Method::Get, false));
        assert!(!requires_auth(Route::Comments, Method::Get, true));
    }

    // -- enabled flags --------------------------------------------------------

    #[test]
    fn enabled_flag_bodies_match_golden() {
        assert_error(comments_not_enabled(), "issue.py:261-264");
        assert_error(issue_reactions_not_enabled(), "issue.py:370-373");
        assert_error(comment_reactions_not_enabled(), "issue.py:455-458");
        assert_error(intake_not_enabled(), "intake.py:59-62");
        assert_error(intake_name_required(), "intake.py:116");
        assert_error(intake_invalid_priority(), "intake.py:126");
        assert_error(intake_edit_forbidden(), "intake.py:191-194");
        assert_error(intake_delete_forbidden(), "intake.py:274-277");
        assert_error(invalid_entity_type(), "asset.py:84-87");
        assert_error(invalid_file_type(), "asset.py:98-104");
        assert_error(no_asset_ids(), "asset.py:204");
        assert_error(same_group_by(), "issue.py:143-146");
    }

    #[test]
    fn enabled_flags_gate_per_board_state() {
        let on = board();
        assert_eq!(check_comments_enabled(&on), Ok(()));
        assert_eq!(check_issue_reactions_enabled(&on), Ok(()));
        assert_eq!(check_comment_reactions_enabled(&on), Ok(()));
        assert_eq!(check_intake_enabled(&on), Ok(()));

        let mut off = on.clone();
        off.is_comments_enabled = false;
        off.is_reactions_enabled = false;
        off.intake_id = None;
        assert_eq!(check_comments_enabled(&off), Err(comments_not_enabled()));
        assert_eq!(
            check_issue_reactions_enabled(&off),
            Err(issue_reactions_not_enabled())
        );
        assert_eq!(
            check_comment_reactions_enabled(&off),
            Err(comment_reactions_not_enabled())
        );
        assert_eq!(check_intake_enabled(&off), Err(intake_not_enabled()));
        // Vote writes are never gated (BUG-vote-write-ungated).
        assert!(!vote_write_gated());
    }

    #[test]
    fn issue_vs_comment_reaction_wording_differs() {
        assert_ne!(
            issue_reactions_not_enabled().body,
            comment_reactions_not_enabled().body,
            "golden pins 'project board' vs bare 'board'"
        );
    }

    // -- creator ownership -----------------------------------------------------

    #[test]
    fn intake_ownership_owner_passes_stranger_fails() {
        let me = "11111111-1111-1111-1111-111111111111";
        let other = "22222222-2222-2222-2222-222222222222";
        assert_eq!(check_intake_edit_owner(me, me), Ok(()));
        assert_eq!(check_intake_delete_owner(me, me), Ok(()));
        assert_eq!(
            check_intake_edit_owner(me, other),
            Err(intake_edit_forbidden())
        );
        assert_eq!(
            check_intake_delete_owner(me, other),
            Err(intake_delete_forbidden())
        );
        // Edit vs delete wording differs (golden pins it).
        assert_ne!(intake_edit_forbidden().body, intake_delete_forbidden().body);
    }

    // -- tenant isolation -------------------------------------------------------

    #[test]
    fn tenant_isolation_cross_workspace_row_is_missing() {
        let scope = BoardScope::from(&board());
        assert_eq!(check_workspace_scope(&scope, "ws-1"), Ok(()));
        assert_eq!(
            check_workspace_scope(&scope, "ws-other"),
            Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
        );
        assert_eq!(
            check_workspace_project_scope(&scope, "ws-1", Some("pr-1")),
            Ok(())
        );
        // Same workspace but another project: invisible on project-scoped paths.
        assert_eq!(
            check_workspace_project_scope(&scope, "ws-1", Some("pr-other")),
            Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
        );
        // Board without project: any candidate project fails the check.
        let scope_noproject = BoardScope {
            workspace_id: "ws-1".to_string(),
            project_id: None,
        };
        assert_eq!(
            check_workspace_project_scope(&scope_noproject, "ws-1", Some("pr-1")),
            Err(handle_exception(ExceptionKind::ObjectDoesNotExist))
        );
    }

    // -- base envelopes ----------------------------------------------------------

    #[test]
    fn handle_exception_mapping_matches_golden() {
        let doc = golden();
        let mapped = doc.get("handle_exception").expect("handle_exception map");
        for (kind, key) in [
            (ExceptionKind::IntegrityError, "integrity_error"),
            (ExceptionKind::ValidationError, "validation_error_django"),
            (ExceptionKind::ObjectDoesNotExist, "object_does_not_exist"),
            (ExceptionKind::KeyError, "key_error"),
            (ExceptionKind::Other, "fallback"),
        ] {
            let entry = mapped
                .get(key)
                .unwrap_or_else(|| panic!("golden lacks {key}"));
            let actual = handle_exception(kind);
            assert_eq!(
                actual.status,
                entry.get("status").and_then(Value::as_u64).unwrap() as u16,
                "status for {key}"
            );
            assert_eq!(actual.body, entry["body"], "body for {key}");
        }
    }

    #[test]
    fn key_error_logging_differs_by_base() {
        assert!(key_error_logs(true));
        assert!(!key_error_logs(false));
    }

    #[test]
    fn dispatch_bug_computed_then_discarded_for_both_bases() {
        // Both dispatch variants (ViewSet :105-117, APIView :188-200) share
        // the bug: the mapped response is computed and discarded; the
        // exception itself is returned.
        for kind in [
            ExceptionKind::IntegrityError,
            ExceptionKind::ValidationError,
            ExceptionKind::ObjectDoesNotExist,
            ExceptionKind::KeyError,
            ExceptionKind::Other,
        ] {
            let outcome = dispatch_except(kind);
            assert_eq!(outcome.computed_response, handle_exception(kind));
            assert!(
                outcome.returned_is_exception,
                "dispatch must return exc, not response ({kind:?})"
            );
        }
    }

    #[test]
    fn timezone_mixin_rule() {
        assert_eq!(
            timezone_action(true, Some("Asia/Kathmandu")),
            TimezoneAction::Activate("Asia/Kathmandu".to_string())
        );
        assert_eq!(timezone_action(false, None), TimezoneAction::Deactivate);
        // Authenticated without a stored zone passes the empty string through
        // (no try/except in Python — no validation here either).
        assert_eq!(
            timezone_action(true, None),
            TimezoneAction::Activate(String::new())
        );
    }

    // -- asset / intake pre-checks -------------------------------------------------

    #[test]
    fn asset_prechecks() {
        assert_eq!(check_entity_type(true), Ok(()));
        assert_eq!(check_entity_type(false), Err(invalid_entity_type()));
        assert_eq!(check_file_type("image/png"), Ok(()));
        assert_eq!(check_file_type("application/pdf"), Err(invalid_file_type()));
        assert_eq!(check_asset_ids_present(3), Ok(()));
        assert_eq!(check_asset_ids_present(0), Err(no_asset_ids()));
    }

    #[test]
    fn intake_create_prechecks_in_order() {
        assert_eq!(check_intake_create(true, "high"), Ok(()));
        // Name check runs before priority (Python `if` order :115 then :119).
        assert_eq!(
            check_intake_create(false, "bogus"),
            Err(intake_name_required())
        );
        assert_eq!(
            check_intake_create(true, "bogus"),
            Err(intake_invalid_priority())
        );
    }
}
