#![forbid(unsafe_code)]

//! The three app search handlers (D-29, PIDASHCONV-276).
//!
//! Ports `apps/api/pi_dash/app/views/search/base.py`
//! (`GlobalSearchEndpoint.get`, `SearchEndpoint.get`) and
//! `apps/api/pi_dash/app/views/search/issue.py` (`IssueSearchEndpoint.get`)
//! with identical URL paths, status codes and JSON byte for byte. Route
//! patterns live in `apps/api/pi_dash/app/urls/search.py` (3 patterns).
//!
//! Query construction lives in
//! [`pidash_services::app_views_search::queries_search`]; this file owns
//! param parsing at the HTTP boundary, the issue pipeline's small lookups,
//! row fetching (via [`super::fetch_shaped`]) and response assembly.
//!
//! Ported bugs (translate, don't redesign — also listed in the PR):
//!
//! * B5 (`views/search/issue.py:77-80`): `filter_root_issues_only` reads
//!   `issue.parent` outside the `if issue:` guard, so `sub_issue=true`
//!   with an unknown `issue_id` raises `AttributeError` → 500 instead of
//!   returning root issues. Unknown ids 500 here too.
//! * B7 (`views/search/base.py:230`): `filter_intakes` uses `Issue.objects`
//!   instead of `Issue.issue_objects` — the intake section keeps the
//!   soft-delete arm only (see the services builder).
//! * B8 (`views/search/base.py:297`): `count = int(…)` is unguarded —
//!   non-numeric counts raise `ValueError` → 500 (never 400), and negative
//!   counts 500 on Django's negative-slicing assertion. Both 500 here.
//! * B9 (`views/search/base.py:174-189`): the pages `ArrayAgg` filter
//!   `~Q(projects__id=True)` coerces `True` to the impossible
//!   `00000000-…-000000000001` UUID — a no-op for real data, kept in the
//!   services builder.
//! * Comment widening has no `IssueComment.access` filter
//!   (`search/issue.py:78-83`, limitation ported as-is): INTERNAL comment
//!   text can surface an issue to a member who cannot read that comment.

use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;

use pidash_services::app_views_search::queries_search::{
    self, EntityScope, GlobalScope, IssueSearchError, IssueSearchInput,
};

use super::{
    actor_user_id, fetch_shaped, json_response, owned, parse_uuid_or_invalid, primary, query_last,
    resolve_project_id, Col, Denial, QueryMap,
};
use crate::state::AppState;

/// Register the three search GET routes. Nothing else: sibling paths stay
/// unmatched and proxy to Django through the fallback.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/workspaces/{slug}/search/", owned(get(global_search)))
        .route(
            "/api/workspaces/{slug}/projects/{project_id}/search-issues/",
            owned(get(issue_search)),
        )
        .route(
            "/api/workspaces/{slug}/entity-search/",
            owned(get(entity_search)),
        )
}

// ---------------------------------------------------------------------------
// Global search
// ---------------------------------------------------------------------------

