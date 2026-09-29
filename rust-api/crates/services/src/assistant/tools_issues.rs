//! Assistant issue query + write tools (D-06, stage 5).
//!
//! Pure port of `apps/api/pi_dash/assistant/tools/issues.py:1-490`:
//! nine `@assistant.tool` functions (`search_issues :56`, `list_issues :83`,
//! `list_my_issues :133`, `get_issue :155`, `create_issue :228`,
//! `update_issue :287`, `relate_issues :448`, `unrelate_issues :468`,
//! `list_issue_relations :483`) plus the resolvers (`_resolve_state :185`,
//! `_resolve_parent :195`, `_resolve_issue_refs :402`, `_relations_view :411`,
//! `_relation_write :422`).
//!
//! Layering, per the Porting guide crate graph (`types` -> `db` ->
//! `services` -> `api`):
//!
//! * Everything here is DB-free: shaping, validation, SQL fragments (as
//!   string builders the handler layer executes), and JSON Schemas for tool
//!   registration. The live pieces stay where the guide puts them —
//!   execution in the handler layer, post-commit side effects behind the
//!   transactions-row wrapper (`record_write` + the orchestration dispatch
//!   run on commit, never inside the write transaction).
//! * Tenancy comes from [`AssistantDeps`](super::agent::AssistantDeps);
//!   scoping helpers live in the sibling tools-core port (PIDASHCONV-252)
//!   and the SQL below is written to compose with them. Description HTML
//!   reuses [`to_safe_html`](super::markdown::to_safe_html).
//! * No `schemars` dependency is added (foundation `Cargo.toml` files are
//!   read-only for port agents; PIDASHCONV-251 set the precedent with
//!   `rmcp`): [`tool_schemas`] returns the same JSON Schema documents as
//!   `serde_json::Value`, built by hand from the Python signatures.
//!
//! Fixture: `rust-api/fixtures/assistant/tools-tasks.json` (`F-A6-10`,
//! `tools.issues` + `tools.caps`); scoping/error vectors under
//! `tools-tasks.json#/scoping` and `#/results_helpers`.

use serde_json::{json, Value};
use uuid::Uuid;

use super::agent::AssistantDeps;

/// `issues.py:25` — page window for every list/search tool.
pub const SEARCH_LIMIT: i64 = 20;
/// `issues.py:26` — brief-name truncation cap (code points).
pub const NAME_CAP: usize = 200;
/// `issues.py:27` — detail-description truncation cap (code points).
pub const DESC_CAP: usize = 2000;
/// `issues.py:28` — per-comment truncation cap (code points).
pub const COMMENT_CAP: usize = 500;
/// `issues.py:33` — sentinel default for `update_issue.parent_issue_id`;
/// distinguishes "argument omitted" from explicit `null` (unlink).
pub const PARENT_UNSET: &str = "__unset__";
/// `issues.py:24` — accepted priority values.
pub const VALID_PRIORITIES: [&str; 5] = ["urgent", "high", "medium", "low", "none"];
/// Byte-exact rendering of `f"Priority must be one of
/// {sorted(_VALID_PRIORITIES)}."` (`issues.py:249,311`): Python `sorted`
/// over the set, then `list.__repr__` (single quotes, `", "` separator).
pub const PRIORITY_CHOICES_MESSAGE: &str =
    "Priority must be one of ['high', 'low', 'medium', 'none', 'urgent'].";
/// `constants.py:76-84` — lifecycle order; the `state_group` filter error
/// lists them in this order (`issue_filters.py:602`).
pub const STATE_GROUP_ORDER: [&str; 7] = [
    "backlog",
    "unstarted",
    "started",
    "review",
    "test",
    "completed",
    "cancelled",
];
/// `relations.py:73-75` — per-type cap on grouped relation lists.
pub const GROUP_LIMIT: usize = 100;
/// `relations.py:53-64` — every relation type an agent may name, display
/// order. [`validate_relation_type`] joins them in this order.
pub const RELATION_TYPES: [&str; 10] = [
    "blocked_by",
    "blocking",
    "relates_to",
    "duplicate",
    "start_before",
    "start_after",
    "finish_before",
    "finish_after",
    "implemented_by",
    "implements",
];
/// `relations.py:68` — types stored with the ends swapped under their
/// forward name (`blocking` is written as `blocked_by` on the other end).
pub fn is_reverse_relation(relation_type: &str) -> bool {
    matches!(
        relation_type,
        "blocking" | "start_after" | "finish_after" | "implements"
    )
}
/// `search/issue.py:45` — `Issue.sequence_id` is Postgres `int4`; wider
/// integers pushed into the equality predicate 500 the endpoint.
pub const SEQUENCE_ID_MAX: i64 = 2_147_483_647;
/// `assistant/models.py:108-114` — `MessageKind.TOOL_RESULT` value stored
/// by `record_write` (`_results.py:49-69`).
pub const TOOL_RESULT_KIND: &str = "tool_result";
/// Project-member roles allowed to write (`_scoping.py:92-103` calling
/// `check_project_role` with `[ROLE_ADMIN, ROLE_MEMBER]`;
/// `core/permissions.py:23-24`).
pub const WRITE_ROLES: [i32; 2] = [20, 15];

// ---------------------------------------------------------------------------
// Tool errors
// ---------------------------------------------------------------------------

/// `_scoping.py:28-36` — both error types subclass `ModelRetry`, so
/// pydantic-ai feeds the message back to the model instead of failing the
/// turn. [`ToolError::Retry`] is the general case (filter errors,
/// validation); the two scoped variants carry the exact Python messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// `ToolNotFound` — outside the member scope or missing.
    NotFound(String),
    /// `ToolPermissionError` — write attempted without ADMIN/MEMBER.
    Permission(String),
    /// Any other `ModelRetry` (filter/validation/relation errors).
    Retry(String),
}

impl ToolError {
    /// `_scoping.py:52-56` — member-scope miss on a project lookup.
    pub fn project_not_found(project_id: &str) -> Self {
        ToolError::NotFound(format!("Project {project_id} not found or not accessible."))
    }

    /// `_scoping.py:67-85` — scoped miss, or a malformed UUID
    /// (`ValidationError`/`ValueError` at query time becomes `ToolNotFound`
    /// so the model gets a retry message instead of crashing the turn).
    pub fn issue_not_found(issue_id: &str) -> Self {
        ToolError::NotFound(format!("Issue {issue_id} not found or not accessible."))
    }

    /// `_scoping.py:92-103` — guest (or non-member) write attempt.
    pub fn write_denied() -> Self {
        ToolError::Permission(
            "You don't have permission to make changes in this project.".to_owned(),
        )
    }

    /// `_resolve_state` (`issues.py:185-192`) — unknown state id.
    pub fn invalid_state(state_id: &str) -> Self {
        ToolError::Retry(format!(
            "State {state_id} is not a valid state for this project."
        ))
    }

    pub fn message(&self) -> &str {
        match self {
            ToolError::NotFound(message)
            | ToolError::Permission(message)
            | ToolError::Retry(message) => message,
        }
    }
}

/// Python `str.strip()` edge (same trap as `markdown::strip_paragraph`):
/// `strip()` also trims `\x1c`-`\x1f`, which Rust's Unicode `trim`
/// leaves. Every `strip()` site in the ported tools goes through this so
/// control-char-padded inputs behave identically.
pub fn py_strip(text: &str) -> &str {
    text.trim_matches(|ch: char| ch.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&ch))
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for ToolError {}

// ---------------------------------------------------------------------------
// Result shaping (_results.py + _brief)
// ---------------------------------------------------------------------------

/// `_results.py:32-36` — returns `(text, truncated)`. The cut is a hard
/// slice with no ellipsis. **Code points, not bytes**: Python `len` counts
/// `str` characters and `s[:limit]` slices them, so this uses `chars()`
/// — byte slicing would both miscount and panic on a UTF-8 boundary
/// (Porting guide semantic traps).
pub fn truncate(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_owned(), false);
    }
    (text.chars().take(limit).collect(), true)
}

/// `_results.py:20-29` — wraps user-generated text so the model treats it
/// as data. Both delimiters are neutralized inside the content (zero-width
/// space U+200B breaks the tag) so a malicious issue/comment body can
/// neither open a nested frame nor close the wrapper early. Replacement
/// order is load-bearing: `</untrusted>` first, then `<untrusted>`.
pub fn wrap_untrusted(text: &str) -> String {
    let safe = text
        .replace("</untrusted>", "<\u{200b}/untrusted>")
        .replace("<untrusted>", "<\u{200b}untrusted>");
    format!("<untrusted>{safe}</untrusted>")
}

/// `_results.py:39-46` — link row persisted alongside a write summary.
pub fn issue_link(deps: &AssistantDeps, project_id: &str, issue_id: &str) -> Value {
    json!({
        "type": "issue",
        "workspace_slug": deps.workspace_slug,
        "project_id": project_id,
        "issue_id": issue_id,
        "url_path": format!(
            "/{}/projects/{}/issues/{}",
            deps.workspace_slug, project_id, issue_id
        ),
    })
}

/// `_identifier` (`issues.py:36-38`): `PROJ-123` when the project has an
/// identifier, else the bare sequence id. (The relations module's
/// `identifier` in `relations.py:89-90` has no fallback — see
/// [`relation_identifier`].)
pub fn tool_identifier(project_identifier: &str, sequence_id: i64) -> String {
    if project_identifier.is_empty() {
        sequence_id.to_string()
    } else {
        format!("{project_identifier}-{sequence_id}")
    }
}

/// `relations.py:89-90` — assumes the project identifier is present.
pub fn relation_identifier(project_identifier: &str, sequence_id: i64) -> String {
    format!("{project_identifier}-{sequence_id}")
}

/// One row of `_brief` (`issues.py:41-53`).
pub struct BriefRow<'a> {
    pub id: &'a str,
    pub project_id: &'a str,
    pub project_identifier: &'a str,
    pub sequence_id: i64,
    pub name: &'a str,
    pub parent_id: Option<&'a str>,
    pub state_name: Option<&'a str>,
    pub state_group: Option<&'a str>,
    pub priority: &'a str,
}

/// `_brief` (`issues.py:41-53`): name wrapped + truncated at 200 code
/// points; `parent_id` renders `None` when unset; state fields render
/// `None` when the issue has no state.
pub fn brief_row(row: &BriefRow<'_>) -> Value {
    let (name, name_truncated) = truncate(row.name, NAME_CAP);
    json!({
        "id": row.id,
        "identifier": tool_identifier(row.project_identifier, row.sequence_id),
        "project_id": row.project_id,
        "name": wrap_untrusted(&name),
        "name_truncated": name_truncated,
        "parent_id": row.parent_id,
        "state": row.state_name,
        "state_group": row.state_group,
        "priority": row.priority,
    })
}

