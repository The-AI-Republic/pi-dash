//! Collab serializers: project members, workspace invites, user lite (D-19).
//!
//! Port of `apps/api/pi_dash/api/serializers/`:
//!
//! * `member.py:15-43` (`ProjectMemberSerializer`, `validate_member`
//!   `:25-34`, `validate_role` `:35-43`)
//! * `invite.py:16-60` (`WorkspaceInviteSerializer`, `validate_email`
//!   `:41-47`, `validate_role` `:48-52`, `validate` `:53-60`)
//! * `user.py:13-38` (`UserLiteSerializer`, `avatar_url` `:21-25`)
//!
//! These are pure kernels: each `validate_*` takes the already-looked-up
//! facts it needs (workspace membership, existing invite) as plain arguments
//! and returns the exact DRF field error; each `to_representation` takes a
//! row borrowed from the caller and returns a `serde::Serialize` view whose
//! fields are the live DRF wire fields in `Meta.fields` order. UUID and FK
//! primary keys render as strings (`PrimaryKeyRelatedField`, read-only); a
//! null FK renders `null`. Datetimes cross this boundary already rendered as
//! DRF iso-8601 strings — formatting owns to the DB edge, so rendering here
//! is a byte-exact passthrough. `avatar_url` is a model `@property`
//! (`db/models/user.py:142-151`: avatar-asset URL, else `avatar`, else
//! `None`) resolved by the caller via [`resolve_avatar_url`]; the view
//! passes it through verbatim (`None` renders `null`: DRF writes `None`
//! without calling the field's `to_representation`).
//!
//! DRF error bodies carry messages only (`{"member": ["..."]}`); the codes
//! (`INVALID_*`) are internal. [`FieldError`] carries both so handlers can
//! render the exact 400 body and still branch on the code.
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields` (`member.py:43`, `invite.py:32-39`, `user.py:38`)
//! constrain writes, of which this port has none. `WorkspaceInviteSerializer`
//! lists `workspace` in `read_only_fields` (`invite.py:34`) although it is
//! not in `fields` — DRF ignores unknown read-only entries; there is no
//! `workspace` key on the wire.
//!
//! Ported bugs (translate, don't redesign):
//!
//! * BUG-3 (`user.py:28-37`, fixture `fields_note`): `Meta.fields` lists
//!   `email` twice. The wire carries it once. [`USER_LITE_DECLARED_FIELDS`]
//!   keeps the duplicate as-is; [`USER_LITE_WIRE_FIELDS`] is the deduped
//!   wire order.
//! * BUG-slug (`invite.py:54`): `validate` reads `self.context["slug"]`,
//!   so a missing slug raises `KeyError` (a 500 in Django), while
//!   `validate_member` (`member.py:26-28`) raises an explicit 400
//!   (`INVALID_SLUG`). [`validate_invite_unique`] returns
//!   [`InviteUniqueError::MissingSlug`] for the `KeyError` arm — the handler
//!   maps it to a 500. Both arms are unreachable from the routes: every
//!   view passes `context={"slug": slug}` (`views/member.py:152`,
//!   `views/invite.py:91,121`).
//! * `validate_member`'s `if not value` arm (`member.py:29-30`) is dead for
//!   real instances (a resolved `User` is always truthy; `None` is rejected
//!   earlier by `required=True`); it is kept as the empty-string arm.
//!
//! Validation pipeline order (DRF field stage before `validate_*`):
//! required (`member.py:20-23`) → pk lookup (`does_not_exist`) →
//! [`validate_member`] (slug → member → workspace) →
//! [`validate_project_member_role`]. [`validate_project_member_create`]
//! runs the full create pipeline and collects every field error, as
//! `is_valid` does. Unknown member UUIDs fail at the pk stage with
//! `Invalid pk "<uuid>" - object does not exist.` (`does_not_exist`); the
//! project id written by `save(project_id=...)` (`views/member.py:152-156`)
//! and the invite `workspace`/`created_by` written by
//! `save(workspace=..., created_by=...)` (`views/invite.py:89-94`) belong to
//! the handler layer, not here.

use serde::Serialize;
use std::sync::LazyLock;

/// Workspace roles, `app/permissions/base.py:13-16` (`member.py` reaches the
/// same values through the `pi_dash.utils.permissions` re-export).
pub const ROLE_ADMIN: i64 = 20;
/// Workspace `MEMBER` role value (see [`ROLE_ADMIN`]).
pub const ROLE_MEMBER: i64 = 15;
/// Workspace `GUEST` role value (see [`ROLE_ADMIN`]).
pub const ROLE_GUEST: i64 = 5;

/// Roles accepted by both `validate_role` kernels, in Python list order
/// (`member.py:36`, `invite.py:49`).
pub const WORKSPACE_ROLES: [i64; 3] = [ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST];

/// `ProjectMember.role` model default (`db/models/project.py:341`); DRF
/// applies it when the key is absent, so a missing role is not an error.
pub const PROJECT_MEMBER_DEFAULT_ROLE: i64 = ROLE_GUEST;

/// One DRF field error: the wire `message` plus the internal `code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Field key the message renders under (`non_field_errors` for
    /// object-level errors).
    pub field: &'static str,
    /// Exact wire message, e.g. `"Member not found in workspace"`.
    pub message: String,
    /// Internal DRF code, e.g. `"INVALID_MEMBER"` (never on the wire).
    pub code: &'static str,
}

