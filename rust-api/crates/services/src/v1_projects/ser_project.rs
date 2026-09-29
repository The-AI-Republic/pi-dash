#![forbid(unsafe_code)]

//! Project serializers (D-19 serializers A, PIDASHCONV-348).
//!
//! Ports `apps/api/pi_dash/api/serializers/project.py:24-377`
//! (`_validate_default_agent_executor` L24-48, `ProjectCreateSerializer`
//! L49-196, `ProjectUpdateSerializer` L197-250, `ProjectSerializer` L251-353,
//! `ProjectLiteSerializer` L354-377) plus the identifier-taken error shapes
//! the views map serializer failures to (`api/views/project.py:272-284`
//! create, `:455-467` patch).
//!
//! Fixture: `FX-PROJ-SER`
//! (`rust-api/fixtures/v1_projects/serializers/project.golden.json`).
//!
//! This module is pure: every check that needs the database in Python
//! (workspace membership, `ProjectIdentifier` pre-check, default-state and
//! estimate scoping, the `is_default` atomic unset, the random `logo_props`
//! default) takes the already-fetched fact as an argument. The queries and
//! handler layers supply those facts and perform the writes; the error
//! bodies and check order here are the contract they must honor.
//!
//! JSON rendering notes (Porting guide DRF rows):
//! - Every error body below is a byte-exact `&str` const in DRF key order.
//!   `serde_json::Map` without the `preserve_order` feature sorts keys, so
//!   multi-key bodies are never built with `json!`; single-key dicts and
//!   ordered `Serialize` structs (field order is preserved) are safe.
//! - DRF wraps a `ValidationError("msg")` raised in `validate()` as
//!   `{"non_field_errors": ["msg"]}`, and a `ValidationError({"f": "msg"})`
//!   as `{"f": ["msg"]}` (verified against DRF 3.18.1). The fixture records
//!   the managed-runner and html cases in raw-string shorthand; the consts
//!   carry the list-wrapped bytes Django actually emits.
//! - `ValidationError(detail="...")` raised in `create()` carries a bare
//!   list detail (`["..."]`); on the D-19 routes the views catch it and
//!   answer the 409 identifier-taken body instead, so the detail strings
//!   survive here only as consts for the handler layer's branch decision.

/// `AgentExecutorKind.CLOUD_AGENT` value (`core/agent_execution.py:7-16`).
pub const EXECUTOR_CLOUD_AGENT: &str = "cloud_agent";
/// `AgentExecutorKind.MANAGED_RUNNER` value.
pub const EXECUTOR_MANAGED_RUNNER: &str = "managed_runner";
/// `AgentExecutorKind.LOCAL_RUNNER` value (the default; always accepted here).
pub const EXECUTOR_LOCAL_RUNNER: &str = "local_runner";

/// Outcome of `_validate_default_agent_executor`
/// (`serializers/project.py:24-48`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorCheck {
    /// Either unset/any other kind, or the requested executor is available.
    Ok,
    /// `cloud_agent` while `cloud_agent_is_configured()` is false: Python
    /// raises `CloudAgentUnavailableAPI`, which propagates as its own 409
    /// response, not a serializer 400.
    CloudUnavailable,
    /// `managed_runner` while `managed_runner_is_enabled()` is false.
    ManagedDisabled,
}

/// Ports `_validate_default_agent_executor(data)`: `executor` is the raw
/// `default_agent_executor` input (`None` when absent), the booleans are the
/// two instance kill switches. Anything other than the two gated kinds
/// passes, exactly like the `if / elif` with no `else`.
pub fn check_default_agent_executor(
    executor: Option<&str>,
    cloud_configured: bool,
    managed_enabled: bool,
) -> ExecutorCheck {
    match executor {
        Some(e) if e == EXECUTOR_CLOUD_AGENT && !cloud_configured => {
            ExecutorCheck::CloudUnavailable
        }
        Some(e) if e == EXECUTOR_MANAGED_RUNNER && !managed_enabled => {
            ExecutorCheck::ManagedDisabled
        }
        _ => ExecutorCheck::Ok,
    }
}

/// `Project.FORBIDDEN_IDENTIFIER_CHARS_PATTERN`
/// (`db/models/project.py:226`), applied with `re.match`.
pub const FORBIDDEN_IDENTIFIER_CHARS_PATTERN: &str = r"^.*[&+,:;$^}{*=?@#|'<>.()%!-].*$";

/// The 24 forbidden characters of the class above, in pattern order.
/// Kept as a table so no `regex` dependency is needed.
pub const FORBIDDEN_CHARS: &[char] = &[
    '&', '+', ',', ':', ';', '$', '^', '}', '{', '*', '=', '?', '@', '#', '|', '\'', '<', '>', '.',
    '(', ')', '%', '!', '-',
];