/// `GlobalSearchEndpoint.get` (`search/base.py:255-286`): one section query
/// per requested entity, `{"results": {<entity>: [...]}}` with HTTP 200.
/// Sections iterate the request's entity order (default: all 8 mapper
/// keys); unknown names were already dropped by `requested_entities`.
async fn global_search(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = primary(&state)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let user_text = user_id.to_string();

    let search = query_last(&query, "search");
    let entities = queries_search::requested_entities(query_last(&query, "entities").as_deref());
    let workspace_search =
        query_last(&query, "workspace_search").unwrap_or_else(|| "false".to_owned());
    let project_raw = query_last(&query, "project_id");
    // The narrow applies only when `workspace_search == "false"` AND the
    // param is truthy (`base.py:99-100`); a badly-formed UUID there raises
    // `ValidationError` in the filter → 400.
    let narrow = queries_search::project_narrow(&workspace_search, project_raw.as_deref());
    // `filter_workspaces` / `filter_projects` (`base.py:54-84`) never apply
    // the narrow, so a badly-formed UUID with only those sections requested
    // stays 200 in Django — validate only when a requested section consumes
    // it (anything other than `workspace` / `project`).
    if narrow.is_some()
        && entities
            .iter()
            .any(|e| *e != "workspace" && *e != "project")
    {
        parse_uuid_or_invalid(narrow.unwrap_or_default())?;
    }
    let scope = GlobalScope {
        user_id: &user_text,
        workspace_slug: &slug,
        narrow_project_id: narrow,
    };
    let search_ref = search.as_deref();

    let mut sections = Vec::with_capacity(entities.len());
    for entity in entities {
        let rows = match entity {
            "workspace" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_workspaces(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("slug", 2, Col::Text),
                    ],
                )
                .await?
            }
            "project" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_projects(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("identifier", 2, Col::Text),
                        ("workspace__slug", 3, Col::Text),
                    ],
                )
                .await?
            }
            "issue" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_issues(&scope, search_ref),
                    &GLOBAL_ISSUE_SPEC,
                )
                .await?
            }
            "cycle" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_cycles(&scope, search_ref),
                    &GLOBAL_NAMED_SPEC,
                )
                .await?
            }
            "module" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_modules(&scope, search_ref),
                    &GLOBAL_NAMED_SPEC,
                )
                .await?
            }
            "issue_view" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_views(&scope, search_ref),
                    &GLOBAL_NAMED_SPEC,
                )
                .await?
            }
            // Output order follows Django's compiler, not the `.values()`
            // call order: concrete columns first, annotations
            // (`project_ids`, `project_identifiers`) last
            // (`base.py:202` vs the emitted `SELECT`).
            "page" => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_pages(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("workspace__slug", 4, Col::Text),
                        ("project_ids", 2, Col::UuidArray),
                        ("project_identifiers", 3, Col::StrArray),
                    ],
                )
                .await?
            }
            // `requested_entities` only yields mapper keys, so the
            // remainder is `"intake"`.
            _ => {
                fetch_shaped(
                    &pool,
                    &queries_search::global_intakes(&scope, search_ref),
                    &GLOBAL_ISSUE_SPEC,
                )
                .await?
            }
        };
        sections.push(format!("\"{entity}\":[{}]", rows.join(",")));
    }
    Ok(json_response(format!(
        "{{\"results\":{{{}}}}}",
        sections.join(",")
    )))
}

/// Row spec for the global issue + intake sections
/// (`values("name", "id", "sequence_id", "project__identifier",
/// "project_id", "workspace__slug")`, `base.py:102-109,245-252`).
/// Output order matches `SELECT` order here (no annotations).
const GLOBAL_ISSUE_SPEC: [(&str, usize, Col); 6] = [
    ("name", 0, Col::Text),
    ("id", 1, Col::Uuid),
    ("sequence_id", 2, Col::Int),
    ("project__identifier", 3, Col::Text),
    ("project_id", 4, Col::Uuid),
    ("workspace__slug", 5, Col::Text),
];

/// Row spec for the global cycle / module / view sections
/// (`values("name", "id", "project_id", "project__identifier",
/// "workspace__slug")`, `base.py:132,156,226`).
const GLOBAL_NAMED_SPEC: [(&str, usize, Col); 5] = [
    ("name", 0, Col::Text),
    ("id", 1, Col::Uuid),
    ("project_id", 2, Col::Uuid),
    ("project__identifier", 3, Col::Text),
    ("workspace__slug", 4, Col::Text),
];

// ---------------------------------------------------------------------------
// Entity search
// ---------------------------------------------------------------------------