impl FieldError {
    fn new(field: &'static str, message: impl Into<String>, code: &'static str) -> Self {
        Self {
            field,
            message: message.into(),
            code,
        }
    }

    /// The exact JSON body Django renders for this single error,
    /// `{"<field>": ["<message>"]}`.
    pub fn body(&self) -> serde_json::Value {
        let mut map = serde_json::Map::with_capacity(1);
        map.insert(
            self.field.to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(self.message.clone())]),
        );
        serde_json::Value::Object(map)
    }
}

/// `ProjectMemberSerializer.Meta.fields` (`member.py:42`), wire order.
pub const PROJECT_MEMBER_FIELDS: [&str; 3] = ["id", "member", "role"];
/// `ProjectMemberSerializer.Meta.read_only_fields` (`member.py:43`).
pub const PROJECT_MEMBER_READ_ONLY: [&str; 1] = ["id"];

/// `PrimaryKeyRelatedField` stage for `member` (`member.py:20-23`,
/// `queryset=User.objects.all()`, `required=True`): absent input fails
/// `required`; an unknown UUID fails `does_not_exist` with the pk
/// interpolated, exactly as DRF renders it.
pub fn resolve_member(member: Option<&str>, user_exists: bool) -> Result<String, FieldError> {
    let raw =
        member.ok_or_else(|| FieldError::new("member", "This field is required.", "required"))?;
    if !user_exists {
        return Err(FieldError::new(
            "member",
            format!(r#"Invalid pk "{raw}" - object does not exist."#),
            "does_not_exist",
        ));
    }
    Ok(raw.to_string())
}

/// Port of `validate_member` (`member.py:25-34`).
///
/// `slug` is the serializer context entry; `in_workspace` is
/// `WorkspaceMember.objects.filter(workspace__slug=slug,
/// member=value).exists()`. Arms in source order: missing/empty slug →
/// `INVALID_SLUG`; empty member (dead arm, see module docs) →
/// `INVALID_MEMBER`; not in workspace → `INVALID_MEMBER`.
pub fn validate_member(
    member: &str,
    slug: Option<&str>,
    in_workspace: bool,
) -> Result<(), FieldError> {
    match slug {
        Some(slug) if !slug.is_empty() => {}
        _ => {
            return Err(FieldError::new(
                "member",
                "Slug is required",
                "INVALID_SLUG",
            ));
        }
    }
    if member.is_empty() {
        return Err(FieldError::new(
            "member",
            "Member is required",
            "INVALID_MEMBER",
        ));
    }
    if !in_workspace {
        return Err(FieldError::new(
            "member",
            "Member not found in workspace",
            "INVALID_MEMBER",
        ));
    }
    Ok(())
}

/// Port of `ProjectMemberSerializer.validate_role` (`member.py:35-43`).
pub fn validate_project_member_role(role: i64) -> Result<(), FieldError> {
    if WORKSPACE_ROLES.contains(&role) {
        Ok(())
    } else {
        Err(FieldError::new("role", "Invalid role", "INVALID_ROLE"))
    }
}

/// A `ProjectMember` row for output rendering: membership `id` UUID string,
/// member user `id` UUID string, integer `role`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMemberRow<'a> {
    pub id: &'a str,
    pub member: &'a str,
    pub role: i64,
}

/// `ProjectMemberSerializer.to_representation` output (`member.py:40-43`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectMemberView<'a> {
    pub id: &'a str,
    pub member: &'a str,
    pub role: i64,
}