/// Author rule in `get_issue` (`issues.py:161-165`): agent turns render
/// `Pi Dash AI`; otherwise the actor's display name, falling back to
/// email, falling back to `Unknown` when there is no actor row.
pub fn comment_author(
    speaker_type: &str,
    display_name: Option<&str>,
    email: Option<&str>,
    has_actor: bool,
) -> String {
    if speaker_type == "agent" {
        return "Pi Dash AI".to_owned();
    }
    if !has_actor {
        return "Unknown".to_owned();
    }
    match display_name.filter(|name| !name.is_empty()) {
        Some(name) => name.to_owned(),
        None => email.unwrap_or("").to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Pagination (search_issues :72-80, list_issues :122-130, list_my_issues)
// ---------------------------------------------------------------------------

/// `max(1, min(int(limit or _SEARCH_LIMIT), _SEARCH_LIMIT))`
/// (`issues.py:72,122,144`): falsy (`0`) falls back to the default before
/// clamping; negatives saturate at 1.
pub fn clamp_limit(limit: i64) -> i64 {
    let effective = if limit == 0 { SEARCH_LIMIT } else { limit };
    effective.clamp(1, SEARCH_LIMIT)
}

/// `max(0, int(offset or 0))` (`issues.py:73,123,145`).
pub fn clamp_offset(offset: i64) -> i64 {
    offset.max(0)
}

/// The `limit + 1` lookahead window (`issues.py:74-79`): one extra row is
/// fetched; `has_more` is set and `next_offset` advances only when the
/// extra row exists.
pub fn page_window(total_fetched: usize, limit: i64, offset: i64) -> (bool, Option<i64>) {
    let has_more = total_fetched as i64 > limit;
    let next_offset = has_more.then(|| offset + limit);
    (has_more, next_offset)
}

// ---------------------------------------------------------------------------
// _resolve_state + _resolve_parent
// ---------------------------------------------------------------------------

/// One project state row for [`resolve_state`]. `sequence` is a float in
/// the schema (`state.py:98`, `FloatField`), ordered with `min_by` on the
/// float bits — NaN never occurs in practice and sorts deterministically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateRef<'a> {
    pub id: &'a str,
    pub is_default: bool,
    pub sequence: f64,
}

/// `_resolve_state` (`issues.py:185-192`): explicit id must exist in the
/// project (else `ModelRetry`); otherwise the default state, else the
/// lowest-`sequence` state (which may be `None` when the project has no
/// states — the caller then creates the issue with a null state, exactly
/// as Django's `state=None` assignment does). Input rows come from
/// `project_states` (`_scoping.py:87-89`): `State.objects` scoped to the
/// project + workspace slug, whose default manager already excludes
/// soft-deleted and triage states (`state.py:79-84`) — the caller queries
/// them, it does not re-filter here.
pub fn resolve_state<'a>(
    state_id: Option<&str>,
    states: &'a [StateRef<'a>],
) -> Result<Option<&'a StateRef<'a>>, ToolError> {
    if let Some(wanted) = state_id {
        if !wanted.is_empty() {
            return states
                .iter()
                .find(|state| state.id == wanted)
                .map(Some)
                .ok_or_else(|| ToolError::invalid_state(wanted));
        }
    }
    Ok(states.iter().find(|state| state.is_default).or_else(|| {
        states
            .iter()
            .min_by(|a, b| a.sequence.total_cmp(&b.sequence))
    }))
}

/// Parent-link validation (`_resolve_parent`, `issues.py:195-225`):
/// same-project check, self-parenting, and the ancestor-chain walk (with
/// the `seen` guard against pre-existing cycles). The chain is supplied
/// by the caller as the parent ids from the proposed parent upwards;
/// `None` ends the walk, mirroring `.first()` returning `None`.
pub fn check_parent_link(
    child_project_id: &str,
    parent_project_id: &str,
    parent_id: &str,
    child_id: Option<&str>,
    parent_ancestors: &[Option<String>],
) -> Result<(), ToolError> {
    if parent_project_id != child_project_id {
        return Err(ToolError::Retry(
            "The parent issue must be in the same project as the child issue.".to_owned(),
        ));
    }
    if let Some(child) = child_id {
        if parent_id == child {
            return Err(ToolError::Retry(
                "An issue can't be its own parent.".to_owned(),
            ));
        }
        let mut seen: Vec<String> = Vec::new();
        // Walk mirrors the Python loop one step per supplied ancestor:
        // each entry is the `parent_id` read for the current ancestor,
        // starting with the proposed parent's own `parent_id`.
        for next in parent_ancestors {
            match next {
                None => break,
                Some(id) if id == child => {
                    return Err(ToolError::Retry(
                        "That parent link would create a cycle.".to_owned(),
                    ));
                }
                Some(id) if seen.iter().any(|s| s == id) => break,
                Some(id) => seen.push(id.clone()),
            }
        }
    }
    Ok(())
}

/// `update_issue` parent tri-state (`issues.py:315-319`): the sentinel
/// means untouched, `None`/`""` means unlink, anything else resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentUpdate<'a> {
    Untouched,
    Unlink,
    Link(&'a str),
}

pub fn parent_update<'a>(parent_issue_id: Option<&'a str>) -> ParentUpdate<'a> {
    match parent_issue_id {
        None | Some("") => ParentUpdate::Unlink,
        Some(PARENT_UNSET) => ParentUpdate::Untouched,
        Some(raw) => ParentUpdate::Link(raw),
    }
}

// ---------------------------------------------------------------------------
// create_issue / update_issue validation + write plans
// ---------------------------------------------------------------------------

/// `create_issue` name rule (`issues.py:245-246`): blank names retry;
/// otherwise stripped and cut to 255 **code points** (`name.strip()[:255]`).
pub fn validate_create_name(name: &str) -> Result<String, ToolError> {
    if py_strip(name).is_empty() {
        return Err(ToolError::Retry("An issue name is required.".to_owned()));
    }
    Ok(py_strip(name).chars().take(255).collect())
}

/// Create priority (`issues.py:247-249`): `(priority or "none").lower()` —
/// both `None` and `""` fall back to the `"none"` default before the
/// membership check; the error is the byte-exact
/// [`PRIORITY_CHOICES_MESSAGE`].
pub fn validate_priority(priority: Option<&str>) -> Result<String, ToolError> {
    let raw = priority.unwrap_or("none");
    let prio = if raw.is_empty() { "none" } else { raw }.to_lowercase();
    if VALID_PRIORITIES.contains(&prio.as_str()) {
        Ok(prio)
    } else {
        Err(ToolError::Retry(PRIORITY_CHOICES_MESSAGE.to_owned()))
    }
}

/// `update_issue` priority (`issues.py:307-311`): `None` means untouched;
/// any given value (including `""`) is lowercased and checked — there is
/// no empty-to-default mapping on this path.
pub fn validate_update_priority(priority: Option<&str>) -> Result<Option<String>, ToolError> {
    match priority {
        None => Ok(None),
        Some(value) => {
            let prio = value.to_lowercase();
            if VALID_PRIORITIES.contains(&prio.as_str()) {
                Ok(Some(prio))
            } else {
                Err(ToolError::Retry(PRIORITY_CHOICES_MESSAGE.to_owned()))
            }
        }
    }
}

/// One validated `update_issue` call (`issues.py:287-391`).
pub struct UpdateInput<'a> {
    pub name: Option<&'a str>,
    pub description_md: Option<&'a str>,
    pub priority: Option<&'a str>,
    pub state_given: bool,
    pub parent: ParentUpdate<'a>,
}

/// Column writes plus the `changed` labels, in Python append order
/// (`issues.py:337-361`): name, description, priority, state, parent, then
/// the audit columns. A whitespace-only `name` is silently ignored
/// (ported bug: no change recorded, no error — `issues.py:338`).
pub fn plan_update(
    input: &UpdateInput<'_>,
) -> Result<(Vec<&'static str>, Vec<&'static str>), ToolError> {
    let mut changed: Vec<&'static str> = Vec::new();
    let mut update_fields: Vec<&'static str> = Vec::new();
    if let Some(name) = input.name {
        if !py_strip(name).is_empty() {
            changed.push("name");
            update_fields.push("name");
        }
    }
    if input.description_md.is_some() {
        changed.push("description");
        update_fields.push("description_html");
        update_fields.push("description_json");
    }
    if input.priority.is_some() {
        changed.push("priority");
        update_fields.push("priority");
    }
    if input.state_given {
        changed.push("state");
        update_fields.push("state");
    }
    if !matches!(input.parent, ParentUpdate::Untouched) {
        changed.push("parent");
        update_fields.push("parent");
    }
    if changed.is_empty() {
        // `issues.py:320-327` — note the em dash (U+2014), byte-exact.
        return Err(ToolError::Retry(
            "Nothing to update \u{2014} provide at least one field to change.".to_owned(),
        ));
    }
    // `BaseModel.save` sets `updated_by` from the impersonated user;
    // `updated_at` is `auto_now` (`issues.py:359-361`).
    update_fields.push("updated_at");
    update_fields.push("updated_by");
    Ok((changed, update_fields))
}

/// Mirror-the-UI dispatch (`issues.py:372-381`): only a state change to a
/// *different* state routes through
/// `orchestration.handle_issue_state_transition` (with the immediate flag
/// suppressed on the save so one move makes one run). Returns whether the
/// handler must dispatch with `dispatch_immediate = true`.
pub fn should_dispatch_transition(state_given: bool, from_state: &str, to_state: &str) -> bool {
    state_given && from_state != to_state
}

/// `create_issue` sequence allocation (`issues.py:256-264`): the project
/// row is locked first, then `MAX(sequence_id) + 1` (0 when empty).
/// Returns the next sequence for a locked project row.
pub fn next_sequence(max_sequence_id: Option<i64>) -> i64 {
    max_sequence_id.unwrap_or(0) + 1
}

/// Columns written by `Issue.objects.create` (`issues.py:265-277`):
/// `description_html` comes from `to_safe_html` (`None` renders `<p></p>`)
/// and `description_json` is always empty (no Tiptap converter).
pub fn create_description_html(description_md: Option<&str>) -> String {
    super::markdown::to_safe_html(description_md)
}

// ---------------------------------------------------------------------------
// Scoping SQL (member_projects / scoped_issues / my_issues / write gate)
// ---------------------------------------------------------------------------

/// `member_projects` (`_scoping.py:43-49`): projects in the workspace where
/// the user holds an active membership row. Drives `get_project`,
/// the token lookup, and every scoped queryset below. Table names are the
/// models' `db_table`s (`projects`, `project_members`, `workspaces`).
/// `$1` is the user id, `$2` the workspace slug. The queried model is
/// `Project`, whose default manager (`SoftDeleteModel.objects`,
/// `db/mixins.py:56-58`) excludes soft-deleted rows — hence the
/// `projects.deleted_at` predicate. Join predicates never carry manager
/// filters in Django, so the joined tables need none.
pub const MEMBER_PROJECTS_SQL: &str = "SELECT DISTINCT projects.id FROM projects \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     INNER JOIN workspaces ON workspaces.id = projects.workspace_id \
     WHERE workspaces.slug = $2 AND projects.deleted_at IS NULL";

/// Exact miss message is [`ToolError::project_not_found`].
/// Malformed-UUID handling ([`ToolError::issue_not_found`]) needs no
/// translation layer in Rust: `Uuid::parse_str` failing *is* the miss.
pub fn parse_scoped_issue_id(raw: &str) -> Result<Uuid, ToolError> {
    Uuid::parse_str(strip_uuid_braces(raw)).map_err(|_| ToolError::issue_not_found(raw))
}

