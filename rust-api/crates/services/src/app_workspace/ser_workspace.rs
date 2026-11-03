//! Workspace + member serializers: `WorkSpace*` read/write shapes (D-24).
//!
//! Port of `apps/api/pi_dash/app/serializers/workspace.py:43-108`:
//!
//! * `WorkSpaceSerializer` (`:43-77`, `validate_name` / `validate_slug`,
//!   read-only `total_members` / `logo_url` / `role`)
//! * `WorkspaceLiteSerializer` (`:79-84`)
//! * `WorkSpaceMemberSerializer` (`:86-92`)
//! * `WorkspaceMemberMeSerializer` (`:94-100`)
//! * `WorkspaceMemberAdminSerializer` (`:102-108`)
//!
//! Nested shapes consumed verbatim:
//!
//! * `UserLiteSerializer` (`app/serializers/user.py:141-154`) and
//!   `UserAdminLiteSerializer` (`user.py:156-171`) — owned by SER-D
//!   (PIDASHCONV-603, `super::ser_user`); referenced here by name only.
//!   The member views are generic over the rendered member value so
//!   handlers compose this kernel with SER-D's renderers.
//!
//! These are pure kernels in the `v1_projects::ser_collab` style: each
//! `to_representation` takes a row borrowed from the caller and returns a
//! `serde::Serialize` view whose fields are the live DRF wire fields in
//! output order. UUID and FK primary keys render as strings
//! (`PrimaryKeyRelatedField`, read-only); a null FK renders `null`.
//! Datetimes cross this boundary already rendered as DRF iso-8601 strings
//! (formatting owns to the DB edge), so rendering here is a byte-exact
//! passthrough. JSON blobs (`view_props`, `default_props`, `issue_props`,
//! `getting_started_checklist`, `tips`, `explored_features`) pass through
//! by reference.
//!
//! Single-owner notes (never fork a helper):
//!
//! * `RESTRICTED_WORKSPACE_SLUGS` is reused from
//!   `pidash_types::license::serializers_workspace` (verbatim port of
//!   `pi_dash/utils/constants.py:5-71`, duplicates included), not
//!   redefined. Only the list is shared: the app `validate_slug` has no
//!   `iexact` taken-branch and adds the charset rule, so it keeps its own
//!   error type.
//! * [`contains_url`] is the shared kernel port of
//!   `pi_dash/utils/url.py:26-53`; SER-D (PIDASHCONV-603) reuses it for
//!   the `UserSerializer` first/last-name guards instead of redefining it.
//! * `logo_url` arrives already resolved (PIDASHCONV-605 owns the model
//!   property port); this module only places the value.
//! * `WorkspaceLite*` here is the canonical port; the `app_project`
//!   nested copy (`app_project::ser_member`, defined from FX-APROJ-02
//!   before D-24 landed) absorbs this one per its own single-owner note —
//!   that reconciliation lives with D-25, not this issue.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-fields (`app/serializers/base.py:16-18`):
//!   `DynamicBaseSerializer` pops `fields` and then overwrites it with
//!   `expand`, so the `fields=` kwarg is silently ignored. The member
//!   list/retrieve views (`app/views/workspace/member.py:52,54,71,73`)
//!   pass `fields=("id", "member", "role")` but render the full 17-key
//!   shape. [`member_fields_to_representation`] and
//!   [`member_admin_fields_to_representation`] take the same argument and
//!   ignore it.
//! * BUG-newline (`app/serializers/workspace.py:59`): `validate_slug`
//!   uses `re.match(r"^[a-zA-Z0-9_-]+$", v)` and Python `$` matches
//!   before a trailing newline, so `"abc\n"` PASSES. [`SLUG_RE`] carries
//!   an explicit `\n?` because Rust `$` matches end-of-haystack only
//!   (verified empirically; `"abc\n\n"` still fails on both sides).
//!
//! Out of scope (documented, not ported): `DynamicBaseSerializer`
//! `expand=` (`base.py:122-200`) — no D-24 call site passes `expand` to
//! these serializers and no fixture covers it; the DRF `{"field":
//! ["message"]}` error envelope — the handler arm; this kernel returns
//! the message via `Display`.

use pidash_types::license::serializers_workspace::RESTRICTED_WORKSPACE_SLUGS;
use serde::Serialize;
use std::sync::LazyLock;
use thiserror::Error as ThisError;

/// `WorkSpaceSerializer` wire keys in output order (`workspace.py:43-77`,
/// `fields = "__all__"`: pk, declared `total_members` / `logo_url` /
/// `role`, then concrete model fields, then forward relations —
/// verified against the live serializer).
pub const WORKSPACE_WIRE_FIELDS: [&str; 17] = [
    "id",
    "total_members",
    "logo_url",
    "role",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "logo",
    "slug",
    "organization_size",
    "timezone",
    "background_color",
    "created_by",
    "updated_by",
    "logo_asset",
    "owner",
];

/// `WorkSpaceSerializer.Meta.read_only_fields` (`workspace.py:68-76`).
pub const WORKSPACE_READ_ONLY_FIELDS: [&str; 7] = [
    "id",
    "created_by",
    "updated_by",
    "created_at",
    "updated_at",
    "owner",
    "logo_url",
];