/// Port of the `ProjectMemberSerializer` output shape (`member.py:40-43`).
pub fn project_member_to_representation<'a>(
    row: &'a ProjectMemberRow<'a>,
) -> ProjectMemberView<'a> {
    ProjectMemberView {
        id: row.id,
        member: row.member,
        role: row.role,
    }
}

/// Create-pipeline input for [`validate_project_member_create`].
pub struct ProjectMemberCreateInput<'a> {
    /// Raw `member` value (`None` when the key is absent).
    pub member: Option<&'a str>,
    /// `User.objects.filter(pk=member).exists()` (pk stage).
    pub user_exists: bool,
    /// Raw `role` value (`None` when absent → model default applies).
    pub role: Option<i64>,
    /// Serializer context `slug` (`None` when absent).
    pub slug: Option<&'a str>,
    /// `WorkspaceMember` existence for the resolved member.
    pub in_workspace: bool,
}

/// Full create pipeline (`member.py:15-43`): field stage then
/// `validate_member`/`validate_role`, collecting every field error in DRF
/// field order, as `is_valid` does. A missing `role` applies
/// [`PROJECT_MEMBER_DEFAULT_ROLE`] with no error (model default,
/// `db/models/project.py:341`).
pub fn validate_project_member_create(input: &ProjectMemberCreateInput) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let member = match resolve_member(input.member, input.user_exists) {
        Ok(member) => Some(member),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let role = input.role.unwrap_or(PROJECT_MEMBER_DEFAULT_ROLE);
    if let Some(member) = member.as_deref() {
        if let Err(error) = validate_member(member, input.slug, input.in_workspace) {
            errors.push(error);
        }
    }
    if input
        .role
        .is_some_and(|role| validate_project_member_role(role).is_err())
    {
        errors.push(validate_project_member_role(role).expect_err("role just failed validation"));
    }
    errors
}

/// `WorkspaceInviteSerializer.Meta.fields` (`invite.py:22-30`), wire order.
pub const WORKSPACE_INVITE_FIELDS: [&str; 7] = [
    "id",
    "email",
    "role",
    "created_at",
    "updated_at",
    "responded_at",
    "accepted",
];

/// `WorkspaceInviteSerializer.Meta.read_only_fields` (`invite.py:32-39`)
/// verbatim, including `workspace`, which is not in `fields` and therefore
/// never appears on the wire (DRF ignores unknown read-only entries).
pub const WORKSPACE_INVITE_READ_ONLY: [&str; 6] = [
    "id",
    "workspace",
    "created_at",
    "updated_at",
    "responded_at",
    "accepted",
];

/// Email presence stage for `email` (auto `EmailField`: `required`,
/// `allow_blank=False`): absent → `required`; empty → `blank`.
pub fn resolve_invite_email(email: Option<&str>) -> Result<&str, FieldError> {
    match email {
        None => Err(FieldError::new(
            "email",
            "This field is required.",
            "required",
        )),
        Some("") => Err(FieldError::new(
            "email",
            "This field may not be blank.",
            "blank",
        )),
        Some(email) => Ok(email),
    }
}

/// Port of `validate_email` (`invite.py:41-47`): Django's `validate_email`,
/// else `INVALID_EMAIL_ADDRESS`. See [`is_valid_email`] for the ported
/// validator.
pub fn validate_invite_email(email: &str) -> Result<(), FieldError> {
    if is_valid_email(email) {
        Ok(())
    } else {
        Err(FieldError::new(
            "email",
            "Invalid email address",
            "INVALID_EMAIL_ADDRESS",
        ))
    }
}

/// Port of `WorkspaceInviteSerializer.validate_role` (`invite.py:48-52`).
pub fn validate_invite_role(role: i64) -> Result<(), FieldError> {
    if WORKSPACE_ROLES.contains(&role) {
        Ok(())
    } else {
        Err(FieldError::new(
            "role",
            "Invalid role",
            "INVALID_WORKSPACE_MEMBER_ROLE",
        ))
    }
}

/// The `KeyError` arm of `validate` (`invite.py:54`): Python reads
/// `self.context["slug"]`, so a missing slug raises `KeyError` (a 500),
/// unlike `validate_member`'s explicit 400. Unreachable from the routes;
/// the handler maps this to a 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingSlug;

/// Outcome of the object-level `validate` beyond the per-field kernels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InviteUniqueError {
    /// `self.context["slug"]` absent (`invite.py:54` `KeyError` parity).
    MissingSlug(MissingSlug),
    /// Duplicate invite scoped by `workspace__slug` (`invite.py:55-59`).
    Duplicate(FieldError),
}