/// `scoped_issues` = `member_project_issues` (`core/querysets.py:19-30`)
/// through the `Issue.issue_objects` manager (`db/models/issue.py:95-104`,
/// which additionally excludes triage states, archived rows/projects, and
/// drafts). Every read tool ANDs these predicates.
///
/// Triage trap: Django renders `.exclude(state__group='triage')` as
/// `NOT (states.group = 'triage')`, and `NOT NULL` is `NULL` — so issues
/// with *no* state are excluded too. The fragment below keeps the exact
/// `NOT (...)` form instead of the "equivalent" `IS NULL OR !=` rewrite,
/// which would wrongly admit stateless issues. The queried model is
/// `Issue.issue_objects` (soft-delete base, like [`MEMBER_PROJECTS_SQL`]),
/// hence `issues.deleted_at IS NULL`; joined tables carry no such
/// predicate.
pub const SCOPED_ISSUES_SQL: &str = "SELECT DISTINCT issues.* FROM issues \
     INNER JOIN projects ON projects.id = issues.project_id \
     INNER JOIN workspaces ON workspaces.id = issues.workspace_id \
     INNER JOIN project_members ON (project_members.project_id = projects.id \
     AND project_members.member_id = $1 AND project_members.is_active) \
     LEFT OUTER JOIN states ON states.id = issues.state_id \
     WHERE workspaces.slug = $2 \
     AND issues.deleted_at IS NULL \
     AND NOT (states.group = 'triage') \
     AND NOT (issues.archived_at IS NOT NULL) \
     AND NOT (projects.archived_at IS NOT NULL) \
     AND NOT (issues.is_draft)";

/// `user_issues_queryset` (`core/querysets.py:33-52`) inside the member
/// scope: `assigned` / `created` / `all` (assigned OR created OR
/// subscribed).
///
/// Soft-delete asymmetry, preserved: these are M2M traversals
/// (`assignees__id`, `issue_subscribers__subscriber_id`), whose through
/// joins carry NO `deleted_at` predicate — a soft-deleted link still
/// matches here. (The explicit through-model joins in
/// `work_item_list_filters` DO filter; see [`ListFilterPredicates`].)
/// Filtering here would hide issues Python still returns.
pub fn my_issues_scope_sql(scope: &str) -> &'static str {
    match scope {
        "assigned" => {
            "EXISTS (SELECT 1 FROM issue_assignees \
             WHERE issue_assignees.issue_id = issues.id \
             AND issue_assignees.assignee_id = $1)"
        }
        "created" => "issues.created_by_id = $1",
        // Unknown scopes fall back to `all` (`list_my_issues :141-142`).
        _ => {
            "(EXISTS (SELECT 1 FROM issue_assignees \
             WHERE issue_assignees.issue_id = issues.id \
             AND issue_assignees.assignee_id = $1) \
             OR issues.created_by_id = $1 \
             OR EXISTS (SELECT 1 FROM issue_subscribers \
             WHERE issue_subscribers.issue_id = issues.id \
             AND issue_subscribers.subscriber_id = $1))"
        }
    }
}

/// `require_project_write` (`_scoping.py:92-103` → `check_project_role`,
/// `core/permissions.py:73-110`): an active `ProjectMember` row with role
/// 20/15, OR any-role membership plus workspace-admin (role 20, exact —
/// `role=ROLE_ADMIN`, not `>=`).
/// Guests are blocked with [`ToolError::write_denied`].
/// `$1` is the user id, `$2` the workspace slug, `$3` the project id.
/// Python filters `workspace__slug`, i.e. a join to `workspaces` — the
/// fragment joins it explicitly so the caller binds the slug it already
/// has (binding a slug against the `workspace_id` UUID column would fail
/// at runtime). Both membership models use the soft-delete default
/// manager, hence the `deleted_at` predicates.
pub const PROJECT_WRITE_GATE_SQL: &str = "SELECT EXISTS(SELECT 1 FROM project_members \
     INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
     WHERE project_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND project_members.project_id = $3 \
     AND project_members.role IN (20, 15) \
     AND project_members.is_active \
     AND project_members.deleted_at IS NULL) \
     OR (EXISTS(SELECT 1 FROM project_members \
     INNER JOIN workspaces ON workspaces.id = project_members.workspace_id \
     WHERE project_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND project_members.project_id = $3 \
     AND project_members.is_active \
     AND project_members.deleted_at IS NULL) \
     AND EXISTS(SELECT 1 FROM workspace_members \
     INNER JOIN workspaces ON workspaces.id = workspace_members.workspace_id \
     WHERE workspace_members.member_id = $1 \
     AND workspaces.slug = $2 \
     AND workspace_members.role = 20 \
     AND workspace_members.is_active \
     AND workspace_members.deleted_at IS NULL))";

// ---------------------------------------------------------------------------
// list_issues filters (work_item_list_filters + issue_filters GET mapping)
// ---------------------------------------------------------------------------

/// `issue_filters.py:482` — the `parent` null spellings. `list_issues`
/// documents `"null"`; `work_item_list_filters` also accepts `"none"`.
pub const NULL_TOKENS: [&str; 2] = ["null", "none"];

/// `_split_tokens` (`issue_filters.py:489-493`): comma-split, stripped,
/// empties dropped. `None` (absent key) yields no tokens.
pub fn split_tokens(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => Vec::new(),
        Some(value) => value
            .split(',')
            .map(py_strip)
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// `uuid.UUID(str)` also accepts the braced form (`{...}`); neither
/// `Uuid::parse_str` nor the regex-free classifiers below do. Strip one
/// matched pair of braces first so braced UUIDs validate exactly as in
/// Python. (Non-ASCII digits are the reverse gap: Python `\d`/`isdigit`
/// accept them, the classifiers here are ASCII-only — same documented
/// limitation as [`search_sequence_tokens`].)
pub fn strip_uuid_braces(value: &str) -> &str {
    value
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(value)
}

/// `_is_uuid` (`issue_filters.py:495-500`): `uuid.UUID(str(value))`
/// accepting anything the constructor takes (hex with or without braces
/// and dashes).
pub fn is_uuid_token(value: &str) -> bool {
    Uuid::parse_str(strip_uuid_braces(value)).is_ok()
}

/// Validated `list_issues` filter parameters
/// (`work_item_list_filters`, `issue_filters.py:573-641`). Name lists that
/// need DB resolution (states, labels, parents) arrive here already
/// resolved to id strings by the caller; this function enforces the
/// enumerations and the null-combination rule with byte-exact errors.
pub struct ListFilterInput {
    pub state_ids: Vec<String>,
    pub state_groups: Vec<String>,
    pub priorities: Vec<String>,
    pub parent_tokens: Vec<String>,
    /// Label ids resolved from names/ids (`_resolve_labels`).
    pub label_ids: Vec<String>,
}

/// The GET-mapping output (`issue_filters(query, "GET")` +
/// `work_item_list_filters` `extra`): Django ORM kwargs rendered as SQL
/// predicates the handler ANDs onto [`SCOPED_ISSUES_SQL`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListFilterPredicates {
    /// `state__in` — UUID-only (`filter_valid_uuids` drops the rest;
    /// the agent layer already resolved names, `issue_filters.py:91-101`).
    pub state_in: Vec<String>,
    /// `state__group__in` (`filter_state_group`, `:104-113`).
    pub state_group_in: Vec<String>,
    /// `priority__in` (`filter_priority`, `:124-132`). Trailing-comma
    /// trap: `issue_filters` splits the joined string again, and a `""`
    /// element disables the whole key — the joined values here never
    /// contain empties, so the key always applies when non-empty.
    pub priority_in: Vec<String>,
    /// `parent=None` (top-level only) vs `parent__in` (`filter_parent`,
    /// `:135-147`). `parent=null` combined with real parents is rejected
    /// before this point.
    pub parent_is_null: bool,
    pub parent_in: Vec<String>,
    /// Through-model join (`work_item_list_filters :623-629`): a
    /// soft-deleted label link never matches.
    pub label_ids_in: Vec<String>,
}

/// `work_item_list_filters` validation half (`issue_filters.py:593-641`).
/// On success the caller feeds the ids into
/// [`ListFilterPredicates::to_sql`]; on failure the message is the exact
/// `IssueFilterError` text (a `ModelRetry` in the tool).
pub fn normalize_list_filters(input: &ListFilterInput) -> Result<ListFilterPredicates, ToolError> {
    let state_in: Vec<String> = input
        .state_ids
        .iter()
        .filter(|id| is_uuid_token(id))
        .cloned()
        .collect();
    let state_group_in = normalize_enum_list(
        &input.state_groups,
        &STATE_GROUP_ORDER,
        "state group",
        "groups",
    )?;
    let priority_in = normalize_enum_list(
        &input.priorities,
        &VALID_PRIORITIES,
        "priority value",
        "priorities",
    )?;
    let nulls = input
        .parent_tokens
        .iter()
        .filter(|token| NULL_TOKENS.contains(&token.to_lowercase().as_str()))
        .count();
    let (parent_is_null, parent_in) = if nulls > 0 {
        if nulls != input.parent_tokens.len() {
            // `issue_filters.py:618-619`, byte-exact.
            return Err(ToolError::Retry(
                "parent=null cannot be combined with specific parent issues.".to_owned(),
            ));
        }
        // `filter_parent` spells top-level-only as the literal `"None"`
        // (`issue_filters.py:620`), which the GET mapping turns into
        // `parent__isnull=True` (`:138-139`).
        (true, Vec::new())
    } else {
        (
            false,
            input
                .parent_tokens
                .iter()
                .filter(|id| is_uuid_token(id))
                .cloned()
                .collect(),
        )
    };
    Ok(ListFilterPredicates {
        state_in,
        state_group_in,
        priority_in,
        parent_is_null,
        parent_in,
        label_ids_in: input.label_ids.clone(),
    })
}

/// Lowercases each token and rejects values outside `valid` with the exact
/// `Unknown <thing>(s): ...` error shape (`issue_filters.py:597-613`).
fn normalize_enum_list(
    tokens: &[String],
    valid: &[&str],
    thing: &str,
    plural: &str,
) -> Result<Vec<String>, ToolError> {
    let lowered: Vec<String> = tokens.iter().map(|token| token.to_lowercase()).collect();
    if lowered.is_empty() {
        return Ok(Vec::new());
    }
    let invalid: Vec<&str> = lowered
        .iter()
        .filter(|token| !valid.contains(&token.as_str()))
        .map(String::as_str)
        .collect();
    if !invalid.is_empty() {
        return Err(ToolError::Retry(format!(
            "Unknown {thing}(s): {}. Valid {plural}: {}.",
            invalid.join(", "),
            valid.join(", ")
        )));
    }
    Ok(lowered)
}

/// Formats the "Valid states/labels" tail of the unknown-name errors
/// (`_resolve_states :503-520`, `_resolve_labels :522-543`). Ported
/// difference, documented: Python sorts a *set* with `key=str.lower`,
/// whose tie order follows hash seed and is nondeterministic across
/// processes; the port sorts by `(lowercased, original)` so the message
/// is stable. Same name set, deterministic order.
pub fn valid_name_list(names: &[&str]) -> String {
    let mut sorted: Vec<&str> = names.to_vec();
    sorted.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    let joined = sorted.join(", ");
    if joined.is_empty() {
        "(none)".to_owned()
    } else {
        joined
    }
}

