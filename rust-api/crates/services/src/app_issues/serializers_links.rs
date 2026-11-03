#![forbid(unsafe_code)]

//! Link + attachment-lite serializers for app:issues (D-26).
//!
//! Port of `apps/api/pi_dash/app/serializers/issue.py`:
//!
//! * `:738-768` (`GithubPullRequestLinkSerializer`)
//! * `:769-800` (`GitCodeReviewLinkSerializer`)
//! * `:852-866` (`IssueLinkLiteSerializer`)
//! * `:884-899` (`IssueAttachmentLiteSerializer`)
//!
//! Pure output kernels: each `to_representation` takes a row borrowed from
//! the caller and returns a `serde::Serialize` view whose fields are the
//! live DRF wire fields in `Meta.fields` order (all four classes use
//! explicit field lists, so the wire order is the listed order — confirmed
//! by a live-DRF probe, Django 4.2.30 / DRF 3.15.2, whose byte vectors the
//! tests below replay). Fixture: `FX-ISS-04.links.json` (`TRACE.md`:
//! serializers/FX-ISS-04).
//!
//! * UUID and FK primary keys render as strings (`PrimaryKeyRelatedField`,
//!   read-only); a null FK renders `null`. The lite link shape renders the
//!   raw attnames (`issue_id`, `created_by_id`): DRF resolves those through
//!   the pk-only optimization (`relations.py:172-189`), so they render
//!   exactly like the FK names — UUID string or `null` (probed).
//! * Datetimes cross this boundary already rendered as DRF `iso-8601`
//!   strings — formatting owns to the DB edge, so rendering here is a
//!   byte-exact passthrough.
//! * `metadata` / `attributes` (`JSONField`, non-null) cross as parsed
//!   `serde_json::Value`, like the space twin (`space::serializers`).
//! * `asset` (`FileField`) renders the stored path verbatim: the project
//!   storage's `url()` returns the name
//!   (`settings/storage.py:22-23`), and no call site passes a request
//!   context, so DRF never absolutizes. An empty value renders `null`
//!   (`fields.py:1539`, `if not value`; probed) — hence `Option`.
//! * `asset_url` is a model `@property` (`db/models/asset.py:79-100`)
//!   resolved by the caller (it needs the workspace slug plus the
//!   project/issue/asset ids); the view passes it through verbatim.
//! * `created_by_detail` is the app `UserLiteSerializer`
//!   (`user.py:141-153`, ported below — field-identical to the space lite,
//!   but each app domain owns its port); `created_by` is nullable
//!   (`db/mixins.py:29-35`), so a null source renders a present `null`
//!   nest (probed).
//!
//! Shape-only no-ops preserved as documentation, not code:
//! `read_only_fields = fields` (`:766,798,864,897`) constrains writes, of
//! which this port has none. The commented-out `# "issue_id"` (`:891`) is
//! an intentional absence — ported as absence.
//!
//! Out of scope here (not in the four units): the full `IssueLinkSerializer`
//! (`:801-850`, URL normalize + `URLValidator` + dup guards) and
//! `IssueAttachmentSerializer` (`:867-882`), which handlers reuse from the
//! merged space `issue_graph` port; and the `DynamicBaseSerializer` expand
//! machinery (`serializers/base.py:12-216`) — no call site passes `expand`
//! to these shapes, so default construction renders the plain `Meta.fields`
//! body (probed).
//!
//! Ported bugs (translate, don't redesign): none in these four units —
//! they are pure reads with no validation, write, or `SkipField` paths.

use serde::Serialize;

/// App `UserLiteSerializer` wire keys (`user.py:141-153`), in
/// `Meta.fields` order: the `created_by_detail` nest on both link shapes.
pub const USER_LITE_FIELDS: [&str; 7] = [
    "id",
    "first_name",
    "last_name",
    "avatar",
    "avatar_url",
    "is_bot",
    "display_name",
];