/// Port of `validate` (`invite.py:53-60`): when `email` is present and a
/// `WorkspaceMemberInvite` with that email already exists in the slug's
/// workspace (`already_invited`), raise under `non_field_errors` with
/// `EMAIL_ALREADY_INVITED`.
pub fn validate_invite_unique(
    email: Option<&str>,
    slug: Option<&str>,
    already_invited: bool,
) -> Result<(), InviteUniqueError> {
    // `invite.py:54` reads `self.context["slug"]`: only an absent key raises
    // `KeyError`. An empty-string slug is a dict hit, so validation proceeds
    // (the workspace filter then matches nothing).
    if slug.is_none() {
        return Err(InviteUniqueError::MissingSlug(MissingSlug));
    }
    match email {
        Some(email) if !email.is_empty() && already_invited => {
            Err(InviteUniqueError::Duplicate(FieldError::new(
                "non_field_errors",
                "Email already invited",
                "EMAIL_ALREADY_INVITED",
            )))
        }
        _ => Ok(()),
    }
}

/// A `WorkspaceMemberInvite` row for output rendering. Datetimes are
/// pre-rendered DRF iso-8601 strings; `responded_at` is `None` until the
/// invite is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInviteRow<'a> {
    pub id: &'a str,
    pub email: &'a str,
    pub role: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub responded_at: Option<&'a str>,
    pub accepted: bool,
}

/// `WorkspaceInviteSerializer.to_representation` output (`invite.py:21-39`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceInviteView<'a> {
    pub id: &'a str,
    pub email: &'a str,
    pub role: i64,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub responded_at: Option<&'a str>,
    pub accepted: bool,
}

/// Port of the `WorkspaceInviteSerializer` output shape (`invite.py:21-39`).
pub fn workspace_invite_to_representation<'a>(
    row: &'a WorkspaceInviteRow<'a>,
) -> WorkspaceInviteView<'a> {
    WorkspaceInviteView {
        id: row.id,
        email: row.email,
        role: row.role,
        created_at: row.created_at,
        updated_at: row.updated_at,
        responded_at: row.responded_at,
        accepted: row.accepted,
    }
}

static EMAIL_USER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?i)\A(?:[-!#$%&'*+/=?^_`{}|~0-9A-Z]+(?:\.[-!#$%&'*+/=?^_`{}|~0-9A-Z]+)*|"(?:[\x01-\x08\x0B\x0C\x0E-\x1F!#-\x5B\x5D-\x7F]|\\[\x01-\x09\x0B\x0C\x0E-\x7F])*")\z"#,
    )
    .expect("email user regex compiles")
});

static EMAIL_DOMAIN_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)\A(?:[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?\.)+(?:[A-Z0-9-]{2,63})\z")
        .expect("email domain regex compiles")
});

static EMAIL_LITERAL_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)\A\[([A-F0-9:.]+)\]\z").expect("email literal regex compiles")
});

/// Django's `domain_allowlist` (`django/core/validators.py:196`), matched
/// exactly (case-sensitive, like Python's `in` on the list).
const EMAIL_DOMAIN_ALLOWLIST: [&str; 1] = ["localhost"];

/// Port of `EmailValidator.validate_domain_part`
/// (`django/core/validators.py:230-241`): the domain regex (the `(?<!-)`
/// look-behind the `regex` crate cannot express is enforced by the
/// `ends_with('-')` check — identical accept set), else a bracketed
/// IPv4/IPv6 literal validated as an IP address.
fn email_domain_part_valid(domain: &str) -> bool {
    if EMAIL_DOMAIN_RE.is_match(domain) && !domain.ends_with('-') {
        return true;
    }
    if let Some(literal) = EMAIL_LITERAL_RE
        .captures(domain)
        .and_then(|captures| captures.get(1))
    {
        if literal.as_str().parse::<std::net::IpAddr>().is_ok() {
            return true;
        }
    }
    false
}