impl ListFilterPredicates {
    /// Renders the predicates as one SQL `AND` chain over the `issues`
    /// alias used by [`SCOPED_ISSUES_SQL`]. `$N` placeholders continue the
    /// caller's numbering (`$1`/`$2` are taken there); values are pushed in
    /// order into `params`.
    pub fn to_sql(&self, params: &mut Vec<String>, first_param: i32) -> String {
        let mut next = first_param;
        let mut parts: Vec<String> = Vec::new();
        let mut take = |count: usize| {
            let placeholders: Vec<String> = (0..count)
                .map(|_| {
                    let p = format!("${next}");
                    next += 1;
                    p
                })
                .collect();
            placeholders.join(", ")
        };
        if !self.state_in.is_empty() {
            let list = take(self.state_in.len());
            params.extend(self.state_in.iter().cloned());
            parts.push(format!("issues.state_id IN ({list})"));
        }
        if !self.state_group_in.is_empty() {
            let list = take(self.state_group_in.len());
            params.extend(self.state_group_in.iter().cloned());
            parts.push(format!(
                "issues.state_id IN (SELECT states.id FROM states WHERE states.group IN ({list}))"
            ));
        }
        if !self.priority_in.is_empty() {
            let list = take(self.priority_in.len());
            params.extend(self.priority_in.iter().cloned());
            parts.push(format!("issues.priority IN ({list})"));
        }
        if self.parent_is_null {
            parts.push("issues.parent_id IS NULL".to_owned());
        } else if !self.parent_in.is_empty() {
            let list = take(self.parent_in.len());
            params.extend(self.parent_in.iter().cloned());
            parts.push(format!("issues.parent_id IN ({list})"));
        }
        if !self.label_ids_in.is_empty() {
            let list = take(self.label_ids_in.len());
            params.extend(self.label_ids_in.iter().cloned());
            parts.push(format!(
                "EXISTS (SELECT 1 FROM issue_labels WHERE issue_labels.issue_id = issues.id \
                 AND issue_labels.label_id IN ({list}) \
                 AND issue_labels.deleted_at IS NULL)"
            ));
        }
        parts.join(" AND ")
    }
}

// ---------------------------------------------------------------------------
// Full-text search (issue_search_queryset, tool path)
// ---------------------------------------------------------------------------

/// The tool calls `issue_search_queryset(qs, query)` with defaults
/// (`with_rank=false, with_headline=false, include_comments=false`), so
/// only `_build_search_filter` (`search/issue.py:92-126`) matters:
/// FTS over name + description, `name__icontains` substring fallback,
/// the guarded `sequence_id` int branch, and the project-identifier
/// branch. Comment-text matches never surface on this path (known
/// limitation, `search/issue.py:77-82` — the contract is preserved, not
/// fixed).
///
/// Bindings: `$1` is the raw query (FTS branch), `$2` the int array from
/// [`search_sequence_tokens`], `$3` the [`escape_icontains`] output.
/// `$1` and `$3` must stay separate: Django escapes LIKE metacharacters
/// only in the `icontains` branches, never in the FTS query.
pub const ISSUE_FTS_SQL: &str = "(to_tsvector('english'::regconfig, \
     COALESCE(issues.name, '') || ' ' || COALESCE(issues.description_stripped, '')) \
     @@ websearch_to_tsquery('english'::regconfig, $1) \
     OR issues.name ILIKE '%' || $3 || '%' ESCAPE '\\' \
     OR issues.sequence_id IN (SELECT * FROM UNNEST($2::int[])) \
     OR EXISTS (SELECT 1 FROM projects \
     WHERE projects.id = issues.project_id \
     AND projects.identifier ILIKE '%' || $3 || '%' ESCAPE '\\'))";

/// Django `icontains` escapes LIKE metacharacters (`\`, `%`, `_`) with a
/// backslash, so a query for `100%` matches literally instead of acting
/// as a wildcard (`db/models/lookups.py`, `PatternLookup`). The `ILIKE`
/// branches of [`ISSUE_FTS_SQL`] pin the same escape character
/// explicitly; the handler binds their parameter through this helper.
pub fn escape_icontains(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for ch in pattern.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Numeric branch of `_build_search_filter` (`search/issue.py:114-123`):
/// only when the whole query is at most 20 chars; per `\b\d+\b` token at
/// most 10 digits (longer runs are skipped before parsing) and value at
/// most int4 max (a bigger paste must not 500 the endpoint).
pub fn search_sequence_tokens(query: &str) -> Vec<i64> {
    // `len(query)` counts code points (`search/issue.py:114`), not bytes.
    if query.chars().count() > 20 {
        return Vec::new();
    }
    let chars: Vec<char> = query.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        // `\b\d+\b`: ASCII digit runs only. Python `\d` also matches
        // non-ASCII decimal digits, but sequence ids are ASCII ints and
        // this branch exists for pasted logs/stack traces — the residual
        // gap is documented, not silently widened.
        if chars[index].is_ascii_digit() && (index == 0 || !is_word_char(chars[index - 1])) {
            let mut end = index;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            if end == chars.len() || !is_word_char(chars[end]) {
                let token: String = chars[index..end].iter().collect();
                if token.len() <= 10 {
                    if let Ok(value) = token.parse::<i64>() {
                        if value <= SEQUENCE_ID_MAX {
                            tokens.push(value);
                        }
                    }
                }
            }
            index = end;
        } else {
            index += 1;
        }
    }
    tokens
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// `search_issues :69` — whitespace-only queries skip the FTS filter
/// entirely (the queryset is returned unchanged); ordering stays
/// `-updated_at` on every path.
pub fn search_uses_fts(query: &str) -> bool {
    !py_strip(query).is_empty()
}

// ---------------------------------------------------------------------------
// Relations (relate_issues / unrelate_issues / list_issue_relations)
// ---------------------------------------------------------------------------

/// `validate_relation_type` (`relations.py:81-86`): lowercased, stripped;
/// the message joins [`RELATION_TYPES`] in tuple order. Byte-exact.
pub fn validate_relation_type(relation_type: &str) -> Result<String, ToolError> {
    let value = py_strip(relation_type).to_lowercase();
    if RELATION_TYPES.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(ToolError::Retry(format!(
            "relation_type must be one of: {}",
            RELATION_TYPES.join(", ")
        )))
    }
}

/// `get_actual_relation` (`issue_relation_mapper.py`): the stored row for
/// a forward-named request. `relates_to`/`duplicate` (symmetric) store as
/// themselves; reverse types fold onto their forward twin. Unknown input
/// maps to itself (`.get(k, k)` — unreachable after validation, but old
/// rows flow through [`type_from_viewpoint`]).
pub fn actual_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocking" => "blocked_by",
        "implements" => "implemented_by",
        other => other,
    }
}

/// `get_inverse_relation` (`issue_relation_mapper.py`): unknown types map
/// to themselves (`.get(k, k)`).
pub fn inverse_relation(relation_type: &str) -> &str {
    match relation_type {
        "start_after" => "start_before",
        "finish_after" => "finish_before",
        "blocked_by" => "blocking",
        "blocking" => "blocked_by",
        "start_before" => "start_after",
        "finish_before" => "finish_after",
        "implemented_by" => "implements",
        "implements" => "implemented_by",
        other => other,
    }
}

/// `_stored_edge` (`relations.py:93-99`): the `(issue_id,
/// related_issue_id, stored_type)` triple for "source <type> target".
/// Returns `(swap_ends, stored_type)`.
pub fn stored_edge(relation_type: &str) -> (bool, &str) {
    (
        is_reverse_relation(relation_type),
        actual_relation(relation_type),
    )
}

/// `_check_targets` (`relations.py:170-183`): self-relations and
/// cross-workspace targets retry; duplicates collapse. Identifiers in the
/// messages are the relations-module form ([`relation_identifier`]).
pub fn check_relation_targets(
    source: &RelationEndpoint,
    targets: &[RelationEndpoint],
) -> Result<Vec<RelationEndpoint>, ToolError> {
    let mut unique: Vec<RelationEndpoint> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for target in targets {
        if target.id == source.id {
            return Err(ToolError::Retry(format!(
                "{} cannot be related to itself",
                source.display
            )));
        }
        if target.workspace_id != source.workspace_id {
            return Err(ToolError::Retry(format!(
                "{} is in a different workspace",
                target.display
            )));
        }
        if !seen.iter().any(|id| id == &target.id) {
            seen.push(target.id.clone());
            unique.push(target.clone());
        }
    }
    Ok(unique)
}

/// One resolved endpoint of a relation write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationEndpoint {
    pub id: String,
    pub workspace_id: String,
    /// Relations-module identifier (`PROJ-123`, no fallback).
    pub display: String,
}

/// Reference kind from `resolve_refs` (`relations.py:138-168`): a UUID
/// parses as an id lookup; otherwise `PROJ-123` splits at the last `-`
/// with an all-digit tail (`seq.isdigit()` — empty tails and missing
/// separators do not match). Anything else is unresolved, never an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueRef {
    Id(String),
    Identifier {
        project_code: String,
        sequence_id: i64,
    },
    Unresolved,
}

pub fn classify_issue_ref(raw: &str) -> IssueRef {
    let reference = py_strip(raw);
    if reference.is_empty() {
        return IssueRef::Unresolved;
    }
    // Brace-stripping applies to the UUID attempt only: Python tries
    // `uuid.UUID(ref)` first and falls back to the identifier split on the
    // *original* string, so `{PROJ-12}` stays unresolved (the tail `12}`
    // is not a digit run).
    if Uuid::parse_str(strip_uuid_braces(reference)).is_ok() {
        return IssueRef::Id(strip_uuid_braces(reference).to_owned());
    }
    match reference.rsplit_once('-') {
        Some((code, tail))
            if !code.is_empty()
                && !tail.is_empty()
                && tail.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            match tail.parse::<i64>() {
                Ok(sequence_id) => IssueRef::Identifier {
                    project_code: code.to_owned(),
                    sequence_id,
                },
                Err(_) => IssueRef::Unresolved,
            }
        }
        _ => IssueRef::Unresolved,
    }
}

/// `_resolve_issue_refs` miss message (`issues.py:402-408`): every
/// reference the scoped lookup could not resolve, in request order.
pub fn unresolved_refs_message(unresolved: &[&str]) -> String {
    format!(
        "Issues not found or not accessible: {}",
        unresolved.join(", ")
    )
}

/// SQL for the identifier half of `resolve_refs`: project code compares
/// case-insensitively (`project__identifier__iexact` → `UPPER()`, mirroring
/// the model's always-upper normalization in `project.py:201-210`).
/// The lookup runs on `Issue.issue_objects`, so soft-deleted rows are
/// invisible (`db/mixins.py:56-58`).
pub const RESOLVE_IDENTIFIER_SQL: &str =
    "SELECT issues.id FROM issues INNER JOIN projects ON projects.id = issues.project_id \
     WHERE UPPER(projects.identifier) = UPPER($1) AND issues.sequence_id = $2 \
     AND issues.deleted_at IS NULL LIMIT 1";