/// `WorkspaceLiteSerializer.Meta.fields` (`workspace.py:82`), wire order.
pub const WORKSPACE_LITE_WIRE_FIELDS: [&str; 4] = ["name", "slug", "id", "logo_url"];

/// `WorkSpaceMemberSerializer` wire keys in output order
/// (`workspace.py:86-92`, `fields = "__all__"`: pk, declared nested
/// `member`, then concrete model fields, then forward relations —
/// verified against the live serializer).
pub const WORKSPACE_MEMBER_WIRE_FIELDS: [&str; 17] = [
    "id",
    "member",
    "created_at",
    "updated_at",
    "deleted_at",
    "role",
    "company_role",
    "view_props",
    "default_props",
    "issue_props",
    "is_active",
    "getting_started_checklist",
    "tips",
    "explored_features",
    "created_by",
    "updated_by",
    "workspace",
];

/// `WorkspaceMemberAdminSerializer` wire keys (`workspace.py:102-108`):
/// identical to [`WORKSPACE_MEMBER_WIRE_FIELDS`]; only the nested
/// `member` shape differs (admin-lite).
pub const WORKSPACE_MEMBER_ADMIN_WIRE_FIELDS: [&str; 17] = WORKSPACE_MEMBER_WIRE_FIELDS;

/// `WorkspaceMemberMeSerializer` wire keys in output order
/// (`workspace.py:94-100`, `fields = "__all__"`: pk, declared
/// `draft_issue_count`, then concrete model fields, then forward
/// relations `workspace` / `member` as PK strings — verified against the
/// live serializer).
pub const WORKSPACE_MEMBER_ME_WIRE_FIELDS: [&str; 18] = [
    "id",
    "draft_issue_count",
    "created_at",
    "updated_at",
    "deleted_at",
    "role",
    "company_role",
    "view_props",
    "default_props",
    "issue_props",
    "is_active",
    "getting_started_checklist",
    "tips",
    "explored_features",
    "created_by",
    "updated_by",
    "workspace",
    "member",
];

/// Port of `URL_PATTERN` (`pi_dash/utils/url.py:13-25`): `http(s)://`
/// URLs, `www.` hosts, bare `host.tld` names, and IPv4 literals. Every
/// construct has a direct `regex`-crate equivalent; `(?i)` matches
/// Python's case-insensitive flag.
static URL_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(?:https?://\S+|www\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*|(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)+[a-zA-Z]{2,6}|(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?))",
    )
    .expect("URL pattern compiles")
});

/// Port of the `WorkSpaceSerializer.validate_slug` charset rule
/// (`workspace.py:59`) plus BUG-newline: Python `$` matches before a
/// trailing newline, Rust `$` does not, so the quirk needs an explicit
/// `\n?` (`"abc\n"` passes, `"abc\n\n"` fails — both sides).
static SLUG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z0-9_-]+\n?$").expect("slug pattern compiles"));

/// Truncate to at most `max_chars` code points without splitting UTF-8
/// (`line[:500]` in `url.py:47-48` counts code points).
fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Port of `contains_url` (`pi_dash/utils/url.py:26-53`): values over
/// 1000 code points never match; each `\n`-separated line is truncated
/// to 500 code points before the pattern search. Shared kernel — SER-D
/// (PIDASHCONV-603) reuses this for the `UserSerializer` name guards.
pub fn contains_url(value: &str) -> bool {
    // Python len() counts code points (url.py:41); byte length would
    // over-count non-ASCII input.
    if value.chars().count() > 1000 {
        return false;
    }
    for line in value.split('\n') {
        if URL_RE.is_match(truncate_chars(line, 500)) {
            return true;
        }
    }
    false
}

/// `WorkSpaceSerializer.validate_name` failure (`workspace.py:48-52`).
///
/// `Display` renders the exact DRF `ValidationError` message, which DRF
/// envelopes as `{"name": ["<message>"]}` in `serializer.errors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
pub enum NameError {
    /// The name contains a URL (`workspace.py:51`).
    #[error("Name must not contain URLs")]
    ContainsUrl,
}

/// Port of `WorkSpaceSerializer.validate_name` (`workspace.py:48-52`).
pub fn validate_name(value: &str) -> Result<&str, NameError> {
    if contains_url(value) {
        return Err(NameError::ContainsUrl);
    }
    Ok(value)
}

/// `WorkSpaceSerializer.validate_slug` failures (`workspace.py:54-63`).
///
/// `Display` renders the exact DRF `ValidationError` message, which DRF
/// envelopes as `{"slug": ["<message>"]}` in `serializer.errors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ThisError)]
pub enum SlugError {
    /// Slug is on the restricted list (`workspace.py:56-57`).
    #[error("Slug is not valid")]
    Restricted,
    /// Slug has characters outside `[a-zA-Z0-9_-]` (`workspace.py:59-62`).
    #[error("Slug can only contain letters, numbers, hyphens (-), and underscores (_)")]
    BadChars,
}