/// Ports `re.match(FORBIDDEN_IDENTIFIER_CHARS_PATTERN, value)` byte for
/// byte. Python `.` never crosses `\n` and `$` only anchors at the end, so
/// a value with an interior newline can never match (verified against
/// CPython: `"a&b\nc"`, `"a\nb&c"`, `"&\n&"` all pass while `"a&b"`,
/// `"a&b\n"`, `"a\rb&c"` are rejected). Equivalent rule: strip one trailing
/// newline; reject iff the rest is single-line and holds a forbidden char.
pub fn contains_forbidden_chars(value: &str) -> bool {
    let single_line = value.strip_suffix('\n').unwrap_or(value);
    if single_line.contains('\n') {
        return false;
    }
    single_line.chars().any(|c| FORBIDDEN_CHARS.contains(&c))
}

/// Ports the `create()` normalisation
/// (`serializers/project.py:169,332`): strip then upper-case. `str::trim`
/// and `to_uppercase` are Unicode-aware like Python's `strip`/`upper`;
/// `Project.save()` (`db/models/project.py:255`) repeats the same
/// normalisation, so an unnormalised `validated_data` identifier still
/// lands upper-cased in the row.
pub fn normalize_identifier(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// `validate()` name check (`serializers/project.py:144-145,218-219,288-289`).
pub const NAME_FORBIDDEN_BODY: &str =
    r#"{"non_field_errors":["Project name cannot contain special characters."]}"#;
/// `validate()` identifier check (`:147-148,221-222,291-292`).
pub const IDENTIFIER_FORBIDDEN_BODY: &str =
    r#"{"non_field_errors":["Project identifier cannot contain special characters."]}"#;
/// `validate()` project-lead check (`:150-156,299-307`).
pub const PROJECT_LEAD_NOT_MEMBER_BODY: &str =
    r#"{"non_field_errors":["Project lead should be a user in the workspace"]}"#;
/// `validate()` default-assignee check (`:158-164,309-317`).
pub const DEFAULT_ASSIGNEE_NOT_MEMBER_BODY: &str =
    r#"{"non_field_errors":["Default assignee should be a user in the workspace"]}"#;
/// `ProjectUpdateSerializer.update()` default-state check (`:225-230`).
pub const DEFAULT_STATE_OUTSIDE_PROJECT_BODY: &str =
    r#"{"non_field_errors":["Default state should be a state in the project"]}"#;
/// `update()` estimate check (`:232-237`); the doubled "a estimate" is
/// verbatim from the Python message.
pub const ESTIMATE_OUTSIDE_PROJECT_BODY: &str =
    r#"{"non_field_errors":["Estimate should be a estimate in the project"]}"#;
/// Unsetting the default (`:238-241` update, `:294-297` read validate,
/// model backstop `db/models/project.py:274-290`).
pub const UNSET_DEFAULT_BODY: &str = r#"{"non_field_errors":["Default project cannot be unset without assigning another default project."]}"#;
/// `managed_runner` while the instance kill switch is off (`:39-45`).
/// DRF renders the dict value list-wrapped; the fixture shows the raw
/// string shorthand.
pub const MANAGED_RUNNER_DISABLED_BODY: &str =
    r#"{"default_agent_executor":["Pi Dash Agent is not enabled on this instance"]}"#;
/// `ProjectSerializer.validate()` html branch (`:319-327`): Python raises
/// `ValidationError({"error": ...})`, which DRF renders list-wrapped.
pub const HTML_INVALID_BODY: &str = r#"{"error":["html content is not valid"]}"#;
/// `CloudAgentUnavailableAPI` (`cloud_agent/api.py:5-8`): status 409, keys
/// in definition order (`error`, then `code`).
pub const CLOUD_AGENT_UNAVAILABLE_BODY: &str = r#"{"error":"Pi Dash Cloud Agent is not currently available","code":"cloud_agent_unavailable"}"#;
/// Status of the cloud-agent-unavailable response.
pub const CLOUD_AGENT_UNAVAILABLE_STATUS: u16 = 409;
/// Unhandled-exception branch of `BaseAPIView.handle_exception`
/// (`api/views/base.py:133-170`): e.g. the `description_html` non-dict
/// quirk below, which raises `UnboundLocalError` in Python.
pub const GENERIC_500_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;
/// Serializer-level detail of the empty-identifier raise
/// (`serializers/project.py:172,334`): a bare list on the wire. The D-19
/// views catch it and answer [`IDENTIFIER_TAKEN_BODY`] 409 instead, so this
/// string never reaches a client on these routes.
pub const IDENTIFIER_REQUIRED_DETAIL: &str = "Project Identifier is required";
/// Serializer-level detail of the taken-identifier raise (`:175,337`):
/// likewise swallowed by the views' 409 branch.
pub const IDENTIFIER_TAKEN_DETAIL: &str = "Project Identifier is taken";
/// POST identifier-taken 409 (`views/project.py:280-284`): answered when a
/// `ProjectIdentifier` row already claims `(name, workspace_id)`.
pub const IDENTIFIER_TAKEN_BODY: &str =
    r#"{"identifier":"The project identifier is already taken"}"#;
/// POST name-taken 409 (`views/project.py:272-277`, patch `:455-461`):
/// answered on `IntegrityError` containing "already exists". Ported bug
/// BUG-1: a taken identifier usually lands here, because
/// `ProjectCreateSerializer.create` never writes the `ProjectIdentifier`
/// row its own pre-check reads, so the clash trips the `projects` unique
/// index instead (`handlers/project.golden.json`, contract
/// `test_create_conflicts`).
pub const NAME_TAKEN_BODY: &str = r#"{"name":"The project name is already taken"}"#;
/// DRF field machinery (not `validate()`): absent required `identifier`
/// (`Meta.fields`, fixture `create_missing_identifier`).
pub const IDENTIFIER_FIELD_REQUIRED_BODY: &str = r#"{"identifier":["This field is required."]}"#;

/// `ProjectCreateSerializer.Meta.fields` (`serializers/project.py:100-127`),
/// in source order.
pub const CREATE_META_FIELDS: &[&str] = &[
    "name",
    "description",
    "project_lead",
    "default_assignee",
    "identifier",
    "icon_prop",
    "emoji",
    "cover_image",
    "module_view",
    "cycle_view",
    "issue_views_view",
    "page_view",
    "intake_view",
    "guest_view_all_features",
    "members_can_edit_states",
    "archive_in",
    "close_in",
    "timezone",
    "external_source",
    "external_id",
    "is_issue_type_enabled",
    "is_time_tracking_enabled",
    "is_default",
    "repo_url",
    "base_branch",
    "default_agent_executor",
];

/// `ProjectCreateSerializer.Meta.read_only_fields` (`:129-137`).
pub const CREATE_META_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "workspace",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "logo_props",
];