/// `_pair_rows` (`relations.py:126-130`) over live rows (the default
/// soft-deletion manager excludes `deleted_at` rows, `mixins.py:56-58`).
pub const PAIR_ROWS_SQL: &str = "SELECT id, issue_id, related_issue_id, relation_type \
     FROM issue_relations \
     WHERE ((issue_id = $1 AND related_issue_id = $2) \
     OR (issue_id = $2 AND related_issue_id = $1)) \
     AND deleted_at IS NULL";

/// Live-row triple for the viewpoint functions below.
pub struct RelationRow<'a> {
    pub issue_id: &'a str,
    pub related_issue_id: &'a str,
    pub stored_type: &'a str,
}

/// `_type_from` (`relations.py:102-116`): the row's type named from the
/// viewpoint's side. Rows stored under a reverse name (which the UI never
/// writes but older data may hold) are normalized exactly like
/// `orchestration.blockers` does.
pub fn type_from_viewpoint<'a>(row: &RelationRow<'a>, viewpoint_id: &str) -> &'a str {
    // Reverse-stored rows are normalized first (ends swapped, type folded
    // to its forward twin); anything else passes through untouched —
    // including unknown junk from old rows, exactly as Python does.
    let (stored, issue_id) = if is_reverse_relation(row.stored_type) {
        // `actual_relation` returns a literal for every reverse type, but
        // the borrow checker cannot see that; re-derive through the same
        // mapping on an owned copy is unnecessary — reverse inputs are
        // exactly the four known names, so match them explicitly.
        let forward = match row.stored_type {
            "blocking" => "blocked_by",
            "start_after" => "start_before",
            "finish_after" => "finish_before",
            "implements" => "implemented_by",
            other => other,
        };
        (forward, row.related_issue_id)
    } else {
        (row.stored_type, row.issue_id)
    };
    if issue_id == viewpoint_id {
        stored
    } else {
        // `inverse_relation` returns a literal for every known stored
        // type; unknown junk maps to itself. Same explicit match so the
        // return borrow is always the row's (or a literal with an
        // unbounded lifetime, which coerces).
        match stored {
            "blocked_by" => "blocking",
            "start_before" => "start_after",
            "finish_before" => "finish_after",
            "implemented_by" => "implements",
            "start_after" => "start_before",
            "finish_after" => "finish_before",
            "blocking" => "blocked_by",
            "implements" => "implemented_by",
            other => other,
        }
    }
}

/// `relate` result shape (`relations.py:180-233`): `{issue,
/// relation_type, created, unchanged, conflicts}`. `created`/`unchanged`
/// are identifier lists; `conflicts` carries the existing relation named
/// from the source's side (`sorted(current)[0]` — one row per pair by the
/// table constraint, so the single current type).
pub fn relate_result(
    source_display: &str,
    relation_type: &str,
    created: &[String],
    unchanged: &[String],
    conflicts: &[(String, String)],
) -> Value {
    json!({
        "issue": source_display,
        "relation_type": relation_type,
        "created": created,
        "unchanged": unchanged,
        "conflicts": conflicts
            .iter()
            .map(|(identifier, existing)| {
                json!({"identifier": identifier, "existing_relation": existing})
            })
            .collect::<Vec<_>>(),
    })
}

/// `unrelate` result shape (`relations.py:235-278`): only rows of exactly
/// the requested type are removed; anything else lands in `not_related`.
pub fn unrelate_result(
    source_display: &str,
    relation_type: &str,
    removed: &[String],
    not_related: &[String],
) -> Value {
    json!({
        "issue": source_display,
        "relation_type": relation_type,
        "removed": removed,
        "not_related": not_related,
    })
}

/// `grouped_relations` query (`relations.py:280-316`): every live row
/// touching the issue in its workspace, self-pairs excluded; the other
/// ends are then narrowed to the viewer's visible set and sorted by
/// `(project.identifier, sequence_id)`, capped at [`GROUP_LIMIT`] per
/// type. Every type key is always present in the output mapping.
pub const GROUPED_RELATIONS_SQL: &str =
    "SELECT id, issue_id, related_issue_id, relation_type FROM issue_relations \
     WHERE (issue_id = $1 OR related_issue_id = $1) \
     AND workspace_id = $2 \
     AND NOT (issue_id = $1 AND related_issue_id = $1) \
     AND deleted_at IS NULL";

/// Sort key for grouped items (`relations.py:308-309`).
pub fn relation_sort_key(project_identifier: &str, sequence_id: i64) -> (String, i64) {
    (project_identifier.to_owned(), sequence_id)
}

/// One grouped item: `_item` (`relations.py:269-277`) with the name
/// truncation + untrusted wrapper applied by `_relations_view`
/// (`issues.py:411-419`). Unknown stored types never reach this builder:
/// `grouped_relations` drops rows whose viewpoint type is outside
/// [`RELATION_TYPES`] (`relations.py:295-296`).
pub fn relation_item(
    id: &str,
    identifier: &str,
    name: &str,
    state_name: Option<&str>,
    state_group: Option<&str>,
) -> Value {
    let (name, _) = truncate(name, NAME_CAP);
    json!({
        "id": id,
        "identifier": identifier,
        "name": wrap_untrusted(&name),
        "state": state_name,
        "state_group": state_group,
    })
}

/// Post-commit activity tasks fired by the relation writes
/// (`relations.py:219-225,251-260`): `relate` logs per created target,
/// `unrelate` logs per removed target. Celery dispatches, not audit rows —
/// the handler fires them after the write transaction commits (same
/// post-commit wrapper as [`RECORD_MESSAGE_INSERT_SQL`]), or the activity
/// feed silently loses these entries.
pub const RELATION_CREATED_ACTIVITY: &str = "issue_relation.activity.created";
/// See [`RELATION_CREATED_ACTIVITY`].
pub const RELATION_DELETED_ACTIVITY: &str = "issue_relation.activity.deleted";

/// `_relation_write` wrapper (`issues.py:422-445`): exactly one source must
/// resolve; write access to its project is required; an empty target list
/// retries (`related_issues must list at least one issue.`); relation
/// errors surface as `ModelRetry`. A write summary is recorded only when
/// `created`/`removed` is non-empty.
pub const EMPTY_RELATED_MESSAGE: &str = "related_issues must list at least one issue.";

pub fn relation_write_summary(
    verb: &str,
    source_display: &str,
    relation_type: &str,
    changed: &[String],
) -> Option<String> {
    if changed.is_empty() {
        None
    } else {
        Some(format!(
            "{verb} {source_display} {relation_type} {}",
            changed.join(", ")
        ))
    }
}

// ---------------------------------------------------------------------------
// get_issue detail + record_write (audit rows)
// ---------------------------------------------------------------------------

/// `get_issue` comments (`issues.py:161-173`): the author's last 10
/// comments (`-created_at`, `select_related("actor")`), each body wrapped
/// and truncated at 500 code points, timestamps as `isoformat()`; the
/// tool then reverses the list so the wire order is chronological
/// (`recent_comments: list(reversed(comments))`, `:179`).
pub fn detail_comment_row(
    author: &str,
    body: &str,
    body_truncated: bool,
    created_at_iso: &str,
) -> Value {
    json!({
        "author": author,
        "body": wrap_untrusted(body),
        "body_truncated": body_truncated,
        "created_at": created_at_iso,
    })
}

/// Composed `get_issue` shape (`issues.py:174-182`): the brief row plus
/// the wrapped description and the chronological comments. The caller
/// supplies comments oldest-first (it reverses the `-created_at` fetch).
pub fn detail_shape(
    brief: Value,
    description: &str,
    description_truncated: bool,
    recent_comments: Vec<Value>,
) -> Value {
    let mut data = brief;
    if let Value::Object(ref mut map) = data {
        map.insert("description".to_owned(), wrap_untrusted(description).into());
        map.insert(
            "description_truncated".to_owned(),
            description_truncated.into(),
        );
        map.insert("recent_comments".to_owned(), recent_comments.into());
    }
    data
}

/// Comment fetch for `get_issue`, over live rows.
pub const ISSUE_COMMENTS_SQL: &str =
    "SELECT issue_comments.comment_stripped, issue_comments.speaker_type, \
     issue_comments.created_at, issue_comments.actor_id, \
     users.display_name, users.email \
     FROM issue_comments LEFT OUTER JOIN users ON users.id = issue_comments.actor_id \
     WHERE issue_comments.issue_id = $1 AND issue_comments.deleted_at IS NULL \
     ORDER BY issue_comments.created_at DESC LIMIT 10";

/// `record_write` (`_results.py:49-69`): persists the human-facing
/// transcript row (tool-result message with links) plus the `tool_result`
/// event carrying the message envelope, so write actions are always
/// visible in the chat. Message `seq` is `(MAX(seq) or 0) + 1` over the
/// thread (**1** when empty — `_next_message_seq`, `events.py:73-80`;
/// `None or 0` then `+ 1`); events allocate identically.
/// Per the Porting guide transactions row, the handler runs this behind
/// the post-commit wrapper — the write transaction commits first, then
/// the message/event inserts and (for `update_issue` dispatches) the
/// orchestration call fire.
///
/// Write summaries below are the exact `record_write` strings, including
/// the em dash (U+2014) in the create summary.
pub fn created_summary(identifier: &str, name: &str) -> String {
    format!("Created issue {identifier} \u{2014} {name}")
}

/// `issues.py:383-387` — `changed` in [`plan_update`] order.
pub fn updated_summary(identifier: &str, changed: &[&str]) -> String {
    format!("Updated issue {identifier} ({})", changed.join(", "))
}

pub const RECORD_MESSAGE_SQL: &str =
    "SELECT assistant_thread.id FROM assistant_thread WHERE assistant_thread.id = $1 FOR UPDATE";

pub const RECORD_MESSAGE_INSERT_SQL: &str =
    "INSERT INTO assistant_message (id, thread_id, turn_id, seq, kind, display_content, payload, status) \
     VALUES ($1, $2, $3, \
     (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_message WHERE thread_id = $2), \
     'tool_result', $4, $5, 'completed') RETURNING id, seq, created_at";

/// `AssistantEvent.id` is a `BigAutoField` (`models.py:154`) — the insert
/// omits it, unlike the message insert (UUID PK with an ORM-side default).
pub const RECORD_EVENT_INSERT_SQL: &str =
    "INSERT INTO assistant_event (thread_id, turn_id, seq, kind, message_id, payload) \
     VALUES ($1, $2, \
     (SELECT COALESCE(MAX(seq), 0) + 1 FROM assistant_event WHERE thread_id = $1), \
     'tool_result', $3, $4) RETURNING id, seq, created_at";