/// `SearchEndpoint.get` (`search/base.py:293-691`): one branch query per
/// requested known `query_type`, `{<type>: [...]}` with HTTP 200, keys in
/// request order. Unknown types are silently ignored (no key emitted).
async fn entity_search(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = primary(&state)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let user_text = user_id.to_string();

    let search = query_last(&query, "query");
    // B8 port: `int()` raises `ValueError` → generic 500, and a parsed
    // negative 500s on Django's negative-slicing assertion.
    let count = match queries_search::parse_count(query_last(&query, "count").as_deref()) {
        Ok(n) if n >= 0 => n,
        _ => return Err(Denial::ServerError),
    };
    // A truthy `project_id` selects the project branch; falsy (absent or
    // `""`) the workspace branch. Django only touches `project_id` inside
    // branches that filter on it — the `project` branch (`base.py:353-370`)
    // never filters on it and unknown types touch nothing — so a
    // badly-formed UUID there stays 200. Validate only when an executed
    // branch consumes it (`user_mention` / `issue` / `cycle` / `module` /
    // `page`). `response_data` is a dict (`base.py:504`), so duplicate
    // `query_type` entries collapse to one key — dedupe preserving order.
    let project_raw = query_last(&query, "project_id").filter(|v| !v.is_empty());
    let mut query_types: Vec<String> = Vec::new();
    for qt in queries_search::split_query_types(query_last(&query, "query_type").as_deref()) {
        if !query_types.contains(&qt) {
            query_types.push(qt);
        }
    }
    if project_raw.is_some()
        && query_types.iter().any(|qt| {
            matches!(
                qt.as_str(),
                "user_mention" | "issue" | "cycle" | "module" | "page"
            )
        })
    {
        parse_uuid_or_invalid(project_raw.as_deref().unwrap_or_default())?;
    }
    let scope = EntityScope {
        user_id: &user_text,
        workspace_slug: &slug,
        project_id: project_raw.as_deref(),
        count,
    };
    let search_ref = search.as_deref();
    // `timezone.now()`: UTC, bound four times by the cycle builder.
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false);

    let mut branches = Vec::new();
    for query_type in &query_types {
        let rows = match query_type.as_str() {
            // Key order follows Django's compiler: the `member__avatar_url`
            // annotation selects last even though `.values()` names it
            // first (`base.py:345-349` vs the emitted `SELECT`).
            "user_mention" => {
                let built = queries_search::entity_user_mention(&scope, search_ref);
                // Positions are explicit, so one spec serves both branches
                // (the project branch's trailing `created_at` is simply
                // never referenced; the workspace branch selects no
                // trailing column at all — `base.py:542`).
                let spec: [(&str, usize, Col); 3] = [
                    ("member__display_name", 0, Col::Text),
                    ("member__id", 1, Col::Uuid),
                    ("member__avatar_url", 2, Col::Text),
                ];
                fetch_shaped(&pool, &built, &spec).await?
            }
            "project" => {
                fetch_shaped(
                    &pool,
                    &queries_search::entity_project(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("identifier", 2, Col::Text),
                        ("logo_props", 3, Col::Json),
                        ("workspace__slug", 4, Col::Text),
                    ],
                )
                .await?
            }
            "issue" => {
                fetch_shaped(
                    &pool,
                    &queries_search::entity_issue(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("sequence_id", 2, Col::Int),
                        ("project__identifier", 3, Col::Text),
                        ("project_id", 4, Col::Uuid),
                        ("priority", 5, Col::Text),
                        ("state_id", 6, Col::Uuid),
                        ("type_id", 7, Col::Uuid),
                    ],
                )
                .await?
            }
            // The `status` annotation selects last (compiler order), even
            // though `.values()` names it before `workspace__slug`; its
            // `SELECT` position stays 4.
            "cycle" => {
                fetch_shaped(
                    &pool,
                    &queries_search::entity_cycle(&scope, search_ref, &now),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("project_id", 2, Col::Uuid),
                        ("project__identifier", 3, Col::Text),
                        ("workspace__slug", 5, Col::Text),
                        ("status", 4, Col::Text),
                    ],
                )
                .await?
            }
            "module" => {
                fetch_shaped(
                    &pool,
                    &queries_search::entity_module(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("project_id", 2, Col::Uuid),
                        ("project__identifier", 3, Col::Text),
                        ("status", 4, Col::Text),
                        ("workspace__slug", 5, Col::Text),
                    ],
                )
                .await?
            }
            "page" => {
                fetch_shaped(
                    &pool,
                    &queries_search::entity_page(&scope, search_ref),
                    &[
                        ("name", 0, Col::Text),
                        ("id", 1, Col::Uuid),
                        ("logo_props", 2, Col::Json),
                        ("projects__id", 3, Col::Uuid),
                        ("workspace__slug", 4, Col::Text),
                    ],
                )
                .await?
            }
            // Unknown types emit no key (`base.py` simply has no branch).
            _ => continue,
        };
        branches.push(format!("\"{query_type}\":[{}]", rows.join(",")));
    }
    Ok(json_response(format!("{{{}}}", branches.join(","))))
}

// ---------------------------------------------------------------------------
// Issue search
// ---------------------------------------------------------------------------