/// Fields `ProjectUpdateSerializer.Meta` adds (`:207-210`); `read_only_fields`
/// are inherited unchanged (`:212`).
pub const UPDATE_EXTRA_FIELDS: &[&str] = &["default_state", "estimate"];

/// `ProjectSerializer` read-only annotation fields
/// (`serializers/project.py:259-266`), in declaration order. With
/// `Meta.fields = "__all__"`, DRF renders `id`, then these, then the model
/// fields (`get_default_field_names`, verified against DRF 3.18.1).
/// `sort_order` is annotated only in the list GET (`views/project.py:169-177`)
/// and reads null on detail/create/update responses.
pub const READ_ANNOTATION_FIELDS: &[&str] = &[
    "total_members",
    "total_cycles",
    "total_modules",
    "is_member",
    "sort_order",
    "member_role",
    "is_deployed",
    "cover_image_url",
];

/// `ProjectSerializer.Meta.read_only_fields` (`:271-281`).
pub const READ_READ_ONLY_FIELDS: &[&str] = &[
    "id",
    "emoji",
    "workspace",
    "created_at",
    "updated_at",
    "created_by",
    "updated_by",
    "deleted_at",
    "cover_image_url",
];

/// `ProjectLiteSerializer.Meta.fields` (`:366-376`); `read_only_fields` is
/// the same list (`:377`).
pub const LITE_FIELDS: &[&str] = &[
    "id",
    "identifier",
    "name",
    "cover_image",
    "icon_prop",
    "emoji",
    "description",
    "is_default",
    "cover_image_url",
];

/// `PROJECT_ICON_DEFAULT_COLORS` (`serializers/project.py:57-66`).
pub const PROJECT_ICON_DEFAULT_COLORS: &[&str] = &[
    "#95999f", "#6d7b8a", "#5e6ad2", "#02b5ed", "#02b55c", "#f2be02", "#e57a00", "#f38e82",
];

/// `PROJECT_ICON_DEFAULT_ICONS` (`:67-96`).
pub const PROJECT_ICON_DEFAULT_ICONS: &[&str] = &[
    "home",
    "apps",
    "settings",
    "star",
    "favorite",
    "done",
    "check_circle",
    "add_task",
    "create_new_folder",
    "dataset",
    "terminal",
    "key",
    "rocket",
    "public",
    "quiz",
    "mood",
    "gavel",
    "eco",
    "diamond",
    "forest",
    "bolt",
    "sync",
    "cached",
    "library_add",
    "view_timeline",
    "view_kanban",
    "empty_dashboard",
    "cycle",
];

/// The `logo_props` default shape (`serializers/project.py:177-185`):
/// `{"in_use": "icon", "icon": {"name": ..., "color": ...}}`. A struct keeps
/// DRF's key order (`serde_json::Map` without `preserve_order` would sort
/// `icon` before `in_use`). Python picks name/color with `random.choice`;
/// the handler layer supplies the picks, this builds the bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LogoIcon {
    /// Icon name, one of [`PROJECT_ICON_DEFAULT_ICONS`].
    pub name: String,
    /// Icon color, one of [`PROJECT_ICON_DEFAULT_COLORS`].
    pub color: String,
}

/// The `logo_props` default document.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LogoProps {
    /// Always `"icon"` on this path.
    pub in_use: String,
    /// The chosen icon.
    pub icon: LogoIcon,
}

/// Builds the `logo_props` default document for the chosen icon name/color.
pub fn logo_props_default(icon_name: &str, icon_color: &str) -> LogoProps {
    LogoProps {
        in_use: "icon".to_string(),
        icon: LogoIcon {
            name: icon_name.to_string(),
            color: icon_color.to_string(),
        },
    }
}