/// `message_envelope` (`events.py:128-140`): `kind → role`,
/// `display_content → content`. Nine parameters mirror the nine envelope
/// keys one-to-one (see the `message_envelope` fixture vector), so they
/// stay positional rather than hidden behind a bag struct.
#[allow(clippy::too_many_arguments)]
pub fn message_envelope(
    id: &str,
    kind: &str,
    content: &str,
    status: &str,
    seq: i64,
    turn_id: Option<&str>,
    payload: &Value,
    created_at_iso: &str,
    completed_at_iso: Option<&str>,
) -> Value {
    json!({
        "id": id,
        "role": kind,
        "content": content,
        "status": status,
        "seq": seq,
        "turn_id": turn_id,
        "payload": payload,
        "created_at": created_at_iso,
        "completed_at": completed_at_iso,
    })
}

/// `create_issue` write transaction (`issues.py:256-277`): impersonation
/// makes `BaseModel.save()` attribute `created_by` to the acting user
/// (the tool runs with no request, so crum's current user is `None`);
/// the project row is locked, the sequence allocated, and the issue
/// inserted atomically. `created_via` comes from the deps mode
/// (`assistant` for chat, `loop` otherwise).
pub const CREATE_ISSUE_SQL: &str = "SELECT id FROM projects WHERE id = $1 FOR UPDATE";

/// `Issue.objects` is the soft-delete default manager, so the maximum is
/// taken over live rows only — a sequence held by a soft-deleted row can
/// be reallocated, exactly as in Python (ported behavior, not fixed).
pub const CREATE_ISSUE_MAX_SEQ_SQL: &str =
    "SELECT COALESCE(MAX(sequence_id), 0) FROM issues WHERE project_id = $1 AND deleted_at IS NULL";

pub const CREATE_ISSUE_INSERT_SQL: &str =
    "INSERT INTO issues (id, name, description_html, description_json, priority, \
     sequence_id, state_id, project_id, workspace_id, parent_id, \
     created_by_id, updated_by_id, created_via) \
     VALUES ($1, $2, $3, '{}', $4, $5, $6, $7, $8, $9, $10, $10, $11) \
     RETURNING id";

/// `update_issue` write transaction (`issues.py:334-381`): the row is
/// re-fetched under `SELECT ... FOR UPDATE` so concurrent UI edits are
/// not lost; only [`plan_update`] columns are written back; the
/// orchestration immediate flag is suppressed on the save so one move
/// makes one run, then [`should_dispatch_transition`] decides the
/// explicit dispatch with `actor` + `dispatch_immediate = true`.
pub const UPDATE_ISSUE_LOCK_SQL: &str = "SELECT * FROM issues WHERE id = $1 FOR UPDATE";

// ---------------------------------------------------------------------------
// Tool schemas
// ---------------------------------------------------------------------------

/// One tool registration: the Python function name, its docstring
/// (verbatim, first line used as the model-facing summary by the agent
/// runtime), and its JSON Schema parameters.
pub struct ToolSchema {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

fn string_param(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn optional_string_param(description: &str) -> Value {
    json!({"type": ["string", "null"], "description": description})
}

fn int_param(description: &str, default: i64) -> Value {
    json!({"type": "integer", "description": description, "default": default})
}

/// JSON Schemas for the nine issue tools, built by hand from the Python
/// signatures (`issues.py:57-490`) — same parameter names, same defaults,
/// same required sets. (`schemars` derives would generate these once the
/// handler layer owns the transport; the documents below are what they
/// must equal.)
pub fn tool_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "search_issues",
            description: "Full-text search issues you can access. Returns up to 20 results per page.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": string_param("Full-text query over issue names and descriptions."),
                    "project_id": optional_string_param("Narrow the search to one project (scope-checked)."),
                    "limit": int_param("Page size, clamped to 1..20.", 20),
                    "offset": int_param("Zero-based offset; advance by limit while has_more.", 0),
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "list_issues",
            description: "List issues in one project, newest activity first, up to 20 per page. Optional filters, each comma-separated (values OR together, filters AND together): state (names or ids), state_group (backlog, unstarted, started, review, test, completed, cancelled), parent_issue_id (issue id or PROJ-123 identifier, or \"null\" for top-level issues only), labels (names or ids), priority (urgent, high, medium, low, none).",
            parameters: json!({
                "type": "object",
                "properties": {
                    "project_id": string_param("Project to list (scope-checked)."),
                    "state": optional_string_param("State names or ids, comma-separated."),
                    "state_group": optional_string_param("State groups, comma-separated."),
                    "parent_issue_id": optional_string_param("Parent issue id/identifier, or \"null\" for top-level only."),
                    "labels": optional_string_param("Label names or ids, comma-separated."),
                    "priority": optional_string_param("Priorities, comma-separated."),
                    "limit": int_param("Page size, clamped to 1..20.", 20),
                    "offset": int_param("Zero-based offset; advance by limit while has_more.", 0),
                },
                "required": ["project_id"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "list_my_issues",
            description: "List issues you're involved in. scope: 'all', 'assigned', or 'created'.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "description": "One of 'all', 'assigned', 'created' (anything else behaves as 'all').",
                        "default": "all",
                    },
                    "limit": int_param("Page size, clamped to 1..20.", 20),
                    "offset": int_param("Zero-based offset; advance by limit while has_more.", 0),
                },
                "required": [],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "get_issue",
            description: "Get one issue in detail, including its most recent comments.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "issue_id": string_param("Issue UUID (scope-checked)."),
                },
                "required": ["issue_id"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "create_issue",
            description: "Create an issue. Requires Member or Admin role. Pass parent_issue_id to link it as a sub-issue of another issue in the same project. (Assignees/labels: set them afterwards in the UI — not yet supported by this tool.)",
            parameters: json!({
                "type": "object",
                "properties": {
                    "project_id": string_param("Project to create in (scope-checked, write-gated)."),
                    "name": string_param("Issue name (required, stripped, cut to 255 chars)."),
                    "description_md": optional_string_param("Markdown body, rendered to sanitized HTML."),
                    "state_id": optional_string_param("State id; defaults to the project's default state."),
                    "priority": optional_string_param("One of urgent, high, medium, low, none (default none)."),
                    "parent_issue_id": optional_string_param("Parent issue in the same project (cycle-checked)."),
                },
                "required": ["project_id", "name"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "update_issue",
            description: "Update an issue's name, description, state, or priority. Requires write access. Changing the state may dispatch a coding run if it moves the issue into a delegated/ticking state (same as moving it in the UI). Pass parent_issue_id to re-parent the issue (must be another issue in the same project); pass null to unlink it from its current parent; omit it to leave the parent unchanged.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "issue_id": string_param("Issue UUID (scope-checked, write-gated)."),
                    "name": optional_string_param("New name (blank values are ignored)."),
                    "description_md": optional_string_param("New markdown body (null/omitted leaves it unchanged)."),
                    "state_id": optional_string_param("New state id (a real change may dispatch a run)."),
                    "priority": optional_string_param("One of urgent, high, medium, low, none."),
                    "parent_issue_id": {
                        "type": ["string", "null"],
                        "description": "Re-parent target; null unlinks; omission (sentinel __unset__) leaves it unchanged.",
                        "default": "__unset__",
                    },
                },
                "required": ["issue_id"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "relate_issues",
            description: "Record a relation from ``issue`` to each of ``related_issues`` (identifiers like PROJ-12, or UUIDs). relation_type is read from ``issue``'s side: one of blocked_by, blocking, relates_to, duplicate, start_before, start_after, finish_before, finish_after, implemented_by, implements. Use blocked_by when ``issue`` cannot finish until the related issues do. Idempotent: pairs that already have this relation come back under ``unchanged``; pairs that already have a different relation come back under ``conflicts`` and are not changed. Requires write access to ``issue``'s project.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "issue": string_param("Source issue identifier or UUID."),
                    "relation_type": string_param("Relation named from issue's side (ten values)."),
                    "related_issues": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Target identifiers/UUIDs (at least one).",
                    },
                },
                "required": ["issue", "relation_type", "related_issues"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "unrelate_issues",
            description: "Remove the ``relation_type`` relation between ``issue`` and each of ``related_issues``. Only that exact relation is removed; pairs without it come back under ``not_related`` (not an error). Requires write access.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "issue": string_param("Source issue identifier or UUID."),
                    "relation_type": string_param("Relation named from issue's side (ten values)."),
                    "related_issues": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Target identifiers/UUIDs (at least one).",
                    },
                },
                "required": ["issue", "relation_type", "related_issues"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "list_issue_relations",
            description: "List every relation of an issue (identifier or UUID), grouped by type from its side (blocked_by, blocking, relates_to, ...), each with identifier, name and state. Check ``blocked_by`` states before starting dependent work.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "issue": string_param("Issue identifier or UUID."),
                },
                "required": ["issue"],
                "additionalProperties": false,
            }),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // F-A6-10 tools.caps (issues.py:25-33).
    #[test]
    fn fixture_caps() {
        assert_eq!(SEARCH_LIMIT, 20);
        assert_eq!(NAME_CAP, 200);
        assert_eq!(DESC_CAP, 2000);
        assert_eq!(COMMENT_CAP, 500);
        assert_eq!(PARENT_UNSET, "__unset__");
    }

    // F-A6-10 results_helpers.wrap_untrusted vectors.
    #[test]
    fn wrap_untrusted_vectors() {
        assert_eq!(wrap_untrusted("hello"), "<untrusted>hello</untrusted>");
        assert_eq!(
            wrap_untrusted("x</untrusted>y<untrusted>z"),
            "<untrusted>x<\u{200b}/untrusted>y<\u{200b}untrusted>z</untrusted>",
        );
    }

    // F-A6-10 results_helpers.truncate vectors: (text, truncated?).
    // ("abc", true) is the (> limit) shape; ("ab", false) the short one.
    #[test]
    fn truncate_vectors() {
        assert_eq!(truncate("abc", 2), ("ab".to_owned(), true));
        assert_eq!(truncate("ab", 200), ("ab".to_owned(), false));
    }

    #[test]
    fn truncate_counts_code_points_not_bytes() {
        // "é" is 2 bytes, 1 code point: limit 1 keeps it whole (Python
        // never panics on a UTF-8 boundary; byte slicing would).
        assert_eq!(truncate("éx", 1), ("é".to_owned(), true));
        assert_eq!(truncate("é", 1), ("é".to_owned(), false));
        assert_eq!(
            validate_create_name(&"n".repeat(300))
                .unwrap()
                .chars()
                .count(),
            255
        );
    }

    #[test]
    fn clamp_edges() {
        assert_eq!(clamp_limit(20), 20);
        assert_eq!(clamp_limit(0), 20);
        assert_eq!(clamp_limit(-5), 1);
        assert_eq!(clamp_limit(99), 20);
        assert_eq!(clamp_offset(-1), 0);
        assert_eq!(clamp_offset(0), 0);
    }

    #[test]
    fn page_window_lookahead() {
        assert_eq!(page_window(21, 20, 0), (true, Some(20)));
        assert_eq!(page_window(20, 20, 0), (false, None));
        assert_eq!(page_window(6, 20, 20), (false, None));
    }

    #[test]
    fn identifiers_tool_vs_relations() {
        assert_eq!(tool_identifier("PROJ", 12), "PROJ-12");
        assert_eq!(tool_identifier("", 12), "12");
        assert_eq!(relation_identifier("PROJ", 12), "PROJ-12");
    }