/// A `User` row for nested lite rendering (`db/models/user.py:56-137`):
/// `id` UUID string, names, `avatar` text, the resolved `avatar_url`
/// (property, `user.py:142-151`), `is_bot` flag, `display_name`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserLiteRow<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Nested app `UserLiteSerializer.to_representation` output
/// (`user.py:141-153`), in wire order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserLiteView<'a> {
    pub id: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub avatar: &'a str,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub display_name: &'a str,
}

/// Port of the nested app `UserLiteSerializer` (`user.py:141-153`).
pub fn user_lite_to_representation<'a>(row: &'a UserLiteRow<'a>) -> UserLiteView<'a> {
    UserLiteView {
        id: row.id,
        first_name: row.first_name,
        last_name: row.last_name,
        avatar: row.avatar,
        avatar_url: row.avatar_url,
        is_bot: row.is_bot,
        display_name: row.display_name,
    }
}

/// `GithubPullRequestLinkSerializer.Meta.fields` (`:749-765`), wire order.
pub const GITHUB_PR_LINK_FIELDS: [&str; 15] = [
    "id",
    "issue",
    "repo_owner",
    "repo_name",
    "pr_number",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "pr_updated_at",
    "created_at",
    "updated_at",
    "created_by",
    "created_by_detail",
];

/// A `GithubPullRequestLink` row for read rendering
/// (`db/models/integration/github.py:217-252`): required `issue` FK, the
/// `owner/repo#number` triple (`pr_number` is a `PositiveIntegerField`),
/// the display snapshot (`title`/`state`/`merged`/`draft`, webhook
/// refreshed), nullable `pr_updated_at`, audit columns, and the nullable
/// `created_by` FK plus its resolved nest.
#[derive(Debug, Clone, PartialEq)]
pub struct GithubPullRequestLinkRow<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub repo_owner: &'a str,
    pub repo_name: &'a str,
    pub pr_number: i32,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub created_by_detail: Option<UserLiteRow<'a>>,
}

/// `GithubPullRequestLinkSerializer.to_representation` output (`:738-768`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GithubPullRequestLinkView<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub repo_owner: &'a str,
    pub repo_name: &'a str,
    pub pr_number: i32,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub pr_updated_at: Option<&'a str>,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub created_by_detail: Option<UserLiteView<'a>>,
}

/// Port of `GithubPullRequestLinkSerializer` (`:738-768`).
/// Field-for-field copy; the client-supplied-`url` create path lives in
/// the view (`attach_pull_request`), not in this all-read-only shape.
pub fn github_pull_request_link_to_representation<'a>(
    row: &'a GithubPullRequestLinkRow<'a>,
) -> GithubPullRequestLinkView<'a> {
    GithubPullRequestLinkView {
        id: row.id,
        issue: row.issue,
        repo_owner: row.repo_owner,
        repo_name: row.repo_name,
        pr_number: row.pr_number,
        url: row.url,
        title: row.title,
        state: row.state,
        merged: row.merged,
        draft: row.draft,
        pr_updated_at: row.pr_updated_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        created_by_detail: row
            .created_by_detail
            .as_ref()
            .map(user_lite_to_representation),
    }
}

/// `GitCodeReviewLinkSerializer.Meta.fields` (`:776-797`), wire order.
pub const GIT_CODE_REVIEW_LINK_FIELDS: [&str; 20] = [
    "id",
    "issue",
    "provider",
    "host_url",
    "namespace",
    "repo_name",
    "repo_external_id",
    "external_id",
    "external_iid",
    "url",
    "title",
    "state",
    "merged",
    "draft",
    "remote_updated_at",
    "metadata",
    "created_at",
    "updated_at",
    "created_by",
    "created_by_detail",
];

/// A `GitCodeReviewLink` row for read rendering
/// (`db/models/integration/git.py:224-265`): required `issue` FK, the
/// provider plus repo coordinates, the display snapshot, nullable
/// `remote_updated_at`, non-null `metadata` JSON, audit columns, and the
/// nullable `created_by` FK plus its resolved nest.
#[derive(Debug, Clone, PartialEq)]
pub struct GitCodeReviewLinkRow<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub provider: &'a str,
    pub host_url: &'a str,
    pub namespace: &'a str,
    pub repo_name: &'a str,
    pub repo_external_id: &'a str,
    pub external_id: &'a str,
    pub external_iid: &'a str,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<&'a str>,
    pub metadata: &'a serde_json::Value,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub created_by_detail: Option<UserLiteRow<'a>>,
}