/// Every failure the project serializers can produce, in the exact HTTP
/// shape the Django stack emits on the D-19 routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSerError {
    /// `cloud_agent` while unconfigured: `CloudAgentUnavailableAPI` 409.
    CloudAgentUnavailable,
    /// `managed_runner` while disabled: serializer 400.
    ManagedRunnerDisabled,
    /// Name holds a forbidden character.
    NameForbidden,
    /// Identifier holds a forbidden character.
    IdentifierForbidden,
    /// `project_lead` is not a workspace member.
    ProjectLeadNotMember,
    /// `default_assignee` is not a workspace member.
    DefaultAssigneeNotMember,
    /// Normalised identifier is empty. Python raises
    /// `ValidationError(detail="Project Identifier is required")`, which the
    /// D-19 views swallow into the 409 identifier-taken body (ported quirk:
    /// a blank identifier answers 409, not 400).
    IdentifierRequired,
    /// The `ProjectIdentifier` pre-check hit. The views answer the 409
    /// identifier-taken body.
    IdentifierTaken,
    /// `default_state` is not a state of this project.
    DefaultStateOutsideProject,
    /// `estimate` is not an estimate of this project.
    EstimateOutsideProject,
    /// Unsetting the default with no replacement.
    UnsetDefault,
    /// `description_html` dict failed `validate_html_content`.
    HtmlInvalid,
    /// Ported bug: truthy non-dict `description_html` reaches
    /// `if not is_valid` with `is_valid` unbound (`:320-327`), raising
    /// `UnboundLocalError`, which `handle_exception` answers with the
    /// generic 500 body.
    DescriptionHtmlNonDict,
}

impl ProjectSerError {
    /// HTTP status Django answers with.
    pub fn status(&self) -> u16 {
        match self {
            ProjectSerError::CloudAgentUnavailable
            | ProjectSerError::IdentifierRequired
            | ProjectSerError::IdentifierTaken => 409,
            ProjectSerError::DescriptionHtmlNonDict => 500,
            _ => 400,
        }
    }

    /// Byte-exact response body.
    pub fn body(&self) -> &'static str {
        match self {
            ProjectSerError::CloudAgentUnavailable => CLOUD_AGENT_UNAVAILABLE_BODY,
            ProjectSerError::ManagedRunnerDisabled => MANAGED_RUNNER_DISABLED_BODY,
            ProjectSerError::NameForbidden => NAME_FORBIDDEN_BODY,
            ProjectSerError::IdentifierForbidden => IDENTIFIER_FORBIDDEN_BODY,
            ProjectSerError::ProjectLeadNotMember => PROJECT_LEAD_NOT_MEMBER_BODY,
            ProjectSerError::DefaultAssigneeNotMember => DEFAULT_ASSIGNEE_NOT_MEMBER_BODY,
            ProjectSerError::IdentifierRequired | ProjectSerError::IdentifierTaken => {
                IDENTIFIER_TAKEN_BODY
            }
            ProjectSerError::DefaultStateOutsideProject => DEFAULT_STATE_OUTSIDE_PROJECT_BODY,
            ProjectSerError::EstimateOutsideProject => ESTIMATE_OUTSIDE_PROJECT_BODY,
            ProjectSerError::UnsetDefault => UNSET_DEFAULT_BODY,
            ProjectSerError::HtmlInvalid => HTML_INVALID_BODY,
            ProjectSerError::DescriptionHtmlNonDict => GENERIC_500_BODY,
        }
    }
}

impl From<ExecutorCheck> for Result<(), ProjectSerError> {
    fn from(check: ExecutorCheck) -> Self {
        match check {
            ExecutorCheck::Ok => Ok(()),
            ExecutorCheck::CloudUnavailable => Err(ProjectSerError::CloudAgentUnavailable),
            ExecutorCheck::ManagedDisabled => Err(ProjectSerError::ManagedRunnerDisabled),
        }
    }
}

/// `description_html` input for [`check_description_html`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptionHtml<'a> {
    /// Key absent or falsy: the branch is skipped (`:320`).
    MissingOrFalsy,
    /// A dict value: `validate_html_content(str(value))` already ran; the
    /// handler layer supplies its verdict. `sanitized` replaces the input
    /// when `Some` (`:322-325`); note Python replaces first and raises
    /// after, so on invalid input the replacement is unobservable.
    Dict {
        /// The `is_valid` element of the validator's 3-tuple.
        is_valid: bool,
        /// The `clean_html` element; written back when `Some`.
        sanitized: Option<&'a str>,
    },
    /// Truthy non-dict: ported bug, see [`ProjectSerError::DescriptionHtmlNonDict`].
    OtherTruthy,
}