    #[test]
    fn brief_shape_keys() {
        let row = BriefRow {
            id: "11111111-1111-1111-1111-111111111111",
            project_id: "22222222-2222-2222-2222-222222222222",
            project_identifier: "PROJ",
            sequence_id: 7,
            name: "Fix it",
            parent_id: None,
            state_name: Some("Backlog"),
            state_group: Some("backlog"),
            priority: "high",
        };
        let value = brief_row(&row);
        assert_eq!(value["identifier"], "PROJ-7");
        assert_eq!(value["name"], "<untrusted>Fix it</untrusted>");
        assert_eq!(value["name_truncated"], false);
        assert_eq!(value["parent_id"], Value::Null);
        assert_eq!(value["state"], "Backlog");
    }

    #[test]
    fn comment_author_matrix() {
        assert_eq!(
            comment_author("agent", Some("X"), Some("x@y"), true),
            "Pi Dash AI"
        );
        assert_eq!(
            comment_author("user", Some("Ann"), Some("a@b"), true),
            "Ann"
        );
        assert_eq!(comment_author("user", None, Some("a@b"), true), "a@b");
        assert_eq!(comment_author("user", Some(""), Some("a@b"), true), "a@b");
        assert_eq!(
            comment_author("user", Some("Ann"), Some("a@b"), false),
            "Unknown"
        );
    }

    #[test]
    fn priority_message_is_byte_exact() {
        assert_eq!(
            PRIORITY_CHOICES_MESSAGE,
            "Priority must be one of ['high', 'low', 'medium', 'none', 'urgent']."
        );
        assert_eq!(validate_priority(None).unwrap(), "none");
        // `(priority or "none")`: empty strings take the default too.
        assert_eq!(validate_priority(Some("")).unwrap(), "none");
        assert_eq!(validate_priority(Some("HIGH")).unwrap(), "high");
        // ...but the update path has no such mapping.
        assert_eq!(validate_update_priority(None).unwrap(), None);
        assert!(validate_update_priority(Some("")).is_err());
        assert!(validate_priority(Some("critical")).is_err());
        assert_eq!(
            validate_priority(Some("critical")).unwrap_err().message(),
            PRIORITY_CHOICES_MESSAGE
        );
    }

    #[test]
    fn create_name_rules() {
        assert!(validate_create_name("   ").is_err());
        assert_eq!(validate_create_name("  spaced  ").unwrap(), "spaced");
    }

    #[test]
    fn update_plan_order_and_audit_columns() {
        let input = UpdateInput {
            name: Some("N"),
            description_md: Some("D"),
            priority: Some("high"),
            state_given: true,
            parent: ParentUpdate::Unlink,
        };
        let (changed, fields) = plan_update(&input).unwrap();
        assert_eq!(
            changed,
            vec!["name", "description", "priority", "state", "parent"]
        );
        assert_eq!(
            fields,
            vec![
                "name",
                "description_html",
                "description_json",
                "priority",
                "state",
                "parent",
                "updated_at",
                "updated_by"
            ]
        );
    }

    #[test]
    fn update_nothing_message_is_byte_exact() {
        let input = UpdateInput {
            name: None,
            description_md: None,
            priority: None,
            state_given: false,
            parent: ParentUpdate::Untouched,
        };
        let err = plan_update(&input).unwrap_err();
        assert_eq!(
            err.message(),
            "Nothing to update \u{2014} provide at least one field to change."
        );
        // Whitespace-only names are silently ignored (ported bug), so a
        // blank name alone is still "nothing to update".
        let blank = UpdateInput {
            name: Some("   "),
            description_md: None,
            priority: None,
            state_given: false,
            parent: ParentUpdate::Untouched,
        };
        assert!(plan_update(&blank).is_err());
    }

    #[test]
    fn parent_tri_state() {
        assert_eq!(parent_update(Some(PARENT_UNSET)), ParentUpdate::Untouched);
        assert_eq!(parent_update(None), ParentUpdate::Unlink);
        assert_eq!(parent_update(Some("")), ParentUpdate::Unlink);
        assert_eq!(parent_update(Some("abc")), ParentUpdate::Link("abc"));
    }

    #[test]
    fn resolve_state_branches() {
        let states = [
            StateRef {
                id: "a",
                is_default: false,
                sequence: 2.0,
            },
            StateRef {
                id: "b",
                is_default: true,
                sequence: 1.0,
            },
        ];
        assert_eq!(resolve_state(Some("a"), &states).unwrap().unwrap().id, "a");
        assert_eq!(
            resolve_state(Some("zzz"), &states).unwrap_err().message(),
            "State zzz is not a valid state for this project."
        );
        assert_eq!(resolve_state(None, &states).unwrap().unwrap().id, "b");
        let no_default = [StateRef {
            id: "a",
            is_default: false,
            sequence: 2.0,
        }];
        assert_eq!(resolve_state(None, &no_default).unwrap().unwrap().id, "a");
        assert!(resolve_state(None, &[]).unwrap().is_none());
        // Empty-string state id falls through to default selection.
        assert_eq!(resolve_state(Some(""), &states).unwrap().unwrap().id, "b");
    }

    #[test]
    fn parent_link_guards() {
        assert!(check_parent_link("p1", "p2", "x", None, &[]).is_err());
        assert_eq!(
            check_parent_link("p1", "p1", "c", Some("c"), &[])
                .unwrap_err()
                .message(),
            "An issue can't be its own parent."
        );
        // Child appears in the proposed parent's ancestor chain.
        let ancestors = [Some("mid".to_owned()), Some("child-1".to_owned())];
        assert_eq!(
            check_parent_link("p", "p", "par", Some("child-1"), &ancestors)
                .unwrap_err()
                .message(),
            "That parent link would create a cycle."
        );
        // A pre-existing cycle in the chain stops the walk, no error.
        let cyclic = [Some("loop".to_owned()), Some("loop".to_owned())];
        assert!(check_parent_link("p", "p", "par", Some("other"), &cyclic).is_ok());
        assert!(check_parent_link("p", "p", "par", Some("other"), &[None]).is_ok());
    }

    #[test]
    fn scoping_error_messages() {
        assert_eq!(
            ToolError::project_not_found("p").message(),
            "Project p not found or not accessible."
        );
        assert_eq!(
            ToolError::issue_not_found("i").message(),
            "Issue i not found or not accessible."
        );
        assert_eq!(
            ToolError::write_denied().message(),
            "You don't have permission to make changes in this project."
        );
        assert!(parse_scoped_issue_id("not-a-uuid").is_err());
        assert!(parse_scoped_issue_id("11111111-1111-1111-1111-111111111111").is_ok());
    }

    #[test]
    fn filter_error_messages_are_byte_exact() {
        let bad_group = ListFilterInput {
            state_ids: vec![],
            state_groups: vec!["bogus".to_owned()],
            priorities: vec![],
            parent_tokens: vec![],
            label_ids: vec![],
        };
        assert_eq!(
            normalize_list_filters(&bad_group).unwrap_err().message(),
            "Unknown state group(s): bogus. Valid groups: backlog, unstarted, started, review, test, completed, cancelled."
        );
        let bad_prio = ListFilterInput {
            state_ids: vec![],
            state_groups: vec![],
            priorities: vec!["critical".to_owned()],
            parent_tokens: vec![],
            label_ids: vec![],
        };
        assert_eq!(
            normalize_list_filters(&bad_prio).unwrap_err().message(),
            "Unknown priority value(s): critical. Valid priorities: urgent, high, medium, low, none."
        );
        let mixed_parent = ListFilterInput {
            state_ids: vec![],
            state_groups: vec![],
            priorities: vec![],
            parent_tokens: vec!["null".to_owned(), "abc".to_owned()],
            label_ids: vec![],
        };
        assert_eq!(
            normalize_list_filters(&mixed_parent).unwrap_err().message(),
            "parent=null cannot be combined with specific parent issues."
        );
    }

    #[test]
    fn null_parent_spellings_and_state_uuid_pass_through() {
        for token in ["null", "NULL", "none", "None"] {
            let input = ListFilterInput {
                state_ids: vec![],
                state_groups: vec![],
                priorities: vec![],
                parent_tokens: vec![token.to_owned()],
                label_ids: vec![],
            };
            let out = normalize_list_filters(&input).unwrap();
            assert!(out.parent_is_null, "{token}");
            assert!(out.parent_in.is_empty());
        }
        let input = ListFilterInput {
            state_ids: vec!["not-a-uuid".to_owned()],
            state_groups: vec!["Review".to_owned()],
            priorities: vec!["HIGH".to_owned()],
            parent_tokens: vec![],
            label_ids: vec![],
        };
        let out = normalize_list_filters(&input).unwrap();
        assert!(out.state_in.is_empty());
        assert_eq!(out.state_group_in, vec!["review"]);
        assert_eq!(out.priority_in, vec!["high"]);
    }

    #[test]
    fn sequence_token_guards() {
        assert_eq!(search_sequence_tokens("fix 42"), vec![42]);
        // The int4 story from search/issue.py: a 10-digit overflow paste
        // contributes no token instead of 500ing.
        assert_eq!(
            search_sequence_tokens("error 9999999999"),
            Vec::<i64>::new()
        );
        assert_eq!(
            search_sequence_tokens("error 2147483647"),
            vec![2_147_483_647]
        );
        assert_eq!(
            search_sequence_tokens("error 2147483648"),
            Vec::<i64>::new()
        );
        // Queries over 20 chars (code points) contribute no tokens.
        assert_eq!(
            search_sequence_tokens("a very long query over twenty"),
            Vec::<i64>::new()
        );
        // Digits glued to words are not tokens.
        assert_eq!(search_sequence_tokens("abc123"), Vec::<i64>::new());
        assert!(!search_uses_fts("   "));
        assert!(search_uses_fts("x"));
    }

    #[test]
    fn relation_type_validation_message() {
        assert_eq!(validate_relation_type(" Blocking ").unwrap(), "blocking");
        assert_eq!(
            validate_relation_type("friend").unwrap_err().message(),
            "relation_type must be one of: blocked_by, blocking, relates_to, duplicate, start_before, start_after, finish_before, finish_after, implemented_by, implements"
        );
    }

    #[test]
    fn relation_edge_mapping() {
        assert_eq!(stored_edge("blocking"), (true, "blocked_by"));
        assert_eq!(stored_edge("blocked_by"), (false, "blocked_by"));
        assert_eq!(stored_edge("implements"), (true, "implemented_by"));
        assert_eq!(stored_edge("relates_to"), (false, "relates_to"));
        assert_eq!(inverse_relation("blocked_by"), "blocking");
        assert_eq!(inverse_relation("blocking"), "blocked_by");
    }

    #[test]
    fn relation_target_guards() {
        let source = RelationEndpoint {
            id: "a".to_owned(),
            workspace_id: "w".to_owned(),
            display: "PROJ-1".to_owned(),
        };
        let other_ws = RelationEndpoint {
            id: "b".to_owned(),
            workspace_id: "x".to_owned(),
            display: "PROJ-2".to_owned(),
        };
        assert_eq!(
            check_relation_targets(&source, std::slice::from_ref(&source))
                .unwrap_err()
                .message(),
            "PROJ-1 cannot be related to itself"
        );
        assert_eq!(
            check_relation_targets(&source, &[other_ws])
                .unwrap_err()
                .message(),
            "PROJ-2 is in a different workspace"
        );
        assert_eq!(
            EMPTY_RELATED_MESSAGE,
            "related_issues must list at least one issue."
        );
        assert!(relation_write_summary("Related", "PROJ-1", "blocked_by", &[]).is_none());
    }