/// `GitCodeReviewLinkSerializer.to_representation` output (`:769-800`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GitCodeReviewLinkView<'a> {
    pub id: &'a str,
    pub issue: &'a str,
    pub provider: &'a str,
    pub host_url: &'a str,
    pub namespace: &'a str,
    pub repo_name: &'a str,
    pub repo_external_id: &'a str,
    pub external_id: &'a str,
    pub external_iid: &'a str,
    pub url: &'a str,
    pub title: &'a str,
    pub state: &'a str,
    pub merged: bool,
    pub draft: bool,
    pub remote_updated_at: Option<&'a str>,
    pub metadata: &'a serde_json::Value,
    pub created_at: &'a str,
    pub updated_at: &'a str,
    pub created_by: Option<&'a str>,
    pub created_by_detail: Option<UserLiteView<'a>>,
}

/// Port of `GitCodeReviewLinkSerializer` (`:769-800`). Field-for-field copy.
pub fn git_code_review_link_to_representation<'a>(
    row: &'a GitCodeReviewLinkRow<'a>,
) -> GitCodeReviewLinkView<'a> {
    GitCodeReviewLinkView {
        id: row.id,
        issue: row.issue,
        provider: row.provider,
        host_url: row.host_url,
        namespace: row.namespace,
        repo_name: row.repo_name,
        repo_external_id: row.repo_external_id,
        external_id: row.external_id,
        external_iid: row.external_iid,
        url: row.url,
        title: row.title,
        state: row.state,
        merged: row.merged,
        draft: row.draft,
        remote_updated_at: row.remote_updated_at,
        metadata: row.metadata,
        created_at: row.created_at,
        updated_at: row.updated_at,
        created_by: row.created_by,
        created_by_detail: row
            .created_by_detail
            .as_ref()
            .map(user_lite_to_representation),
    }
}

/// `IssueLinkLiteSerializer.Meta.fields` (`:855-863`), wire order: the raw
/// FK attnames (`issue_id`, `created_by_id`), not nested objects.
pub const ISSUE_LINK_LITE_FIELDS: [&str; 7] = [
    "id",
    "issue_id",
    "title",
    "url",
    "metadata",
    "created_by_id",
    "created_at",
];

/// An `IssueLink` row for lite rendering (`db/models/issue.py:471-475`).
/// `title` is nullable (`:472`); `url` is a required `TextField` (`:473`);
/// `issue` is a required FK rendered under its attname; `created_by` is
/// the nullable audit FK, also under its attname.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueLinkLiteRow<'a> {
    pub id: &'a str,
    pub issue_id: &'a str,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a serde_json::Value,
    pub created_by_id: Option<&'a str>,
    pub created_at: &'a str,
}

/// `IssueLinkLiteSerializer.to_representation` output (`:852-866`), in
/// `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueLinkLiteView<'a> {
    pub id: &'a str,
    pub issue_id: &'a str,
    pub title: Option<&'a str>,
    pub url: &'a str,
    pub metadata: &'a serde_json::Value,
    pub created_by_id: Option<&'a str>,
    pub created_at: &'a str,
}

/// Port of `IssueLinkLiteSerializer` (`:852-866`). Field-for-field copy.
pub fn issue_link_lite_to_representation<'a>(
    row: &'a IssueLinkLiteRow<'a>,
) -> IssueLinkLiteView<'a> {
    IssueLinkLiteView {
        id: row.id,
        issue_id: row.issue_id,
        title: row.title,
        url: row.url,
        metadata: row.metadata,
        created_by_id: row.created_by_id,
        created_at: row.created_at,
    }
}