/// Port of `WorkSpaceSerializer.validate_slug` (`workspace.py:54-63`):
/// the restricted-list check runs first, exactly as in Python. The
/// restricted test is a case-sensitive exact match
/// (`value in RESTRICTED_WORKSPACE_SLUGS`).
pub fn validate_slug(value: &str) -> Result<&str, SlugError> {
    if RESTRICTED_WORKSPACE_SLUGS.contains(&value) {
        return Err(SlugError::Restricted);
    }
    if !SLUG_RE.is_match(value) {
        return Err(SlugError::BadChars);
    }
    Ok(value)
}

/// A `Workspace` row for [`workspace_to_representation`]
/// (`db/models/workspace.py:119-139`): `total_members` / `role` are
/// annotation-fed (`None` = unannotated, absent from the output —
/// DRF `SkipField`; `Some(None)` renders `null`); `logo_url` is the
/// resolved model property (PIDASHCONV-605 owns the resolution); `owner`
/// is a non-nullable FK rendering as a UUID string; `logo` /
/// `organization_size` (`CharField(null=True)`) and the nullable
/// `created_by` / `updated_by` / `logo_asset` FKs render `null` when
/// unset; datetimes are pre-rendered DRF strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRow<'a> {
    pub id: &'a str,
    pub total_members: Option<Option<i64>>,
    pub logo_url: Option<&'a str>,
    pub role: Option<Option<i64>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub logo: Option<&'a str>,
    pub slug: &'a str,
    pub organization_size: Option<&'a str>,
    pub timezone: &'a str,
    pub background_color: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub logo_asset: Option<&'a str>,
    pub owner: &'a str,
}

/// `WorkSpaceSerializer.to_representation` output (`workspace.py:43-77`),
/// in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceView<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_members: Option<Option<i64>>,
    pub logo_url: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Option<i64>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub name: &'a str,
    pub logo: Option<&'a str>,
    pub slug: &'a str,
    pub organization_size: Option<&'a str>,
    pub timezone: &'a str,
    pub background_color: &'a str,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub logo_asset: Option<&'a str>,
    pub owner: &'a str,
}

/// Port of `WorkSpaceSerializer` (`workspace.py:43-77`) read shape.
pub fn workspace_to_representation<'a>(row: &'a WorkspaceRow<'a>) -> WorkspaceView<'a> {
    WorkspaceView {
        id: row.id,
        total_members: row.total_members,
        logo_url: row.logo_url,
        role: row.role,
        created_at: row.created_at,
        updated_at: row.updated_at,
        deleted_at: row.deleted_at,
        name: row.name,
        logo: row.logo,
        slug: row.slug,
        organization_size: row.organization_size,
        timezone: row.timezone,
        background_color: row.background_color,
        created_by: row.created_by,
        updated_by: row.updated_by,
        logo_asset: row.logo_asset,
        owner: row.owner,
    }
}

/// A `Workspace` row for lite rendering (`workspace.py:79-84`): `name`
/// / `slug` are non-null (`db/models/workspace.py:122,136`); `logo_url`
/// is the resolved model property (PIDASHCONV-605 owns the resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLiteRow<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
    pub logo_url: Option<&'a str>,
}

/// `WorkspaceLiteSerializer.to_representation` output, in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceLiteView<'a> {
    pub name: &'a str,
    pub slug: &'a str,
    pub id: &'a str,
    pub logo_url: Option<&'a str>,
}

/// Port of `WorkspaceLiteSerializer` (`workspace.py:79-84`).
pub fn workspace_lite_to_representation<'a>(
    row: &'a WorkspaceLiteRow<'a>,
) -> WorkspaceLiteView<'a> {
    WorkspaceLiteView {
        name: row.name,
        slug: row.slug,
        id: row.id,
        logo_url: row.logo_url,
    }
}

/// The non-relation `WorkspaceMember` columns shared by the member,
/// admin, and me shapes (`db/models/workspace.py:198-213`): `role`
/// (`PositiveSmallIntegerField`, 20/15/5), nullable `company_role`
/// (`TextField(null=True)`), JSON `view_props` / `default_props` /
/// `issue_props` / `getting_started_checklist` / `tips` /
/// `explored_features`, `is_active`, nullable `created_by` /
/// `updated_by` FKs (render as UUID strings, `null` when unset), and the
/// `created_at` / `updated_at` / nullable `deleted_at` datetimes
/// (pre-rendered DRF strings, byte-exact passthrough).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMemberCore<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i64,
    pub company_role: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub issue_props: &'a serde_json::Value,
    pub is_active: bool,
    pub getting_started_checklist: &'a serde_json::Value,
    pub tips: &'a serde_json::Value,
    pub explored_features: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

/// A `WorkspaceMember` row for [`member_to_representation`]: the
/// non-nullable `workspace` FK renders as a PK string; `member` is the
/// already-rendered nested value (SER-D's `UserLiteView`, composed by
/// the handler — the `member` FK is non-nullable,
/// `db/models/workspace.py:200-204`, so it always renders an object).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkSpaceMemberRow<'a, M> {
    pub core: WorkspaceMemberCore<'a>,
    pub workspace: &'a str,
    pub member: M,
}

/// `WorkSpaceMemberSerializer.to_representation` output
/// (`workspace.py:86-92`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkSpaceMemberView<'a, M> {
    pub id: &'a str,
    pub member: &'a M,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i64,
    pub company_role: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub issue_props: &'a serde_json::Value,
    pub is_active: bool,
    pub getting_started_checklist: &'a serde_json::Value,
    pub tips: &'a serde_json::Value,
    pub explored_features: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
}