/// Ports the `description_html` branch (`serializers/project.py:320-327`).
/// Returns the sanitiser replacement to write back (`Some`), or `None` to
/// leave the input untouched.
pub fn check_description_html(
    input: DescriptionHtml<'_>,
) -> Result<Option<String>, ProjectSerError> {
    match input {
        DescriptionHtml::MissingOrFalsy => Ok(None),
        DescriptionHtml::Dict {
            is_valid,
            sanitized,
        } => {
            if !is_valid {
                return Err(ProjectSerError::HtmlInvalid);
            }
            Ok(sanitized.map(str::to_string))
        }
        DescriptionHtml::OtherTruthy => Err(ProjectSerError::DescriptionHtmlNonDict),
    }
}

/// Shared `validate()` inputs (`ProjectCreateSerializer.validate`,
/// `serializers/project.py:139-166`; inherited unchanged by the update and
/// read serializers). `Option<&str>` is `None` when the key is absent
/// (partial updates skip absent keys via `data.get`); `Option<bool>` facts
/// are `None` when the key is absent and `Some(exists)` otherwise.
pub struct SharedChecks<'a> {
    /// Raw `default_agent_executor` input.
    pub executor: Option<&'a str>,
    /// `cloud_agent_is_configured()`.
    pub cloud_configured: bool,
    /// `managed_runner_is_enabled()`.
    pub managed_enabled: bool,
    /// Raw `name` input.
    pub name: Option<&'a str>,
    /// Raw `identifier` input.
    pub identifier: Option<&'a str>,
    /// `WorkspaceMember` row for `project_lead`, when supplied.
    pub project_lead_is_member: Option<bool>,
    /// `WorkspaceMember` row for `default_assignee`, when supplied.
    pub default_assignee_is_member: Option<bool>,
}

/// Ports `ProjectCreateSerializer.validate` (`:139-166`): first failure
/// wins, in source order.
pub fn validate_shared(input: &SharedChecks<'_>) -> Result<(), ProjectSerError> {
    Result::<(), ProjectSerError>::from(check_default_agent_executor(
        input.executor,
        input.cloud_configured,
        input.managed_enabled,
    ))?;
    if input.name.is_some_and(contains_forbidden_chars) {
        return Err(ProjectSerError::NameForbidden);
    }
    if input.identifier.is_some_and(contains_forbidden_chars) {
        return Err(ProjectSerError::IdentifierForbidden);
    }
    if input.project_lead_is_member == Some(false) {
        return Err(ProjectSerError::ProjectLeadNotMember);
    }
    if input.default_assignee_is_member == Some(false) {
        return Err(ProjectSerError::DefaultAssigneeNotMember);
    }
    Ok(())
}

/// Ports `ProjectUpdateSerializer.update` (`:214-248`) after the inherited
/// `validate()` already passed. `default_state_in_project` /
/// `estimate_in_project` are the `State`/`Estimate` existence facts, `None`
/// when the key is absent. `is_default` is the requested value.
pub fn check_update_tail(
    instance_is_default: bool,
    default_state_in_project: Option<bool>,
    estimate_in_project: Option<bool>,
    is_default: Option<bool>,
) -> Result<(), ProjectSerError> {
    if default_state_in_project == Some(false) {
        return Err(ProjectSerError::DefaultStateOutsideProject);
    }
    if estimate_in_project == Some(false) {
        return Err(ProjectSerError::EstimateOutsideProject);
    }
    if instance_is_default && is_default == Some(false) {
        return Err(ProjectSerError::UnsetDefault);
    }
    Ok(())
}

/// Full PATCH decision: inherited `validate()` first, then `update()`.
/// Matches the observable behavior (identical messages in both phases, first
/// failure wins across them).
pub fn validate_update(
    shared: &SharedChecks<'_>,
    instance_is_default: bool,
    default_state_in_project: Option<bool>,
    estimate_in_project: Option<bool>,
    is_default: Option<bool>,
) -> Result<(), ProjectSerError> {
    validate_shared(shared)?;
    check_update_tail(
        instance_is_default,
        default_state_in_project,
        estimate_in_project,
        is_default,
    )
}

/// `ProjectSerializer.validate()` tail (`:294-327`) after the shared
/// checks: the instance-default guard, then the `description_html` branch.
/// `instance_is_default` is `None` on the (unreachable from D-19 views)
/// create path, where `self.instance` is `None`.
pub fn validate_read_tail(
    instance_is_default: Option<bool>,
    is_default: Option<bool>,
    description_html: DescriptionHtml<'_>,
) -> Result<Option<String>, ProjectSerError> {
    if instance_is_default == Some(true) && is_default == Some(false) {
        return Err(ProjectSerError::UnsetDefault);
    }
    check_description_html(description_html)
}

/// Requires a non-blank normalised identifier (`:171-172,333-334`).
/// On the D-19 routes the views map this failure to the 409
/// identifier-taken body (see [`ProjectSerError::IdentifierRequired`]).
pub fn require_identifier(normalized: &str) -> Result<(), ProjectSerError> {
    if normalized.is_empty() {
        return Err(ProjectSerError::IdentifierRequired);
    }
    Ok(())
}

/// The `ProjectIdentifier` pre-check (`:174-175,336-337`).
pub fn check_identifier_taken(exists: bool) -> Result<(), ProjectSerError> {
    if exists {
        return Err(ProjectSerError::IdentifierTaken);
    }
    Ok(())
}