/// `IssueAttachmentLiteSerializer.Meta.fields` (`:887-896`), wire order.
/// `issue_id` stays commented out (`:891`) — ported as absence.
pub const ISSUE_ATTACHMENT_LITE_FIELDS: [&str; 7] = [
    "id",
    "asset",
    "attributes",
    "created_by",
    "updated_at",
    "updated_by",
    "asset_url",
];

/// A `FileAsset` row for lite rendering (`db/models/asset.py:28-62`).
/// `asset` is `None` when the stored name is empty (DRF renders an empty
/// `FileField` as `null`); `attributes` is non-null JSON; `asset_url` is
/// the caller-resolved `asset.py:79-100` property (`None` unless the
/// entity type maps to a route).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueAttachmentLiteRow<'a> {
    pub id: &'a str,
    pub asset: Option<&'a str>,
    pub attributes: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_at: &'a str,
    pub updated_by: Option<&'a str>,
    pub asset_url: Option<&'a str>,
}

/// `IssueAttachmentLiteSerializer.to_representation` output (`:884-899`),
/// in `Meta.fields` order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IssueAttachmentLiteView<'a> {
    pub id: &'a str,
    pub asset: Option<&'a str>,
    pub attributes: &'a serde_json::Value,
    pub created_by: Option<&'a str>,
    pub updated_at: &'a str,
    pub updated_by: Option<&'a str>,
    pub asset_url: Option<&'a str>,
}