/// Port of Django's `validate_email`
/// (`django/core/validators.py:206-228`, ground truth: Django 4.2.30 as
/// vendored for this repo): at most 320 characters, an `@` split, the
/// dot-atom/quoted-string user part, then the domain part — allowlist,
/// domain regex/literal, else one IDN round-trip (`punycode`, i.e.
/// `domain.encode("idna")`) re-validated. The `regex` crate has no
/// look-behind, so the trailing-hyphen rule is a manual check (same accept
/// set); the IDN encoding itself comes from the `idna` crate's UTS-46
/// mapping where Python uses IDNA 2003 — identical on ordinary registered
/// names, divergence only on unassigned or transitional code points, which
/// no contract case exercises.
pub fn is_valid_email(value: &str) -> bool {
    if value.is_empty() || value.chars().count() > 320 || !value.contains('@') {
        return false;
    }
    let Some((user, domain)) = value.rsplit_once('@') else {
        return false;
    };
    if !EMAIL_USER_RE.is_match(user) {
        return false;
    }
    if EMAIL_DOMAIN_ALLOWLIST.contains(&domain) {
        return true;
    }
    if email_domain_part_valid(domain) {
        return true;
    }
    if !domain.is_ascii() {
        if let Ok(ascii) = idna::domain_to_ascii(domain) {
            if email_domain_part_valid(&ascii) {
                return true;
            }
        }
    }
    false
}

/// `UserLiteSerializer.Meta.fields` (`user.py:28-37`) verbatim: `email`
/// appears twice (`:33` and `:36`) — BUG-3, ported as-is; the wire carries
/// it once (see [`USER_LITE_WIRE_FIELDS`]).
pub const USER_LITE_DECLARED_FIELDS: [&str; 8] = [
    "id",
    "first_name",
    "last_name",
    "email",
    "avatar",
    "avatar_url",
    "display_name",
    "email",
];

/// `UserLiteSerializer` wire keys in output order: the declared list with
/// the second `email` dropped (DRF builds one field per name).
pub const USER_LITE_WIRE_FIELDS: [&str; 7] = [
    "id",
    "first_name",
    "last_name",
    "email",
    "avatar",
    "avatar_url",
    "display_name",
];

/// Port of `User.avatar_url` (`db/models/user.py:142-151`): the avatar
/// asset URL when an asset is attached, else the `avatar` text, else `None`.
/// The caller resolves the asset URL; this kernel only encodes the
/// precedence.
pub fn resolve_avatar_url<'a>(
    avatar_asset_url: Option<&'a str>,
    avatar: &'a str,
) -> Option<&'a str> {
    if let Some(url) = avatar_asset_url {
        return Some(url);
    }
    if !avatar.is_empty() {
        return Some(avatar);
    }
    None
}

/// A `User` row for lite rendering: `id` UUID string, names, `email`,
/// `avatar` text, and the resolved [`resolve_avatar_url`] value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserLiteRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub email: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub display_name: &'a str,
}

/// `UserLiteSerializer.to_representation` output (`user.py:13-38`), in wire
/// order. `avatar_url` is read-only (`user.py:21-25`); `None` renders
/// `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UserLiteView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub email: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub display_name: &'a str,
}