/// `IssueSearchEndpoint.get` (`search/issue.py:104-166`): the base scope
/// plus whichever query-param branches activate, rendered as a bare JSON
/// list (`values(…)` 11 keys, `[:100]`) with HTTP 200.
async fn issue_search(
    State(state): State<AppState>,
    Path((slug, project_raw)): Path<(String, String)>,
    Query(query): Query<QueryMap>,
    extension: Option<axum::Extension<crate::middleware::SessionHandle>>,
) -> Result<Response, Denial> {
    let pool = primary(&state)?;
    let user_id = actor_user_id(extension).ok_or(Denial::Unauthorized)?;
    let user_text = user_id.to_string();

    // `_rewrite_project_kwarg` runs in `initial()`, before auth-gated
    // `get()`: UUIDs pass through unchecked; identifiers resolve or 404.
    let project_id = resolve_project_id(&pool, &slug, &project_raw).await?;

    let search = query_last(&query, "search");
    let workspace_search =
        query_last(&query, "workspace_search").unwrap_or_else(|| "false".to_owned());
    let flag = |key: &str| query_last(&query, key).as_deref() == Some("true");
    let parent = flag("parent");
    let issue_relation = flag("issue_relation");
    let sub_issue = flag("sub_issue");
    let cycle = flag("cycle");
    let module = query_last(&query, "module").filter(|v| !v.is_empty());
    let target_date_none = query_last(&query, "target_date").as_deref() == Some("none");
    let issue_id = query_last(&query, "issue_id").filter(|v| !v.is_empty());

    // Filters built on `issue_id` execute `.filter(pk=…)` eagerly via
    // `.first()` / the relation collection — a badly-formed UUID raises
    // `ValidationError` → 400. With no consuming flag the param is never
    // touched (stays 200). Same for a truthy `module` (`issue.py:90-95`).
    let consumes_issue_id = parent || issue_relation || sub_issue;
    if consumes_issue_id && issue_id.is_some() {
        parse_uuid_or_invalid(issue_id.as_deref().unwrap_or_default())?;
    }
    if let Some(ref mid) = module {
        parse_uuid_or_invalid(mid)?;
    }

    // DB-resolved inputs for the builder. The parent lookup runs under the
    // `Issue.issue_objects` scope (`issue.py:44,54,73`): a triage, draft,
    // archived or soft-deleted row reads as unknown (`.first()` is None).
    let mut issue_parent_id: Option<Option<uuid::Uuid>> = None;
    let mut related_ids: Vec<String> = Vec::new();
    if consumes_issue_id {
        if let Some(ref xid) = issue_id {
            let xid_uuid = xid
                .parse::<uuid::Uuid>()
                .map_err(|_| Denial::InvalidDetail)?;
            match lookup_issue_parent(&pool, &xid_uuid).await? {
                // Unknown `issue_id`: the parent and relation filters are
                // guarded by `if issue:` (no-op); the root-only filter is NOT
                // (B5) — the builder 500s instead of filtering.
                None => {
                    if sub_issue {
                        return Err(Denial::ServerError);
                    }
                }
                Some(parent_id) => {
                    issue_parent_id = Some(parent_id);
                    if issue_relation {
                        related_ids = lookup_related_ids(&pool, &xid_uuid).await?;
                        related_ids.push(xid.clone());
                    }
                }
            }
        }
    }
    let issue_parent_text: Option<Option<String>> =
        issue_parent_id.map(|inner| inner.map(|id| id.to_string()));
    let issue_parent_ref: Option<Option<&str>> = issue_parent_text
        .as_ref()
        .map(|inner| inner.as_ref().map(String::as_str));

    // Guest scoping (`issue.py:146-149`): an active `role=5` membership on
    // the URL project narrows to rows the user created.
    let guest = is_guest(&pool, &project_id, &user_id).await?;

    let url_project_text = project_id.to_string();
    let input = IssueSearchInput {
        slug: &slug,
        url_project_id: &url_project_text,
        user_id: &user_text,
        query: search.as_deref().filter(|v| !v.is_empty()),
        workspace_search: &workspace_search,
        parent,
        issue_relation,
        sub_issue,
        cycle,
        module: module.as_deref(),
        target_date_none,
        issue_id: issue_id.as_deref(),
        issue_parent_id: issue_parent_ref,
        related_ids,
        guest,
    };
    // B5 surfaces as `Err(UnknownIssue)` when the builder meets an unknown
    // id under `sub_issue` (also guarded above — belt and braces).
    let built = match queries_search::build_issue_search(&input) {
        Ok(built) => built,
        Err(IssueSearchError::UnknownIssue) => return Err(Denial::ServerError),
    };
    let rows = fetch_shaped(
        &pool,
        &built,
        &[
            ("name", 0, Col::Text),
            ("id", 1, Col::Uuid),
            ("start_date", 2, Col::Date),
            ("sequence_id", 3, Col::Int),
            ("project__name", 4, Col::Text),
            ("project__identifier", 5, Col::Text),
            ("project_id", 6, Col::Uuid),
            ("workspace__slug", 7, Col::Text),
            ("state__name", 8, Col::Text),
            ("state__group", 9, Col::Text),
            ("state__color", 10, Col::Text),
        ],
    )
    .await?;
    Ok(json_response(format!("[{}]", rows.join(","))))
}