/// Port of `IssueAttachmentLiteSerializer` (`:884-899`). Field-for-field
/// copy of the default (no-`expand`) body.
pub fn issue_attachment_lite_to_representation<'a>(
    row: &'a IssueAttachmentLiteRow<'a>,
) -> IssueAttachmentLiteView<'a> {
    IssueAttachmentLiteView {
        id: row.id,
        asset: row.asset,
        attributes: row.attributes,
        created_by: row.created_by,
        updated_at: row.updated_at,
        updated_by: row.updated_by,
        asset_url: row.asset_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        let path = format!(
            "{}/../../fixtures/app_issues/serializers/FX-ISS-04.links.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(&path).expect("golden exists"))
            .expect("golden parses")
    }

    fn fields_in_order(fixture: &Value, case: &str) -> Vec<String> {
        fixture
            .get(case)
            .unwrap_or_else(|| panic!("golden lacks {case}"))
            .get("fields_in_order")
            .and_then(Value::as_array)
            .expect("fields_in_order array")
            .iter()
            .map(|key| key.as_str().expect("key str").to_string())
            .collect()
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

    fn const_keys<const N: usize>(fields: &[&str; N]) -> Vec<String> {
        fields.iter().map(|name| name.to_string()).collect()
    }

    fn user_row() -> UserLiteRow<'static> {
        UserLiteRow {
            id: "11111111-1111-1111-1111-111111111111",
            first_name: "Ada",
            last_name: "Lovelace",
            avatar: "",
            avatar_url: None,
            is_bot: false,
            display_name: "Ada Lovelace",
        }
    }

    #[test]
    fn field_consts_match_fx_iss_04() {
        let golden = fixture();
        assert_eq!(
            const_keys(&GITHUB_PR_LINK_FIELDS),
            fields_in_order(&golden, "github_pr_link"),
        );
        assert_eq!(
            const_keys(&GIT_CODE_REVIEW_LINK_FIELDS),
            fields_in_order(&golden, "git_code_review_link"),
        );
        assert_eq!(
            const_keys(&ISSUE_LINK_LITE_FIELDS),
            fields_in_order(&golden, "issue_link_lite"),
        );
        assert_eq!(
            const_keys(&ISSUE_ATTACHMENT_LITE_FIELDS),
            fields_in_order(&golden, "issue_attachment_lite"),
        );
        // The nested user shape pins values, not a key array, in FX-ISS-04
        // ("UserLiteSerializer(read_only, source=created_by)"); its order is
        // asserted through the serialized nests below.
        assert_eq!(USER_LITE_FIELDS.len(), 7);
    }

    #[test]
    fn github_pr_link_replays_probe_bytes() {
        // Live-DRF probe vector (github_pr.full): every key populated,
        // draft pr with a null-free snapshot.
        let row = GithubPullRequestLinkRow {
            id: "55555555-5555-5555-5555-555555555555",
            issue: "44444444-4444-4444-4444-444444444444",
            repo_owner: "acme-corp",
            repo_name: "web",
            pr_number: 42,
            url: "https://github.com/acme-corp/web/pull/42",
            title: "Fix button",
            state: "open",
            merged: false,
            draft: true,
            pr_updated_at: Some("2026-09-02T01:02:03Z"),
            created_at: "2026-09-01T12:34:56.789000Z",
            updated_at: "2026-09-02T01:02:03Z",
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            created_by_detail: Some(user_row()),
        };
        let view = github_pull_request_link_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&GITHUB_PR_LINK_FIELDS));
        assert_eq!(
            serialized_keys(view.created_by_detail.as_ref().expect("nest present")),
            const_keys(&USER_LITE_FIELDS),
            "nested keys follow app UserLiteSerializer order",
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"55555555-5555-5555-5555-555555555555","issue":"44444444-4444-4444-4444-444444444444","repo_owner":"acme-corp","repo_name":"web","pr_number":42,"url":"https://github.com/acme-corp/web/pull/42","title":"Fix button","state":"open","merged":false,"draft":true,"pr_updated_at":"2026-09-02T01:02:03Z","created_at":"2026-09-01T12:34:56.789000Z","updated_at":"2026-09-02T01:02:03Z","created_by":"11111111-1111-1111-1111-111111111111","created_by_detail":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"","avatar_url":null,"is_bot":false,"display_name":"Ada Lovelace"}}"#,
        );
    }

    #[test]
    fn github_pr_link_nulls_render_present_nulls() {
        // Live-DRF probe vector (github_pr.nulls): a null `created_by` FK
        // renders both `created_by` and the nest as present nulls.
        let row = GithubPullRequestLinkRow {
            id: "55555555-5555-5555-5555-555555555555",
            issue: "44444444-4444-4444-4444-444444444444",
            repo_owner: "acme-corp",
            repo_name: "web",
            pr_number: 42,
            url: "https://github.com/acme-corp/web/pull/42",
            title: "Fix button",
            state: "open",
            merged: false,
            draft: true,
            pr_updated_at: None,
            created_at: "2026-09-01T12:34:56.789000Z",
            updated_at: "2026-09-02T01:02:03Z",
            created_by: None,
            created_by_detail: None,
        };
        let view = github_pull_request_link_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&GITHUB_PR_LINK_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"55555555-5555-5555-5555-555555555555","issue":"44444444-4444-4444-4444-444444444444","repo_owner":"acme-corp","repo_name":"web","pr_number":42,"url":"https://github.com/acme-corp/web/pull/42","title":"Fix button","state":"open","merged":false,"draft":true,"pr_updated_at":null,"created_at":"2026-09-01T12:34:56.789000Z","updated_at":"2026-09-02T01:02:03Z","created_by":null,"created_by_detail":null}"#,
        );
    }

    #[test]
    fn git_code_review_link_replays_probe_bytes() {
        // Live-DRF probe vector (git_review.full): empty-string charfields,
        // null remote timestamp, one-entry metadata object.
        let metadata = serde_json::json!({"labels": ["a"]});
        let row = GitCodeReviewLinkRow {
            id: "66666666-6666-6666-6666-666666666666",
            issue: "44444444-4444-4444-4444-444444444444",
            provider: "gitlab",
            host_url: "https://gitlab.com",
            namespace: "acme",
            repo_name: "web",
            repo_external_id: "",
            external_id: "",
            external_iid: "7",
            url: "https://gitlab.com/acme/web/-/merge_requests/7",
            title: "",
            state: "merged",
            merged: true,
            draft: false,
            remote_updated_at: None,
            metadata: &metadata,
            created_at: "2026-09-01T12:34:56.789000Z",
            updated_at: "2026-09-02T01:02:03Z",
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            created_by_detail: Some(user_row()),
        };
        let view = git_code_review_link_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&GIT_CODE_REVIEW_LINK_FIELDS)
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"66666666-6666-6666-6666-666666666666","issue":"44444444-4444-4444-4444-444444444444","provider":"gitlab","host_url":"https://gitlab.com","namespace":"acme","repo_name":"web","repo_external_id":"","external_id":"","external_iid":"7","url":"https://gitlab.com/acme/web/-/merge_requests/7","title":"","state":"merged","merged":true,"draft":false,"remote_updated_at":null,"metadata":{"labels":["a"]},"created_at":"2026-09-01T12:34:56.789000Z","updated_at":"2026-09-02T01:02:03Z","created_by":"11111111-1111-1111-1111-111111111111","created_by_detail":{"id":"11111111-1111-1111-1111-111111111111","first_name":"Ada","last_name":"Lovelace","avatar":"","avatar_url":null,"is_bot":false,"display_name":"Ada Lovelace"}}"#,
        );
    }

    #[test]
    fn git_code_review_link_null_creator_renders_null_nest() {
        // Source-derived arm (created_by null=True, mixins.py:29-35): same
        // present-null pair as the probed PR-link null arm.
        let metadata = serde_json::json!({});
        let row = GitCodeReviewLinkRow {
            id: "66666666-6666-6666-6666-666666666666",
            issue: "44444444-4444-4444-4444-444444444444",
            provider: "github",
            host_url: "https://github.com",
            namespace: "acme",
            repo_name: "web",
            repo_external_id: "99",
            external_id: "100",
            external_iid: "42",
            url: "https://github.com/acme/web/pull/42",
            title: "Fix button",
            state: "open",
            merged: false,
            draft: false,
            remote_updated_at: Some("2026-09-02T01:02:03Z"),
            metadata: &metadata,
            created_at: "2026-09-01T12:34:56.789000Z",
            updated_at: "2026-09-02T01:02:03Z",
            created_by: None,
            created_by_detail: None,
        };
        let produced =
            serde_json::to_value(git_code_review_link_to_representation(&row)).expect("serializes");
        assert_eq!(produced["created_by"], Value::Null);
        assert_eq!(produced["created_by_detail"], Value::Null);
        assert!(produced
            .as_object()
            .expect("object")
            .contains_key("created_by_detail"));
    }

    #[test]
    fn issue_link_lite_replays_probe_bytes() {
        // Live-DRF probe vector (issue_link_lite.full): attname FKs render
        // as UUID strings, exactly like the FK names.
        let metadata = serde_json::json!({});
        let row = IssueLinkLiteRow {
            id: "77777777-7777-7777-7777-777777777777",
            issue_id: "44444444-4444-4444-4444-444444444444",
            title: Some("spec"),
            url: "https://example.com/spec",
            metadata: &metadata,
            created_by_id: Some("11111111-1111-1111-1111-111111111111"),
            created_at: "2026-09-01T12:34:56.789000Z",
        };
        let view = issue_link_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_LINK_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"77777777-7777-7777-7777-777777777777","issue_id":"44444444-4444-4444-4444-444444444444","title":"spec","url":"https://example.com/spec","metadata":{},"created_by_id":"11111111-1111-1111-1111-111111111111","created_at":"2026-09-01T12:34:56.789000Z"}"#,
        );
    }

    #[test]
    fn issue_link_lite_nulls_render_present_nulls() {
        // Live-DRF probe vector (issue_link_lite.nulls): nullable title
        // (issue.py:472) and nullable created_by attname.
        let metadata = serde_json::json!({});
        let row = IssueLinkLiteRow {
            id: "77777777-7777-7777-7777-777777777777",
            issue_id: "44444444-4444-4444-4444-444444444444",
            title: None,
            url: "https://example.com/spec",
            metadata: &metadata,
            created_by_id: None,
            created_at: "2026-09-01T12:34:56.789000Z",
        };
        let view = issue_link_lite_to_representation(&row);
        assert_eq!(serialized_keys(&view), const_keys(&ISSUE_LINK_LITE_FIELDS));
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"77777777-7777-7777-7777-777777777777","issue_id":"44444444-4444-4444-4444-444444444444","title":null,"url":"https://example.com/spec","metadata":{},"created_by_id":null,"created_at":"2026-09-01T12:34:56.789000Z"}"#,
        );
    }

    #[test]
    fn issue_attachment_lite_replays_probe_bytes() {
        // Live-DRF probe vector (attachment_lite.issue_attachment): `asset`
        // is the stored path verbatim (S3Storage.url returns the name) and
        // `asset_url` is the resolved ISSUE_ATTACHMENT route — no `issue_id`
        // key (:891 stays commented out).
        let attributes = serde_json::json!({"kind": "png"});
        let row = IssueAttachmentLiteRow {
            id: "88888888-8888-8888-8888-888888888888",
            asset: Some("22222222-2222-2222-2222-222222222222/ab12-shot.png"),
            attributes: &attributes,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_at: "2026-09-02T01:02:03Z",
            updated_by: None,
            asset_url: Some("/api/assets/v2/workspaces/acme/projects/33333333-3333-3333-3333-333333333333/issues/44444444-4444-4444-4444-444444444444/attachments/88888888-8888-8888-8888-888888888888/"),
        };
        let view = issue_attachment_lite_to_representation(&row);
        assert_eq!(
            serialized_keys(&view),
            const_keys(&ISSUE_ATTACHMENT_LITE_FIELDS)
        );
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"88888888-8888-8888-8888-888888888888","asset":"22222222-2222-2222-2222-222222222222/ab12-shot.png","attributes":{"kind":"png"},"created_by":"11111111-1111-1111-1111-111111111111","updated_at":"2026-09-02T01:02:03Z","updated_by":null,"asset_url":"/api/assets/v2/workspaces/acme/projects/33333333-3333-3333-3333-333333333333/issues/44444444-4444-4444-4444-444444444444/attachments/88888888-8888-8888-8888-888888888888/"}"#,
        );
    }

    #[test]
    fn issue_attachment_lite_unmapped_entity_renders_null_url() {
        // Live-DRF probe vector (attachment_lite.no_entity): the property
        // returns None off the mapped entity types (asset.py:79-100), and
        // the ReadOnlyField renders it as a present null.
        let attributes = serde_json::json!({"kind": "png"});
        let row = IssueAttachmentLiteRow {
            id: "88888888-8888-8888-8888-888888888888",
            asset: Some("22222222-2222-2222-2222-222222222222/ab12-shot.png"),
            attributes: &attributes,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_at: "2026-09-02T01:02:03Z",
            updated_by: None,
            asset_url: None,
        };
        let view = issue_attachment_lite_to_representation(&row);
        assert_eq!(
            serde_json::to_string(&view).expect("serializes"),
            r#"{"id":"88888888-8888-8888-8888-888888888888","asset":"22222222-2222-2222-2222-222222222222/ab12-shot.png","attributes":{"kind":"png"},"created_by":"11111111-1111-1111-1111-111111111111","updated_at":"2026-09-02T01:02:03Z","updated_by":null,"asset_url":null}"#,
        );
    }

    #[test]
    fn issue_attachment_lite_empty_asset_renders_null() {
        // Live-DRF probe vector (attachment_lite.empty_asset): `FileField`
        // renders a falsy value as null (`if not value`, fields.py:1539).
        let attributes = serde_json::json!({"kind": "png"});
        let row = IssueAttachmentLiteRow {
            id: "88888888-8888-8888-8888-888888888888",
            asset: None,
            attributes: &attributes,
            created_by: Some("11111111-1111-1111-1111-111111111111"),
            updated_at: "2026-09-02T01:02:03Z",
            updated_by: None,
            asset_url: Some("/api/assets/v2/workspaces/acme/projects/33333333-3333-3333-3333-333333333333/issues/44444444-4444-4444-4444-444444444444/attachments/88888888-8888-8888-8888-888888888888/"),
        };
        let produced = serde_json::to_value(issue_attachment_lite_to_representation(&row))
            .expect("serializes");
        assert_eq!(produced["asset"], Value::Null);
        assert!(produced.as_object().expect("object").contains_key("asset"));
    }
}