    #[test]
    fn ref_classification() {
        assert!(matches!(
            classify_issue_ref("11111111-1111-1111-1111-111111111111"),
            IssueRef::Id(_)
        ));
        assert_eq!(
            classify_issue_ref("PROJ-12"),
            IssueRef::Identifier {
                project_code: "PROJ".to_owned(),
                sequence_id: 12
            }
        );
        assert_eq!(classify_issue_ref("PROJ-"), IssueRef::Unresolved);
        assert_eq!(classify_issue_ref("12"), IssueRef::Unresolved);
        assert_eq!(classify_issue_ref(""), IssueRef::Unresolved);
        assert_eq!(classify_issue_ref("a-b-c"), IssueRef::Unresolved);
    }

    #[test]
    fn viewpoint_naming() {
        let forward = RelationRow {
            issue_id: "a",
            related_issue_id: "b",
            stored_type: "blocked_by",
        };
        assert_eq!(type_from_viewpoint(&forward, "a"), "blocked_by");
        assert_eq!(type_from_viewpoint(&forward, "b"), "blocking");
        // Reverse-stored legacy rows normalize like blockers does.
        let legacy = RelationRow {
            issue_id: "b",
            related_issue_id: "a",
            stored_type: "blocking",
        };
        assert_eq!(type_from_viewpoint(&legacy, "a"), "blocked_by");
        assert_eq!(type_from_viewpoint(&legacy, "b"), "blocking");
    }

    #[test]
    fn relate_shapes() {
        let created = vec!["PROJ-2".to_owned()];
        let value = relate_result("PROJ-1", "blocked_by", &created, &[], &[]);
        assert_eq!(value["created"], json!(["PROJ-2"]));
        assert_eq!(value["unchanged"], json!([]));
        let un = unrelate_result("PROJ-1", "blocked_by", &[], &["PROJ-2".to_owned()]);
        assert_eq!(un["not_related"], json!(["PROJ-2"]));
        assert_eq!(relation_sort_key("B", 2), ("B".to_owned(), 2));
    }

    #[test]
    fn schemas_cover_all_nine_tools() {
        let schemas = tool_schemas();
        let names: Vec<&str> = schemas.iter().map(|schema| schema.name).collect();
        assert_eq!(
            names,
            vec![
                "search_issues",
                "list_issues",
                "list_my_issues",
                "get_issue",
                "create_issue",
                "update_issue",
                "relate_issues",
                "unrelate_issues",
                "list_issue_relations",
            ]
        );
        // Fixture params (F-A6-10 tools.issues): required sets match the
        // Python signatures; the update parent sentinel is the default.
        let by_name = |name: &str| schemas.iter().find(|schema| schema.name == name).unwrap();
        assert_eq!(
            by_name("search_issues").parameters["required"],
            json!(["query"])
        );
        assert_eq!(
            by_name("list_issues").parameters["required"],
            json!(["project_id"])
        );
        assert_eq!(
            by_name("update_issue").parameters["properties"]["parent_issue_id"]["default"],
            "__unset__"
        );
    }

    #[test]
    fn my_issues_keeps_soft_deleted_links() {
        // M2M traversals carry no deleted_at predicate (Python parity —
        // only the explicit through-model joins filter).
        assert!(!my_issues_scope_sql("assigned").contains("deleted_at"));
        assert!(!my_issues_scope_sql("all").contains("deleted_at"));
        assert!(my_issues_scope_sql("weird").contains("issue_subscribers"));
    }

    #[test]
    fn detail_shape_merges_brief() {
        let row = BriefRow {
            id: "i",
            project_id: "p",
            project_identifier: "PROJ",
            sequence_id: 1,
            name: "N",
            parent_id: None,
            state_name: None,
            state_group: None,
            priority: "none",
        };
        let value = detail_shape(brief_row(&row), "Desc", false, vec![]);
        assert_eq!(value["identifier"], "PROJ-1");
        assert_eq!(value["description"], "<untrusted>Desc</untrusted>");
        assert_eq!(value["recent_comments"], json!([]));
    }

    #[test]
    fn sql_fragments_name_real_tables() {
        assert!(SCOPED_ISSUES_SQL.contains("FROM issues"));
        assert!(SCOPED_ISSUES_SQL.contains("project_members"));
        assert!(SCOPED_ISSUES_SQL.contains("NOT (states.group = 'triage')"));
        assert!(MEMBER_PROJECTS_SQL.contains("FROM projects"));
        assert!(RESOLVE_IDENTIFIER_SQL.contains("UPPER(projects.identifier)"));
        assert!(PAIR_ROWS_SQL.contains("FROM issue_relations"));
        assert!(GROUPED_RELATIONS_SQL.contains("workspace_id = $2"));
        assert!(ISSUE_COMMENTS_SQL.contains("ORDER BY issue_comments.created_at DESC LIMIT 10"));
        assert!(CREATE_ISSUE_INSERT_SQL.contains("created_via"));
    }

    #[test]
    fn dispatch_and_sequence_rules() {
        assert!(should_dispatch_transition(true, "a", "b"));
        assert!(!should_dispatch_transition(true, "a", "a"));
        assert!(!should_dispatch_transition(false, "a", "b"));
        assert_eq!(next_sequence(None), 1);
        assert_eq!(next_sequence(Some(41)), 42);
    }

    #[test]
    fn envelope_keys_match_fixture() {
        let value = message_envelope(
            "id",
            "tool_result",
            "did it",
            "completed",
            3,
            None,
            &json!({"links": []}),
            "2026-09-29T03:39:16.065775+00:00",
            None,
        );
        assert_eq!(value["role"], "tool_result");
        assert_eq!(value["content"], "did it");
        assert_eq!(value["turn_id"], Value::Null);
        assert_eq!(value["completed_at"], Value::Null);
    }

    #[test]
    fn seq_starts_at_one_on_empty_threads() {
        // `(MAX(seq) or 0) + 1` (`events.py:73-80`): the first message /
        // event in a thread takes seq 1, not 0.
        assert!(RECORD_MESSAGE_INSERT_SQL.contains("COALESCE(MAX(seq), 0) + 1"));
        assert!(RECORD_EVENT_INSERT_SQL.contains("COALESCE(MAX(seq), 0) + 1"));
        assert!(!RECORD_MESSAGE_INSERT_SQL.contains("MAX(seq), -1"));
        assert!(!RECORD_EVENT_INSERT_SQL.contains("MAX(seq), -1"));
    }

    #[test]
    fn soft_delete_predicates_match_default_managers() {
        // Every `.objects` query in the ported Python runs under
        // `SoftDeleteModel.objects` (`db/mixins.py:56-58`); joins never
        // carry manager filters, so exactly the queried tables predicate.
        assert!(SCOPED_ISSUES_SQL.contains("issues.deleted_at IS NULL"));
        assert!(MEMBER_PROJECTS_SQL.contains("projects.deleted_at IS NULL"));
        assert!(RESOLVE_IDENTIFIER_SQL.contains("issues.deleted_at IS NULL"));
        assert!(CREATE_ISSUE_MAX_SEQ_SQL.contains("deleted_at IS NULL"));
        assert!(PROJECT_WRITE_GATE_SQL.contains("project_members.deleted_at IS NULL"));
        assert!(PROJECT_WRITE_GATE_SQL.contains("workspace_members.deleted_at IS NULL"));
        // M2M traversals join the through tables with no manager filter
        // (only the explicit through-model joins filter) — filtering here
        // would hide issues Python still returns.
        assert!(!my_issues_scope_sql("assigned").contains("deleted_at"));
    }

    #[test]
    fn write_gate_binds_workspace_slug() {
        // `check_project_role` filters `workspace__slug`, i.e. a join —
        // never a bare `workspace_id = <slug>` comparison, which would
        // fail at runtime (UUID column vs text).
        assert!(PROJECT_WRITE_GATE_SQL.contains("workspaces.slug = $2"));
        assert!(!PROJECT_WRITE_GATE_SQL.contains("workspace_id = $2"));
        assert!(PROJECT_WRITE_GATE_SQL.contains("role IN (20, 15)"));
        assert!(PROJECT_WRITE_GATE_SQL.contains("role = 20"));
    }

    #[test]
    fn fts_escapes_like_metacharacters() {
        assert_eq!(escape_icontains("100%"), "100\\%");
        assert_eq!(escape_icontains("a_b\\c"), "a\\_b\\\\c");
        assert_eq!(escape_icontains("plain"), "plain");
        assert!(ISSUE_FTS_SQL.contains("$3"));
        assert!(ISSUE_FTS_SQL.contains("ESCAPE '\\'"));
    }

    #[test]
    fn braced_uuids_match_python_constructor() {
        let id = "11111111-1111-1111-1111-111111111111";
        assert_eq!(strip_uuid_braces(&format!("{{{id}}}")), id);
        assert_eq!(strip_uuid_braces("{abc"), "{abc");
        assert_eq!(strip_uuid_braces("abc}"), "abc}");
        assert!(is_uuid_token(&format!("{{{id}}}")));
        assert!(matches!(
            classify_issue_ref(&format!("{{{id}}}")),
            IssueRef::Id(_)
        ));
        // Braces are stripped for the UUID attempt only: the identifier
        // fallback still sees the original string, as in `resolve_refs`.
        assert_eq!(classify_issue_ref("{PROJ-12}"), IssueRef::Unresolved);
        assert!(parse_scoped_issue_id(&format!("{{{id}}}")).is_ok());
    }

    #[test]
    fn write_summaries_are_byte_exact() {
        assert_eq!(
            created_summary("PROJ-7", "Fix it"),
            "Created issue PROJ-7 \u{2014} Fix it"
        );
        assert_eq!(
            updated_summary("PROJ-7", &["name", "state"]),
            "Updated issue PROJ-7 (name, state)"
        );
        assert_eq!(
            unresolved_refs_message(&["PROJ-9", "nope"]),
            "Issues not found or not accessible: PROJ-9, nope"
        );
    }

    #[test]
    fn grouped_item_shape() {
        let value = relation_item("i", "PROJ-2", "Some work", Some("Backlog"), Some("backlog"));
        assert_eq!(value["id"], "i");
        assert_eq!(value["identifier"], "PROJ-2");
        assert_eq!(value["name"], "<untrusted>Some work</untrusted>");
        assert_eq!(value["state"], "Backlog");
        assert_eq!(value["state_group"], "backlog");
        let long = "n".repeat(300);
        let trunc = relation_item("i", "P-1", &long, None, None);
        assert_eq!(
            trunc["name"],
            format!("<untrusted>{}</untrusted>", "n".repeat(200))
        );
        assert_eq!(RELATION_CREATED_ACTIVITY, "issue_relation.activity.created");
        assert_eq!(RELATION_DELETED_ACTIVITY, "issue_relation.activity.deleted");
    }
}