/// Whether `create()`/`update()` clears the workspace's other default
/// (`:188-191,242-246`): exactly when the requested `is_default` is true.
/// The clearing itself is a queries-layer write inside `transaction.atomic()`.
pub fn unsets_other_defaults(is_default: Option<bool>) -> bool {
    is_default == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_shared() -> SharedChecks<'static> {
        SharedChecks {
            executor: None,
            cloud_configured: false,
            managed_enabled: false,
            name: None,
            identifier: None,
            project_lead_is_member: None,
            default_assignee_is_member: None,
        }
    }

    #[test]
    fn executor_gate_matches_helper_branches() {
        // Fixture `executor_cloud_unconfigured`: 409 with its own body.
        assert_eq!(
            check_default_agent_executor(Some("cloud_agent"), false, false),
            ExecutorCheck::CloudUnavailable
        );
        let err = ProjectSerError::CloudAgentUnavailable;
        assert_eq!(err.status(), CLOUD_AGENT_UNAVAILABLE_STATUS);
        assert_eq!(err.status(), 409);
        assert_eq!(
            err.body(),
            r#"{"error":"Pi Dash Cloud Agent is not currently available","code":"cloud_agent_unavailable"}"#
        );
        // Configured cloud passes.
        assert_eq!(
            check_default_agent_executor(Some("cloud_agent"), true, false),
            ExecutorCheck::Ok
        );
        // Fixture `executor_managed_disabled`: 400 list-wrapped body.
        assert_eq!(
            check_default_agent_executor(Some("managed_runner"), false, false),
            ExecutorCheck::ManagedDisabled
        );
        let err = ProjectSerError::ManagedRunnerDisabled;
        assert_eq!(err.status(), 400);
        assert_eq!(
            err.body(),
            r#"{"default_agent_executor":["Pi Dash Agent is not enabled on this instance"]}"#
        );
        assert_eq!(
            check_default_agent_executor(Some("managed_runner"), false, true),
            ExecutorCheck::Ok
        );
        // Absent and ungated kinds pass.
        assert_eq!(
            check_default_agent_executor(None, false, false),
            ExecutorCheck::Ok
        );
        assert_eq!(
            check_default_agent_executor(Some("local_runner"), false, false),
            ExecutorCheck::Ok
        );
        assert_eq!(
            check_default_agent_executor(Some("other"), false, false),
            ExecutorCheck::Ok
        );
    }

    #[test]
    fn forbidden_table_matches_re_match_including_newline_quirk() {
        assert_eq!(FORBIDDEN_CHARS.len(), 24);
        // Every table char is rejected on a single line.
        for c in FORBIDDEN_CHARS {
            assert!(
                contains_forbidden_chars(&format!("ab{c}cd")),
                "char {c:?} should be forbidden"
            );
        }
        assert!(!contains_forbidden_chars("Engine"));
        assert!(!contains_forbidden_chars("ENG"));
        assert!(!contains_forbidden_chars(""));
        assert!(!contains_forbidden_chars("plain name 123_"));
        assert!(contains_forbidden_chars("plain-name"));
        // Interior newline: Python `re.match` can never match (`.` does not
        // cross `\n`, `$` anchors at the end), so these pass.
        assert!(!contains_forbidden_chars("a&b\nc"));
        assert!(!contains_forbidden_chars("a\nb&c"));
        assert!(!contains_forbidden_chars("&\n&"));
        assert!(!contains_forbidden_chars("ok\n&x"));
        // Trailing newline still anchors `$`: rejected.
        assert!(contains_forbidden_chars("a&b\n"));
        // `\r` is matched by `.`: rejected.
        assert!(contains_forbidden_chars("a\rb&c"));
    }

    #[test]
    fn identifier_normalisation_strips_and_uppercases() {
        // Fixture `create_ok_shape`: validated_data keeps ' eng ', the row
        // stores 'ENG'.
        assert_eq!(normalize_identifier(" eng "), "ENG");
        assert_eq!(normalize_identifier("eng"), "ENG");
        assert_eq!(normalize_identifier("ENG"), "ENG");
        assert!(require_identifier(&normalize_identifier(" eng ")).is_ok());
        // Blank (even whitespace-only) fails the empty check; the views map
        // it to the 409 identifier-taken body (ported quirk).
        assert_eq!(
            require_identifier(&normalize_identifier("   ")),
            Err(ProjectSerError::IdentifierRequired)
        );
        let err = ProjectSerError::IdentifierRequired;
        assert_eq!(err.status(), 409);
        assert_eq!(
            err.body(),
            r#"{"identifier":"The project identifier is already taken"}"#
        );
        // Pre-check hit maps to the same 409 body.
        assert_eq!(
            check_identifier_taken(true),
            Err(ProjectSerError::IdentifierTaken)
        );
        assert!(check_identifier_taken(false).is_ok());
        assert_eq!(IDENTIFIER_REQUIRED_DETAIL, "Project Identifier is required");
        assert_eq!(IDENTIFIER_TAKEN_DETAIL, "Project Identifier is taken");
    }

    #[test]
    fn logo_default_shape_renders_in_drf_key_order() {
        let doc = logo_props_default("rocket", "#5e6ad2");
        let bytes = serde_json::to_string(&doc).expect("serializes");
        assert_eq!(
            bytes,
            r##"{"in_use":"icon","icon":{"name":"rocket","color":"#5e6ad2"}}"##
        );
        assert_eq!(PROJECT_ICON_DEFAULT_COLORS.len(), 8);
        assert_eq!(PROJECT_ICON_DEFAULT_ICONS.len(), 28);
        assert!(PROJECT_ICON_DEFAULT_COLORS.contains(&"#5e6ad2"));
        assert!(PROJECT_ICON_DEFAULT_ICONS.contains(&"rocket"));
    }

    #[test]
    fn create_validate_matches_fixture_goldens() {
        // `create_invalid_name_chars`.
        let mut input = quiet_shared();
        input.name = Some("Bad & Name");
        input.identifier = Some("ENG");
        assert_eq!(validate_shared(&input), Err(ProjectSerError::NameForbidden));
        assert_eq!(
            ProjectSerError::NameForbidden.body(),
            r#"{"non_field_errors":["Project name cannot contain special characters."]}"#
        );
        assert_eq!(ProjectSerError::NameForbidden.status(), 400);
        // `create_invalid_identifier_chars`.
        let mut input = quiet_shared();
        input.name = Some("Ok Name");
        input.identifier = Some("BAD slug!");
        assert_eq!(
            validate_shared(&input),
            Err(ProjectSerError::IdentifierForbidden)
        );
        assert_eq!(
            ProjectSerError::IdentifierForbidden.body(),
            r#"{"non_field_errors":["Project identifier cannot contain special characters."]}"#
        );
        // `create_non_workspace_lead` (+ the default-assignee mirror).
        let mut input = quiet_shared();
        input.name = Some("N");
        input.identifier = Some("ENG");
        input.project_lead_is_member = Some(false);
        assert_eq!(
            validate_shared(&input),
            Err(ProjectSerError::ProjectLeadNotMember)
        );
        assert_eq!(
            ProjectSerError::ProjectLeadNotMember.body(),
            r#"{"non_field_errors":["Project lead should be a user in the workspace"]}"#
        );
        input.project_lead_is_member = Some(true);
        input.default_assignee_is_member = Some(false);
        assert_eq!(
            validate_shared(&input),
            Err(ProjectSerError::DefaultAssigneeNotMember)
        );
        assert_eq!(
            ProjectSerError::DefaultAssigneeNotMember.body(),
            r#"{"non_field_errors":["Default assignee should be a user in the workspace"]}"#
        );
        // `create_ok_shape` inputs pass validation.
        let mut input = quiet_shared();
        input.name = Some("Engine");
        input.identifier = Some(" eng ");
        input.cloud_configured = true;
        input.managed_enabled = true;
        assert!(validate_shared(&input).is_ok());
        // First failure wins, in source order: name beats lead.
        let mut input = quiet_shared();
        input.name = Some("Bad & Name");
        input.project_lead_is_member = Some(false);
        assert_eq!(validate_shared(&input), Err(ProjectSerError::NameForbidden));
        // Executor gate runs before the name check.
        let mut input = quiet_shared();
        input.executor = Some("cloud_agent");
        input.name = Some("Bad & Name");
        assert_eq!(
            validate_shared(&input),
            Err(ProjectSerError::CloudAgentUnavailable)
        );
        // Absent keys are skipped (partial-update parity).
        assert!(validate_shared(&quiet_shared()).is_ok());
    }

    #[test]
    fn update_tail_matches_fixture_goldens() {
        // `update_default_state_outside_project`.
        assert_eq!(
            check_update_tail(true, Some(false), None, None),
            Err(ProjectSerError::DefaultStateOutsideProject)
        );
        assert_eq!(
            ProjectSerError::DefaultStateOutsideProject.body(),
            r#"{"non_field_errors":["Default state should be a state in the project"]}"#
        );
        // `update_estimate_outside_project` (verbatim "a estimate").
        assert_eq!(
            check_update_tail(false, None, Some(false), None),
            Err(ProjectSerError::EstimateOutsideProject)
        );
        assert_eq!(
            ProjectSerError::EstimateOutsideProject.body(),
            r#"{"non_field_errors":["Estimate should be a estimate in the project"]}"#
        );
        // `update_unset_default`.
        assert_eq!(
            check_update_tail(true, None, None, Some(false)),
            Err(ProjectSerError::UnsetDefault)
        );
        assert_eq!(
            ProjectSerError::UnsetDefault.body(),
            r#"{"non_field_errors":["Default project cannot be unset without assigning another default project."]}"#
        );
        // Non-default instances may pass `is_default: false`.
        assert!(check_update_tail(false, None, None, Some(false)).is_ok());
        // Absent keys pass; `is_default: true` arms the atomic unset.
        assert!(check_update_tail(false, None, None, None).is_ok());
        assert!(unsets_other_defaults(Some(true)));
        assert!(!unsets_other_defaults(Some(false)));
        assert!(!unsets_other_defaults(None));
        // Tail order: default-state beats estimate beats unset.
        assert_eq!(
            check_update_tail(true, Some(false), Some(false), Some(false)),
            Err(ProjectSerError::DefaultStateOutsideProject)
        );
        // Full PATCH: shared validate runs before the tail.
        let mut shared = quiet_shared();
        shared.name = Some("Bad & Name");
        assert_eq!(
            validate_update(&shared, true, Some(false), None, None),
            Err(ProjectSerError::NameForbidden)
        );
        let shared = quiet_shared();
        assert_eq!(
            validate_update(&shared, true, Some(false), None, None),
            Err(ProjectSerError::DefaultStateOutsideProject)
        );
    }

    #[test]
    fn read_tail_matches_html_branch_and_unset_guard() {
        // Instance-default guard.
        assert_eq!(
            validate_read_tail(Some(true), Some(false), DescriptionHtml::MissingOrFalsy),
            Err(ProjectSerError::UnsetDefault)
        );
        // No instance (create path): guard skipped.
        assert!(validate_read_tail(None, Some(false), DescriptionHtml::MissingOrFalsy).is_ok());
        // Absent/falsy description_html: untouched.
        assert_eq!(
            validate_read_tail(None, None, DescriptionHtml::MissingOrFalsy),
            Ok(None)
        );
        // Valid dict with sanitised replacement: written back.
        assert_eq!(
            validate_read_tail(
                None,
                None,
                DescriptionHtml::Dict {
                    is_valid: true,
                    sanitized: Some("<p>clean</p>"),
                }
            ),
            Ok(Some("<p>clean</p>".to_string()))
        );
        // Valid dict, sanitiser returned nothing: untouched.
        assert_eq!(
            validate_read_tail(
                None,
                None,
                DescriptionHtml::Dict {
                    is_valid: true,
                    sanitized: None,
                }
            ),
            Ok(None)
        );
        // Invalid dict: 400 with the list-wrapped body.
        assert_eq!(
            validate_read_tail(
                None,
                None,
                DescriptionHtml::Dict {
                    is_valid: false,
                    sanitized: None,
                }
            ),
            Err(ProjectSerError::HtmlInvalid)
        );
        assert_eq!(
            ProjectSerError::HtmlInvalid.body(),
            r#"{"error":["html content is not valid"]}"#
        );
        // Ported bug: truthy non-dict hits unbound `is_valid` -> 500.
        assert_eq!(
            validate_read_tail(None, None, DescriptionHtml::OtherTruthy),
            Err(ProjectSerError::DescriptionHtmlNonDict)
        );
        let err = ProjectSerError::DescriptionHtmlNonDict;
        assert_eq!(err.status(), 500);
        assert_eq!(
            err.body(),
            r#"{"error":"Something went wrong please try again later"}"#
        );
        // Guard runs before the html branch.
        assert_eq!(
            validate_read_tail(
                Some(true),
                Some(false),
                DescriptionHtml::Dict {
                    is_valid: false,
                    sanitized: None,
                }
            ),
            Err(ProjectSerError::UnsetDefault)
        );
    }

    #[test]
    fn view_conflict_bodies_match_handler_fixture() {
        assert_eq!(
            IDENTIFIER_TAKEN_BODY,
            r#"{"identifier":"The project identifier is already taken"}"#
        );
        assert_eq!(
            NAME_TAKEN_BODY,
            r#"{"name":"The project name is already taken"}"#
        );
        assert_eq!(
            IDENTIFIER_FIELD_REQUIRED_BODY,
            r#"{"identifier":["This field is required."]}"#
        );
    }

    #[test]
    fn shape_consts_match_python_source() {
        assert_eq!(CREATE_META_FIELDS.len(), 26);
        assert_eq!(CREATE_META_FIELDS[0], "name");
        assert_eq!(CREATE_META_FIELDS[25], "default_agent_executor");
        assert!(CREATE_META_FIELDS.contains(&"identifier"));
        assert_eq!(
            CREATE_META_READ_ONLY_FIELDS,
            [
                "id",
                "workspace",
                "created_at",
                "updated_at",
                "created_by",
                "updated_by",
                "logo_props"
            ]
        );
        assert_eq!(UPDATE_EXTRA_FIELDS, ["default_state", "estimate"]);
        assert_eq!(
            READ_ANNOTATION_FIELDS,
            [
                "total_members",
                "total_cycles",
                "total_modules",
                "is_member",
                "sort_order",
                "member_role",
                "is_deployed",
                "cover_image_url"
            ]
        );
        assert_eq!(READ_READ_ONLY_FIELDS.len(), 9);
        assert!(READ_READ_ONLY_FIELDS.contains(&"cover_image_url"));
        assert_eq!(
            LITE_FIELDS,
            [
                "id",
                "identifier",
                "name",
                "cover_image",
                "icon_prop",
                "emoji",
                "description",
                "is_default",
                "cover_image_url"
            ]
        );
    }
}