/// Port of `WorkSpaceMemberSerializer` (`workspace.py:86-92`).
pub fn member_to_representation<'a, M>(
    row: &'a WorkSpaceMemberRow<'a, M>,
) -> WorkSpaceMemberView<'a, M> {
    let core = &row.core;
    WorkSpaceMemberView {
        id: core.id,
        member: &row.member,
        created_at: core.created_at,
        updated_at: core.updated_at,
        deleted_at: core.deleted_at,
        role: core.role,
        company_role: core.company_role,
        view_props: core.view_props,
        default_props: core.default_props,
        issue_props: core.issue_props,
        is_active: core.is_active,
        getting_started_checklist: core.getting_started_checklist,
        tips: core.tips,
        explored_features: core.explored_features,
        created_by: core.created_by,
        updated_by: core.updated_by,
        workspace: row.workspace,
    }
}

/// Port of the `fields=` call sites (`app/views/workspace/member.py:52,
/// 54,71,73`): the views pass `fields=("id", "member", "role")`, but
/// `DynamicBaseSerializer.__init__` overwrites `fields` with `expand`
/// (`base.py:16-18`), so the argument is silently ignored and the full
/// 17-key shape renders. This kernel takes the same argument and ignores
/// it — BUG-fields, ported as-is.
pub fn member_fields_to_representation<'a, M>(
    row: &'a WorkSpaceMemberRow<'a, M>,
    _fields: &[&str],
) -> WorkSpaceMemberView<'a, M> {
    member_to_representation(row)
}

/// A `WorkspaceMember` row for [`member_admin_to_representation`]: same
/// as [`WorkSpaceMemberRow`] with the admin-lite member shape (SER-D's
/// `UserAdminLiteView`, composed by the handler).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkSpaceMemberAdminRow<'a, M> {
    pub core: WorkspaceMemberCore<'a>,
    pub workspace: &'a str,
    pub member: M,
}

/// `WorkspaceMemberAdminSerializer.to_representation` output
/// (`workspace.py:102-108`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkSpaceMemberAdminView<'a, M> {
    pub id: &'a str,
    pub member: &'a M,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i64,
    pub company_role: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub issue_props: &'a serde_json::Value,
    pub is_active: bool,
    pub getting_started_checklist: &'a serde_json::Value,
    pub tips: &'a serde_json::Value,
    pub explored_features: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
}

/// Port of `WorkspaceMemberAdminSerializer` (`workspace.py:102-108`).
pub fn member_admin_to_representation<'a, M>(
    row: &'a WorkSpaceMemberAdminRow<'a, M>,
) -> WorkSpaceMemberAdminView<'a, M> {
    let core = &row.core;
    WorkSpaceMemberAdminView {
        id: core.id,
        member: &row.member,
        created_at: core.created_at,
        updated_at: core.updated_at,
        deleted_at: core.deleted_at,
        role: core.role,
        company_role: core.company_role,
        view_props: core.view_props,
        default_props: core.default_props,
        issue_props: core.issue_props,
        is_active: core.is_active,
        getting_started_checklist: core.getting_started_checklist,
        tips: core.tips,
        explored_features: core.explored_features,
        created_by: core.created_by,
        updated_by: core.updated_by,
        workspace: row.workspace,
    }
}

/// Port of the `fields=` call sites for the admin shape
/// (`app/views/workspace/member.py:52,71`): same BUG-fields as
/// [`member_fields_to_representation`] — the argument is ignored and
/// the full 17-key shape renders.
pub fn member_admin_fields_to_representation<'a, M>(
    row: &'a WorkSpaceMemberAdminRow<'a, M>,
    _fields: &[&str],
) -> WorkSpaceMemberAdminView<'a, M> {
    member_admin_to_representation(row)
}

/// A `WorkspaceMember` row for [`member_me_to_representation`]:
/// `draft_issue_count` is annotation-fed (`None` = unannotated, absent
/// from the output — DRF `SkipField`; `Some(None)` renders `null`);
/// the non-nullable `workspace` / `member` FKs render as PK strings.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceMemberMeRow<'a> {
    pub core: WorkspaceMemberCore<'a>,
    pub draft_issue_count: Option<Option<i64>>,
    pub workspace: &'a str,
    pub member: &'a str,
}

/// `WorkspaceMemberMeSerializer.to_representation` output
/// (`workspace.py:94-100`), in wire order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceMemberMeView<'a> {
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_issue_count: Option<Option<i64>>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub deleted_at: Option<&'a str>,
    pub role: i64,
    pub company_role: Option<&'a str>,
    pub view_props: &'a serde_json::Value,
    pub default_props: &'a serde_json::Value,
    pub issue_props: &'a serde_json::Value,
    pub is_active: bool,
    pub getting_started_checklist: &'a serde_json::Value,
    pub tips: &'a serde_json::Value,
    pub explored_features: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
    pub workspace: &'a str,
    pub member: &'a str,
}