/// `Issue.issue_objects.filter(pk=…).first()` (`issue.py:44,54,73`): the
/// row's `parent_id` under the manager scope (soft-delete, triage-state,
/// archived issue/project, drafts — `db/models/issue.py:95-104`, same arms
/// the section builders use). `None` = unknown id (`.first()` is None).
async fn lookup_issue_parent(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Option<Option<uuid::Uuid>>, Denial> {
    let row: Option<(Option<uuid::Uuid>,)> = sqlx::query_as(
        r#"SELECT issues.parent_id FROM issues
           LEFT OUTER JOIN states ON (issues.state_id = states.id)
           INNER JOIN projects ON (issues.project_id = projects.id)
           WHERE issues.id = $1
             AND issues.deleted_at IS NULL
             AND NOT (states.group = 'triage' AND states.group IS NOT NULL)
             AND NOT (issues.archived_at IS NOT NULL)
             AND NOT (projects.archived_at IS NOT NULL)
             AND NOT (issues.is_draft)"#,
    )
    .bind(issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.map(|row| row.0))
}

/// `filter_issues_excluding_related_issues` (`issue.py:52-70`): both
/// `IssueRelation` columns touching the issue, flattened
/// (`values_list("issue_id", "related_issue_id")`), soft-deleted rows
/// excluded via the manager. The caller appends `issue_id` itself.
async fn lookup_related_ids(
    pool: &sqlx::PgPool,
    issue_id: &uuid::Uuid,
) -> Result<Vec<String>, Denial> {
    let rows: Vec<(uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
        r#"SELECT issue_id, related_issue_id FROM issue_relations
           WHERE (issue_id = $1 OR related_issue_id = $1) AND deleted_at IS NULL"#,
    )
    .bind(issue_id)
    .fetch_all(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    let mut ids = Vec::with_capacity(rows.len() * 2);
    for (left, right) in rows {
        ids.push(left.to_string());
        ids.push(right.to_string());
    }
    Ok(ids)
}

/// Guest scoping probe (`issue.py:146-149`): an active `role=5`
/// (`ROLE.GUEST`) `ProjectMember` row for this user on the URL project.
async fn is_guest(
    pool: &sqlx::PgPool,
    project_id: &uuid::Uuid,
    user_id: &uuid::Uuid,
) -> Result<bool, Denial> {
    let row: Option<(i32,)> = sqlx::query_as(
        r#"SELECT 1 FROM project_members
           WHERE project_id = $1 AND member_id = $2 AND is_active AND role = 5
             AND deleted_at IS NULL LIMIT 1"#,
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| Denial::ServerError)?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::super::{query_last, OneOrMany, QueryMap};
    use super::routes;

    fn query_map(pairs: &[(&str, &str)]) -> QueryMap {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OneOrMany::One((*value).to_owned())))
            .collect()
    }

    #[test]
    fn denial_bodies_replay_drf_bytes() {
        use super::super::{
            INVALID_DETAIL_BODY, PROJECT_NOT_FOUND_BODY, SERVER_ERROR_BODY, UNAUTHENTICATED_BODY,
        };
        assert_eq!(
            UNAUTHENTICATED_BODY,
            r#"{"detail":"Authentication credentials were not provided."}"#
        );
        assert_eq!(
            INVALID_DETAIL_BODY,
            r#"{"error":"Please provide valid detail"}"#
        );
        // DRF maps `Http404("Project not found")` (the project-kwarg
        // rewrite miss) to `NotFound` with the message kept.
        assert_eq!(PROJECT_NOT_FOUND_BODY, r#"{"detail":"Project not found"}"#);
        assert_eq!(
            SERVER_ERROR_BODY,
            r#"{"error":"Something went wrong please try again later"}"#
        );
    }

    #[test]
    fn query_last_wins_like_querydict_get() {
        let mut map = query_map(&[("search", "first")]);
        map.insert(
            "search".to_owned(),
            OneOrMany::Many(vec!["first".to_owned(), "second".to_owned()]),
        );
        assert_eq!(query_last(&map, "search").as_deref(), Some("second"));
        assert_eq!(query_last(&map, "missing"), None);
    }

    #[test]
    fn search_routes_register_without_conflict() {
        // Construction itself proves no duplicate-path panic against the
        // merged App group (checked again at overlay build).
        let _ = routes();
    }

    #[test]
    fn issue_row_spec_matches_project_search_keys() {
        // The contract's PROJECT_SEARCH_KEYS order is the shape order.
        let keys: Vec<&str> = [
            "name",
            "id",
            "start_date",
            "sequence_id",
            "project__name",
            "project__identifier",
            "project_id",
            "workspace__slug",
            "state__name",
            "state__group",
            "state__color",
        ]
        .to_vec();
        assert_eq!(
            keys,
            pidash_services::app_views_search::queries_search::PROJECT_SEARCH_KEYS
        );
    }
}