/// Port of `UserLiteSerializer` (`user.py:13-38`).
pub fn user_lite_to_representation<'a>(row: &'a UserLiteRow<'a>) -> UserLiteView<'a> {
    UserLiteView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        email: row.email,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        display_name: row.display_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/v1_projects/serializers/collab.golden.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn serializer<'a>(golden: &'a Value, name: &str) -> &'a Value {
        golden
            .get("serializers")
            .and_then(|serializers| serializers.get(name))
            .unwrap_or_else(|| panic!("golden lacks {name}"))
    }

    fn golden_case<'a>(golden: &'a Value, name: &str, case: &str) -> &'a Value {
        serializer(golden, name)
            .get("goldens")
            .and_then(|goldens| goldens.get(case))
            .unwrap_or_else(|| panic!("golden lacks {name}.{case}"))
    }

    /// Canonical form: objects with recursively sorted keys, so the replay
    /// comparison is byte-identical regardless of serializer field order.
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for key in keys {
                    sorted.insert(key.clone(), canonical(&map[key]));
                }
                Value::Object(sorted)
            }
            Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }

    /// Field-for-field equality with the golden plus byte-identical replay
    /// of the canonical form the goldens are stored in.
    fn assert_replay(produced: &Value, expected: &Value) {
        assert_eq!(
            produced, expected,
            "field-for-field mismatch against golden"
        );
        assert_eq!(
            serde_json::to_string(&canonical(produced)).expect("serializes"),
            serde_json::to_string(&canonical(expected)).expect("serializes"),
            "byte-identical replay mismatch"
        );
    }

    /// Top-level JSON key order of a struct's serialization.
    ///
    /// Read off the serialized string, not a `serde_json::Value`: struct
    /// serialization always emits declaration order, while `Value` objects
    /// iterate alphabetically.
    fn serialized_keys<T: serde::Serialize>(value: &T) -> Vec<String> {
        let rendered = serde_json::to_string(value).expect("serializes");
        let mut keys = Vec::new();
        let mut depth = 0usize;
        let mut chars = rendered.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                }
                '"' if depth == 1 => {
                    let mut key = String::new();
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '"' {
                            break;
                        }
                        key.push(next);
                    }
                    if chars.peek() == Some(&':') {
                        keys.push(key);
                    }
                }
                _ => {}
            }
        }
        keys
    }

    #[test]
    fn roles_match_python() {
        // app/permissions/base.py:13-16; pinned by the golden role_source
        // notes on both serializers.
        assert_eq!((ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST), (20, 15, 5));
        assert_eq!(WORKSPACE_ROLES, [20, 15, 5]);
    }

    #[test]
    fn member_ok_replays_golden() {
        // Fixture ProjectMemberSerializer.ok: input member/role echoes into
        // output id/member/role (member.py:40-43).
        let golden = fixture();
        let case = golden_case(&golden, "ProjectMemberSerializer", "ok");
        let input = &case["input"];
        let member = input["member"].as_str().expect("member uuid");
        let role = input["role"].as_i64().expect("role int");
        assert!(validate_project_member_create(&ProjectMemberCreateInput {
            member: Some(member),
            user_exists: true,
            role: Some(role),
            slug: Some("ws"),
            in_workspace: true,
        })
        .is_empty());
        let row = ProjectMemberRow {
            id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            member,
            role,
        };
        let view = project_member_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            PROJECT_MEMBER_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "output keys follow Meta.fields order"
        );
        // The golden pins placeholders for both input and output member;
        // the serializer echoes the input verbatim (member.py:40-43).
        let produced = serde_json::to_value(&view).expect("serializes");
        assert_eq!(produced["member"], case["output"]["member"]);
        assert_eq!(produced["role"], case["output"]["role"]);
        assert_eq!(produced["id"], Value::String(row.id.to_string()));
        assert_replay(
            &produced,
            &serde_json::json!({
                "id": row.id,
                "member": case["output"]["member"],
                "role": case["output"]["role"],
            }),
        );
    }

    #[test]
    fn member_missing_replays_golden() {
        // Fixture missing_member: absent key → required (member.py:20-23).
        let golden = fixture();
        let case = golden_case(&golden, "ProjectMemberSerializer", "missing_member");
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: None,
            user_exists: false,
            role: Some(15),
            slug: Some("ws"),
            in_workspace: false,
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "required");
        assert_replay(&errors[0].body(), &case["response"]);
    }

    #[test]
    fn member_not_in_workspace_replays_golden() {
        // Fixture member_not_in_workspace: INVALID_MEMBER (member.py:31-32).
        let golden = fixture();
        let case = golden_case(
            &golden,
            "ProjectMemberSerializer",
            "member_not_in_workspace",
        );
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: Some("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
            user_exists: true,
            role: Some(15),
            slug: Some("ws"),
            in_workspace: false,
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "INVALID_MEMBER");
        assert_replay(&errors[0].body(), &case["response"]);
    }

    #[test]
    fn member_missing_slug_replays_golden() {
        // Fixture missing_slug_context: INVALID_SLUG (member.py:26-28).
        let golden = fixture();
        let case = golden_case(&golden, "ProjectMemberSerializer", "missing_slug_context");
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: Some("cccccccc-cccc-cccc-cccc-cccccccccccc"),
            user_exists: true,
            role: Some(15),
            slug: None,
            in_workspace: true,
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "INVALID_SLUG");
        assert_replay(&errors[0].body(), &case["response"]);
    }

    #[test]
    fn member_unknown_role_replays_golden() {
        // Fixture role_unknown: INVALID_ROLE (member.py:35-38).
        let golden = fixture();
        let case = golden_case(&golden, "ProjectMemberSerializer", "role_unknown");
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: Some("dddddddd-dddd-dddd-dddd-dddddddddddd"),
            user_exists: true,
            role: Some(99),
            slug: Some("ws"),
            in_workspace: true,
        });
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "INVALID_ROLE");
        assert_replay(&errors[0].body(), &case["response"]);
    }

    #[test]
    fn member_unknown_pk_matches_drf() {
        // PrimaryKeyRelatedField stage (member.py:20-23): an unknown UUID
        // fails before validate_member runs.
        let error = resolve_member(Some("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee"), false)
            .expect_err("unknown pk fails");
        assert_eq!(error.field, "member");
        assert_eq!(error.code, "does_not_exist");
        assert_eq!(
            error.message,
            r#"Invalid pk "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee" - object does not exist."#
        );
    }

    #[test]
    fn member_create_collects_member_and_role_errors() {
        // is_valid collects every field error: outsider member plus an
        // unknown role yields both, member first (field order).
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: Some("ffffffff-ffff-ffff-ffff-ffffffffffff"),
            user_exists: true,
            role: Some(99),
            slug: Some("ws"),
            in_workspace: false,
        });
        assert_eq!(
            errors.iter().map(|error| error.code).collect::<Vec<_>>(),
            vec!["INVALID_MEMBER", "INVALID_ROLE"]
        );
        assert_eq!(
            errors.iter().map(|error| error.field).collect::<Vec<_>>(),
            vec!["member", "role"]
        );
    }

    #[test]
    fn member_missing_role_applies_model_default() {
        // role has a model default (db/models/project.py:341), so an
        // absent role is not an error.
        let errors = validate_project_member_create(&ProjectMemberCreateInput {
            member: Some("11111111-1111-1111-1111-111111111111"),
            user_exists: true,
            role: None,
            slug: Some("ws"),
            in_workspace: true,
        });
        assert!(errors.is_empty());
    }

    #[test]
    fn invite_ok_replays_golden() {
        // Fixture ok: email/role echo into the 7-key output
        // (invite.py:21-39); datetimes pass through pre-rendered.
        let golden = fixture();
        let case = golden_case(&golden, "WorkspaceInviteSerializer", "ok");
        let email = case["input"]["email"].as_str().expect("email");
        let role = case["input"]["role"].as_i64().expect("role");
        assert!(resolve_invite_email(Some(email)).is_ok());
        assert!(validate_invite_email(email).is_ok());
        assert!(validate_invite_role(role).is_ok());
        assert!(validate_invite_unique(Some(email), Some("ws"), false).is_ok());
        let row = WorkspaceInviteRow {
            id: "22222222-2222-2222-2222-222222222222",
            email,
            role,
            created_at: "2026-09-29T00:00:00.000000+00:00",
            updated_at: "2026-09-29T00:00:00.000000+00:00",
            responded_at: None,
            accepted: false,
        };
        let view = workspace_invite_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            WORKSPACE_INVITE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "output keys follow Meta.fields order"
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        let expected = &case["output"];
        for key in ["email", "role", "accepted"] {
            assert_eq!(&produced[key], &expected[key], "golden key {key}");
        }
        assert_eq!(produced["responded_at"], Value::Null);
        assert!(produced["id"].is_string());
        assert!(produced["created_at"].is_string());
        assert!(produced["updated_at"].is_string());
    }

    #[test]
    fn invite_bad_email_replays_golden() {
        // Fixture bad_email: Django validate_email rejects "not-an-email"
        // (invite.py:41-46, INVALID_EMAIL_ADDRESS).
        let golden = fixture();
        let case = golden_case(&golden, "WorkspaceInviteSerializer", "bad_email");
        let email = case["input"]["email"].as_str().expect("email");
        assert!(!is_valid_email(email));
        let error = validate_invite_email(email).expect_err("bad email fails");
        assert_eq!(error.code, "INVALID_EMAIL_ADDRESS");
        assert_replay(&error.body(), &case["response"]);
    }

    #[test]
    fn invite_bad_role_replays_golden() {
        // Fixture bad_role: INVALID_WORKSPACE_MEMBER_ROLE
        // (invite.py:48-51).
        let golden = fixture();
        let case = golden_case(&golden, "WorkspaceInviteSerializer", "bad_role");
        let role = case["input"]["role"].as_i64().expect("role");
        let error = validate_invite_role(role).expect_err("bad role fails");
        assert_eq!(error.code, "INVALID_WORKSPACE_MEMBER_ROLE");
        assert_replay(&error.body(), &case["response"]);
    }

    #[test]
    fn invite_duplicate_replays_golden() {
        // Fixture duplicate_email: EMAIL_ALREADY_INVITED under
        // non_field_errors (invite.py:53-60).
        let golden = fixture();
        let case = golden_case(&golden, "WorkspaceInviteSerializer", "duplicate_email");
        let error = validate_invite_unique(Some("dup@example.com"), Some("ws"), true)
            .expect_err("duplicate fails");
        let InviteUniqueError::Duplicate(error) = error else {
            panic!("expected the duplicate arm");
        };
        assert_eq!(error.field, "non_field_errors");
        assert_eq!(error.code, "EMAIL_ALREADY_INVITED");
        assert_replay(&error.body(), &case["response"]);
    }

    #[test]
    fn invite_missing_slug_is_key_error_parity() {
        // invite.py:54 reads self.context["slug"]: a missing slug is a
        // KeyError (500), not a 400 — the opposite of validate_member.
        let error =
            validate_invite_unique(Some("a@b.co"), None, false).expect_err("missing slug fails");
        assert_eq!(error, InviteUniqueError::MissingSlug(MissingSlug));
        assert!(validate_invite_unique(None, Some("ws"), true).is_ok());
    }

    #[test]
    fn invite_empty_slug_proceeds_like_dict_hit() {
        // invite.py:54 reads self.context["slug"]: only an absent key raises
        // KeyError. An empty-string slug is a dict hit, so validation
        // proceeds instead of taking the missing-slug arm.
        assert!(validate_invite_unique(Some("a@b.co"), Some(""), false).is_ok());
        assert!(validate_invite_unique(None, Some(""), false).is_ok());
        let error =
            validate_invite_unique(Some("a@b.co"), None, false).expect_err("absent slug fails");
        assert_eq!(error, InviteUniqueError::MissingSlug(MissingSlug));
    }

    #[test]
    fn user_me_shape_replays_golden() {
        // Fixture me_shape: the exact 7-key output (user.py:28-37).
        let golden = fixture();
        let case = golden_case(&golden, "UserLiteSerializer", "me_shape");
        let output = &case["output"];
        let row = UserLiteRow {
            id: "33333333-3333-3333-3333-333333333333",
            first_name: "Ct",
            last_name: "Owner",
            email: "ct-owner@example.com",
            avatar: "",
            avatar_url: None,
            display_name: "ct-owner",
        };
        let view = user_lite_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            USER_LITE_WIRE_FIELDS
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>(),
            "output keys follow the wire order"
        );
        let produced = serde_json::to_value(&view).expect("serializes");
        let mut wire: Vec<String> = USER_LITE_WIRE_FIELDS
            .iter()
            .map(|name| name.to_string())
            .collect();
        wire.sort();
        let mut golden_keys: Vec<String> = output
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
        golden_keys.sort();
        assert_eq!(wire, golden_keys, "wire key set matches the golden");
        // BUG-3: the declared list keeps the duplicated email as-is.
        assert_eq!(USER_LITE_DECLARED_FIELDS.len(), 8);
        assert_eq!(
            USER_LITE_DECLARED_FIELDS
                .iter()
                .filter(|name| **name == "email")
                .count(),
            2
        );
        assert_replay(
            &produced,
            &serde_json::json!({
                "id": row.id,
                "first_name": row.first_name,
                "last_name": row.last_name,
                "email": row.email,
                "avatar": row.avatar,
                "avatar_url": Value::Null,
                "display_name": row.display_name,
            }),
        );
    }

    #[test]
    fn avatar_url_precedence_matches_model_property() {
        // db/models/user.py:142-151: asset URL, else avatar text, else None.
        assert_eq!(
            resolve_avatar_url(Some("https://cdn.example/a.png"), "legacy"),
            Some("https://cdn.example/a.png")
        );
        assert_eq!(
            resolve_avatar_url(None, "https://cdn.example/b.png"),
            Some("https://cdn.example/b.png")
        );
        assert_eq!(resolve_avatar_url(None, ""), None);
    }

    #[test]
    fn email_battery_matches_django() {
        // Golden-pinned arms plus the common shapes the contract tests
        // exercise; the full differential against Django 4.2 lives in the
        // workpad validation notes.
        for valid in [
            "new@example.com",
            "a@b.co",
            "user@localhost",
            "first.last+tag@sub.example.org",
            "user@[127.0.0.1]",
            "user@[::1]",
            "\"quoted!name\"@example.com",
            "test@international.international",
        ] {
            assert!(is_valid_email(valid), "{valid} must validate");
        }
        for invalid in [
            "not-an-email",
            "",
            "a@b",
            "a@b.c",
            "a b@example.com",
            "user@exam ple.com",
            "user@example.com-",
            "user@[999.999.999.999]",
            "\"quoted space\"@example.com",
            "münchen@münchen.de",
        ] {
            assert!(!is_valid_email(invalid), "{invalid} must fail");
        }
    }
}