/// Port of `WorkspaceMemberMeSerializer` (`workspace.py:94-100`).
pub fn member_me_to_representation<'a>(
    row: &'a WorkspaceMemberMeRow<'a>,
) -> WorkspaceMemberMeView<'a> {
    let core = &row.core;
    WorkspaceMemberMeView {
        id: core.id,
        draft_issue_count: row.draft_issue_count,
        created_at: core.created_at,
        updated_at: core.updated_at,
        deleted_at: core.deleted_at,
        role: core.role,
        company_role: core.company_role,
        view_props: core.view_props,
        default_props: core.default_props,
        issue_props: core.issue_props,
        is_active: core.is_active,
        getting_started_checklist: core.getting_started_checklist,
        tips: core.tips,
        explored_features: core.explored_features,
        created_by: core.created_by,
        updated_by: core.updated_by,
        workspace: row.workspace,
        member: row.member,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// F-W24-01 golden
    /// (`rust-api/fixtures/app_workspace/serializers/workspace_core.golden.json`),
    /// the Done-when oracle for this module.
    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_workspace/serializers/workspace_core.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture exists"))
            .expect("fixture parses")
    }

    fn case<'a>(fixture: &'a Value, name: &str) -> &'a Value {
        fixture["cases"]
            .as_array()
            .expect("cases array")
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("fixture lacks case {name}"))
    }

    fn str_list(value: &Value) -> Vec<String> {
        value
            .as_array()
            .expect("string array")
            .iter()
            .map(|s| s.as_str().expect("string").to_string())
            .collect()
    }

    fn const_list<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fixture_cases_drive_validators() {
        // Every validator case in F-W24-01 passes against this module.
        let golden = fixture();
        let name_case = case(&golden, "validate_name rejects URL");
        let err = validate_name(name_case["input"]["name"].as_str().expect("input name"))
            .expect_err("name with URL is rejected");
        assert_eq!(err.to_string(), name_case["error"].as_str().expect("error"));

        let slug_case = case(&golden, "validate_slug rejects restricted slug");
        let err = validate_slug(slug_case["input"]["slug"].as_str().expect("input slug"))
            .expect_err("restricted slug is rejected");
        assert_eq!(err.to_string(), slug_case["error"].as_str().expect("error"));

        let chars_case = case(&golden, "validate_slug rejects bad chars");
        let err = validate_slug(chars_case["input"]["slug"].as_str().expect("input slug"))
            .expect_err("bad-chars slug is rejected");
        assert_eq!(
            err.to_string(),
            chars_case["error"].as_str().expect("error")
        );

        let accept_case = case(&golden, "validate_slug accepts");
        let output = accept_case["output"].as_str().expect("output");
        let slug = output.strip_suffix(" OK").expect("accept shorthand");
        assert_eq!(validate_slug(slug), Ok(slug));
    }

    #[test]
    fn read_only_and_lite_fields_match_fixture() {
        let golden = fixture();
        let serializers = &golden["serializers"];
        assert_eq!(
            const_list(&WORKSPACE_READ_ONLY_FIELDS),
            str_list(&serializers["WorkSpaceSerializer"]["read_only"]),
            "read_only list"
        );
        assert_eq!(
            const_list(&WORKSPACE_LITE_WIRE_FIELDS),
            str_list(&serializers["WorkspaceLiteSerializer"]["fields"]),
            "lite fields"
        );
        // The __all__ serializers carry no key arrays in the fixture; their
        // shapes are pinned by the live-bytes replay tests below.
        for name in [
            "WorkSpaceSerializer",
            "WorkSpaceMemberSerializer",
            "WorkspaceMemberMeSerializer",
            "WorkspaceMemberAdminSerializer",
        ] {
            assert_eq!(
                serializers[name]["fields"].as_str(),
                Some("__all__"),
                "{name} fields"
            );
        }
    }

    #[test]
    fn restricted_list_matches_fixture_verbatim() {
        // The fixture records the Python list as one string
        // ("utils/constants.py:5-71: [...]"); it must equal the reused
        // const entry for entry — order, contents, and duplicates.
        let golden = fixture();
        let recorded = golden["restricted_slugs"]
            .as_str()
            .expect("restricted_slugs");
        let start = recorded.find('[').expect("list opens") + 1;
        let end = recorded.rfind(']').expect("list closes");
        let parsed: Vec<&str> = recorded[start..end].split(',').collect();
        assert_eq!(parsed, RESTRICTED_WORKSPACE_SLUGS.to_vec());
        assert_eq!(RESTRICTED_WORKSPACE_SLUGS.len(), 65);
        for slug in &parsed {
            assert_eq!(
                validate_slug(slug),
                Err(SlugError::Restricted),
                "{slug} is restricted"
            );
        }
    }

    #[test]
    fn slug_newline_quirk_matches_python() {
        // BUG-newline (workspace.py:59): Python `$` matches before a
        // trailing newline. Both sides accept "abc\n" and reject "abc\n\n".
        assert_eq!(validate_slug("abc\n"), Ok("abc\n"));
        assert_eq!(validate_slug("abc\n\n"), Err(SlugError::BadChars));
        assert_eq!(validate_slug(""), Err(SlugError::BadChars));
        // No iexact taken-branch in the app serializer: the restricted
        // test is case-sensitive, so "API" passes here.
        assert_eq!(validate_slug("API"), Ok("API"));
        assert_eq!(validate_slug("my_ws-1"), Ok("my_ws-1"));
        assert_eq!(
            SlugError::BadChars.to_string(),
            "Slug can only contain letters, numbers, hyphens (-), and underscores (_)"
        );
    }

    #[test]
    fn contains_url_vectors_match_python() {
        // Every vector below was probed against the live
        // `pi_dash.utils.url.contains_url`.
        let cases: Vec<(String, bool)> = vec![
            ("see https://x.io".to_string(), true),
            ("plain name".to_string(), false),
            ("visit www.example.com now".to_string(), true),
            ("example.com".to_string(), true),
            ("nodots here".to_string(), false),
            ("1.2.3.4".to_string(), true),
            ("Acme Works".to_string(), false),
            ("my_ws-1".to_string(), false),
            ("a.b".to_string(), false),
            ("x.io".to_string(), true),
            ("256.1.1.1".to_string(), true),
            ("foo,bar.baz qux".to_string(), true),
            // Length and truncation guards count code points, not bytes:
            // 412 chars (1213 bytes) with a URL still matches.
            ("é".repeat(400) + " example.com", true),
            // 561-char line: truncated to 500 chars, the URL is cut.
            ("x".repeat(550) + "example.com", false),
            // Over 1000 code points: never matches, even with a URL.
            ("é".repeat(1001) + "https://x.io", false),
            ("x".repeat(990) + " example.com", false),
            ("line1 example.com\nline2 plain".to_string(), true),
            // 513-char single line: truncated, the URL is cut.
            ("a".repeat(500) + " https://x.io", false),
        ];
        for (input, expected) in &cases {
            assert_eq!(contains_url(input), *expected, "{input:?}");
        }
        assert_eq!(validate_name("Acme Wörks"), Ok("Acme Wörks"));
        assert_eq!(
            validate_name("see https://x.io"),
            Err(NameError::ContainsUrl)
        );
    }

    #[test]
    fn wire_fields_match_live_serializer_order() {
        // Pinned against `list(WorkSpaceSerializer().fields)` from the
        // live Django serializers (repo venv probe).
        assert_eq!(
            WORKSPACE_WIRE_FIELDS.to_vec(),
            [
                "id",
                "total_members",
                "logo_url",
                "role",
                "created_at",
                "updated_at",
                "deleted_at",
                "name",
                "logo",
                "slug",
                "organization_size",
                "timezone",
                "background_color",
                "created_by",
                "updated_by",
                "logo_asset",
                "owner",
            ]
        );
        assert_eq!(
            WORKSPACE_MEMBER_WIRE_FIELDS.to_vec(),
            [
                "id",
                "member",
                "created_at",
                "updated_at",
                "deleted_at",
                "role",
                "company_role",
                "view_props",
                "default_props",
                "issue_props",
                "is_active",
                "getting_started_checklist",
                "tips",
                "explored_features",
                "created_by",
                "updated_by",
                "workspace",
            ]
        );
        assert_eq!(
            WORKSPACE_MEMBER_ADMIN_WIRE_FIELDS, WORKSPACE_MEMBER_WIRE_FIELDS,
            "admin shape differs only in the nested member value"
        );
        assert_eq!(
            WORKSPACE_MEMBER_ME_WIRE_FIELDS.to_vec(),
            [
                "id",
                "draft_issue_count",
                "created_at",
                "updated_at",
                "deleted_at",
                "role",
                "company_role",
                "view_props",
                "default_props",
                "issue_props",
                "is_active",
                "getting_started_checklist",
                "tips",
                "explored_features",
                "created_by",
                "updated_by",
                "workspace",
                "member",
            ]
        );
    }

    /// Live DRF bytes below: captured from the real serializers via
    /// DRF's `JSONRenderer` (repo venv, unsaved model instances, no DB —
    /// `{"a": [1, "x\"y"]}` and `väl`/`Wörks` stress escaping/unicode).
    const WS_FULL: &str = r##"{"id":"55555555-5555-5555-5555-555555555555","total_members":7,"logo_url":null,"role":20,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"name":"Acme Wörks","logo":null,"slug":"acme-works","organization_size":null,"timezone":"UTC","background_color":"#FF0000","created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","logo_asset":null,"owner":"11111111-1111-1111-1111-111111111111"}"##;
    const WS_NONE: &str = r##"{"id":"55555555-5555-5555-5555-555555555555","total_members":null,"logo_url":null,"role":null,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"name":"Acme Wörks","logo":null,"slug":"acme-works","organization_size":null,"timezone":"UTC","background_color":"#FF0000","created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","logo_asset":null,"owner":"11111111-1111-1111-1111-111111111111"}"##;
    const WS_ABSENT: &str = r##"{"id":"55555555-5555-5555-5555-555555555555","logo_url":null,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"name":"Acme Wörks","logo":null,"slug":"acme-works","organization_size":null,"timezone":"UTC","background_color":"#FF0000","created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","logo_asset":null,"owner":"11111111-1111-1111-1111-111111111111"}"##;
    const WS_LITE: &str = r##"{"name":"Acme Wörks","slug":"acme-works","id":"55555555-5555-5555-5555-555555555555","logo_url":null}"##;
    const MEMBER: &str = r##"{"id":"77777777-7777-7777-7777-777777777777","member":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"avatars/a.png","avatar_url":"avatars/a.png","is_bot":false,"display_name":""},"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"role":15,"company_role":null,"view_props":{"labels":true},"default_props":{"a":[1,"x\"y"]},"issue_props":{"subscribed":true},"is_active":true,"getting_started_checklist":{},"tips":{"k":"väl"},"explored_features":{},"created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","workspace":"55555555-5555-5555-5555-555555555555"}"##;
    const MEMBER_ADMIN: &str = r##"{"id":"77777777-7777-7777-7777-777777777777","member":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"avatars/a.png","avatar_url":"avatars/a.png","is_bot":false,"display_name":"","email":"ada@acme.test","last_login_medium":"password"},"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"role":15,"company_role":null,"view_props":{"labels":true},"default_props":{"a":[1,"x\"y"]},"issue_props":{"subscribed":true},"is_active":true,"getting_started_checklist":{},"tips":{"k":"väl"},"explored_features":{},"created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","workspace":"55555555-5555-5555-5555-555555555555"}"##;
    const ME_FULL: &str = r##"{"id":"77777777-7777-7777-7777-777777777777","draft_issue_count":3,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"role":15,"company_role":null,"view_props":{"labels":true},"default_props":{"a":[1,"x\"y"]},"issue_props":{"subscribed":true},"is_active":true,"getting_started_checklist":{},"tips":{"k":"väl"},"explored_features":{},"created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","workspace":"55555555-5555-5555-5555-555555555555","member":"11111111-1111-1111-1111-111111111111"}"##;
    const ME_NONE: &str = r##"{"id":"77777777-7777-7777-7777-777777777777","draft_issue_count":null,"created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"role":15,"company_role":null,"view_props":{"labels":true},"default_props":{"a":[1,"x\"y"]},"issue_props":{"subscribed":true},"is_active":true,"getting_started_checklist":{},"tips":{"k":"väl"},"explored_features":{},"created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","workspace":"55555555-5555-5555-5555-555555555555","member":"11111111-1111-1111-1111-111111111111"}"##;
    const ME_ABSENT: &str = r##"{"id":"77777777-7777-7777-7777-777777777777","created_at":"2026-01-15T12:30:45.123456Z","updated_at":"2026-01-16T08:05:04Z","deleted_at":null,"role":15,"company_role":null,"view_props":{"labels":true},"default_props":{"a":[1,"x\"y"]},"issue_props":{"subscribed":true},"is_active":true,"getting_started_checklist":{},"tips":{"k":"väl"},"explored_features":{},"created_by":null,"updated_by":"22222222-2222-2222-2222-222222222222","workspace":"55555555-5555-5555-5555-555555555555","member":"11111111-1111-1111-1111-111111111111"}"##;

    fn workspace_row() -> WorkspaceRow<'static> {
        WorkspaceRow {
            id: "55555555-5555-5555-5555-555555555555",
            total_members: Some(Some(7)),
            logo_url: None,
            role: Some(Some(20)),
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            deleted_at: None,
            name: "Acme Wörks",
            logo: None,
            slug: "acme-works",
            organization_size: None,
            timezone: "UTC",
            background_color: "#FF0000",
            created_by: None,
            updated_by: Some("22222222-2222-2222-2222-222222222222"),
            logo_asset: None,
            owner: "11111111-1111-1111-1111-111111111111",
        }
    }

    struct Blobs {
        view_props: Value,
        default_props: Value,
        issue_props: Value,
        getting_started_checklist: Value,
        tips: Value,
        explored_features: Value,
    }

    fn blobs() -> Blobs {
        Blobs {
            view_props: serde_json::json!({"labels": true}),
            default_props: serde_json::json!({"a": [1, "x\"y"]}),
            issue_props: serde_json::json!({"subscribed": true}),
            getting_started_checklist: serde_json::json!({}),
            tips: serde_json::json!({"k": "väl"}),
            explored_features: serde_json::json!({}),
        }
    }

    fn member_core(blobs: &Blobs) -> WorkspaceMemberCore<'_> {
        WorkspaceMemberCore {
            id: "77777777-7777-7777-7777-777777777777",
            created_at: "2026-01-15T12:30:45.123456Z",
            updated_at: "2026-01-16T08:05:04Z",
            deleted_at: None,
            role: 15,
            company_role: None,
            view_props: &blobs.view_props,
            default_props: &blobs.default_props,
            issue_props: &blobs.issue_props,
            is_active: true,
            getting_started_checklist: &blobs.getting_started_checklist,
            tips: &blobs.tips,
            explored_features: &blobs.explored_features,
            created_by: None,
            updated_by: Some("22222222-2222-2222-2222-222222222222"),
        }
    }

    /// Stand-in for SER-D's `UserLiteView` (PIDASHCONV-603): same 7 keys
    /// in the same order (`user.py:144-152`), proving the
    /// handler-composed member slot replays byte-identical output.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    struct StubUserLite<'a> {
        id: &'a str,
        first_name: &'a str,
        last_name: &'a str,
        avatar: &'a str,
        avatar_url: Option<&'a str>,
        is_bot: bool,
        display_name: &'a str,
    }

    fn stub_user_lite() -> StubUserLite<'static> {
        StubUserLite {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "avatars/a.png",
            avatar_url: Some("avatars/a.png"),
            is_bot: false,
            display_name: "",
        }
    }

    /// Stand-in for SER-D's `UserAdminLiteView` (PIDASHCONV-603): same 9
    /// keys in the same order (`user.py:159-168`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    struct StubUserAdminLite<'a> {
        id: &'a str,
        first_name: &'a str,
        last_name: &'a str,
        avatar: &'a str,
        avatar_url: Option<&'a str>,
        is_bot: bool,
        display_name: &'a str,
        email: Option<&'a str>,
        last_login_medium: &'a str,
    }

    fn stub_user_admin_lite() -> StubUserAdminLite<'static> {
        StubUserAdminLite {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "avatars/a.png",
            avatar_url: Some("avatars/a.png"),
            is_bot: false,
            display_name: "",
            email: Some("ada@acme.test"),
            last_login_medium: "password",
        }
    }

    #[test]
    fn workspace_replays_live_bytes() {
        let row = workspace_row();
        let rendered =
            serde_json::to_string(&workspace_to_representation(&row)).expect("serializes");
        assert_eq!(rendered, WS_FULL);

        let mut none = workspace_row();
        none.total_members = Some(None);
        none.role = Some(None);
        let rendered =
            serde_json::to_string(&workspace_to_representation(&none)).expect("serializes");
        assert_eq!(rendered, WS_NONE);

        let mut absent = workspace_row();
        absent.total_members = None;
        absent.role = None;
        let rendered =
            serde_json::to_string(&workspace_to_representation(&absent)).expect("serializes");
        assert_eq!(rendered, WS_ABSENT);
    }

    #[test]
    fn workspace_lite_replays_live_bytes() {
        let row = WorkspaceLiteRow {
            name: "Acme Wörks",
            slug: "acme-works",
            id: "55555555-5555-5555-5555-555555555555",
            logo_url: None,
        };
        let rendered =
            serde_json::to_string(&workspace_lite_to_representation(&row)).expect("serializes");
        assert_eq!(rendered, WS_LITE);
    }

    #[test]
    fn member_replays_live_bytes() {
        let blobs = blobs();
        let row = WorkSpaceMemberRow {
            core: member_core(&blobs),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: stub_user_lite(),
        };
        let rendered = serde_json::to_string(&member_to_representation(&row)).expect("serializes");
        assert_eq!(rendered, MEMBER);
    }

    #[test]
    fn member_fields_kwarg_ignored_replays_live_bytes() {
        // BUG-fields (member.py:54,73): fields=("id", "member", "role") is
        // dead — the full 17-key shape renders.
        let blobs = blobs();
        let row = WorkSpaceMemberRow {
            core: member_core(&blobs),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: stub_user_lite(),
        };
        let view = member_fields_to_representation(&row, &["id", "member", "role"]);
        assert_eq!(serde_json::to_string(&view).expect("serializes"), MEMBER);
    }

    #[test]
    fn member_admin_replays_live_bytes() {
        let blobs = blobs();
        let row = WorkSpaceMemberAdminRow {
            core: member_core(&blobs),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: stub_user_admin_lite(),
        };
        let rendered =
            serde_json::to_string(&member_admin_to_representation(&row)).expect("serializes");
        assert_eq!(rendered, MEMBER_ADMIN);
    }

    #[test]
    fn member_admin_fields_kwarg_ignored_replays_live_bytes() {
        // BUG-fields (member.py:52,71): same dead kwarg, admin shape.
        let blobs = blobs();
        let row = WorkSpaceMemberAdminRow {
            core: member_core(&blobs),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: stub_user_admin_lite(),
        };
        let view = member_admin_fields_to_representation(&row, &["id", "member", "role"]);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            MEMBER_ADMIN
        );
    }

    #[test]
    fn member_me_replays_live_bytes() {
        let blobs = blobs();
        let full = WorkspaceMemberMeRow {
            core: member_core(&blobs),
            draft_issue_count: Some(Some(3)),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: "11111111-1111-1111-1111-111111111111",
        };
        let rendered =
            serde_json::to_string(&member_me_to_representation(&full)).expect("serializes");
        assert_eq!(rendered, ME_FULL);

        let none = WorkspaceMemberMeRow {
            core: member_core(&blobs),
            draft_issue_count: Some(None),
            workspace: "55555555-5555-5555-5555-555555555555",
            member: "11111111-1111-1111-1111-111111111111",
        };
        let rendered =
            serde_json::to_string(&member_me_to_representation(&none)).expect("serializes");
        assert_eq!(rendered, ME_NONE);

        let absent = WorkspaceMemberMeRow {
            core: member_core(&blobs),
            draft_issue_count: None,
            workspace: "55555555-5555-5555-5555-555555555555",
            member: "11111111-1111-1111-1111-111111111111",
        };
        let rendered =
            serde_json::to_string(&member_me_to_representation(&absent)).expect("serializes");
        assert_eq!(rendered, ME_ABSENT);
    }
}
