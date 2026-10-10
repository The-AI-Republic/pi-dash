//! Route + job inventory for parity checks (PIDASHCONV-820).
//!
//! [`ROUTES`] is the claimed router inventory: every path the Rust app
//! registers, with the methods it owns and the methods it proxies, per
//! route group. [`TASKS`] is the claimed worker registry: every task
//! name the Rust worker owns or forwards. `routes --json` / `jobs
//! --json` print them; the parity checker
//! (`rust-api/contract-tests/parity/`) diffs them against Django.
//!
//! Truthfulness is pinned, not trusted: the pin tests below probe every
//! [`ROUTES`] row against the real router (all flags on, dead
//! upstream) — owned methods must serve, proxied methods must 502,
//! unlisted methods must 405 — and the mirror test rebuilds the worker
//! registry exactly like `worker()` and compares it against [`TASKS`].
//! Any router or registry drift fails `cargo test` here first.

#[cfg(test)]
use pidash_api::{build_router_with_overlay, AppState, EdgeFlags, EdgeHandle, Overlay, Prefix};
use pidash_jobs::{default_schedule, BeatEntry, Cadence};

/// Tracked HTTP methods, the same set the parity checker diffs.
#[cfg(test)]
pub const TRACKED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// One registered route: the group that serves it (an [`Overlay`]
/// group name; `foundation` is the ungrouped `/healthz`), the exact
/// axum path, and the methods Rust owns vs explicitly proxies. A
/// tracked method in neither list must 405 from axum.
pub struct RouteRow {
    pub group: &'static str,
    pub path: &'static str,
    pub owned: &'static [&'static str],
    pub proxied: &'static [&'static str],
}

/// Route groups in [`Prefix`] order, with their URL prefix (informational).
pub const GROUPS: &[(&str, &str)] = &[
    ("web", ""),
    ("app", "api/"),
    ("assistant", "api/"),
    ("loop", "api/"),
    ("prompting", "api/"),
    ("space", "api/public/"),
    ("license", "api/instances/"),
    ("runner_web", "api/runners/"),
    ("api_v1", "api/v1/"),
    ("runner", "api/v1/runner/"),
    ("auth", "auth/"),
    ("foundation", "(ungrouped)"),
];

/// The claimed router inventory, grouped and path-sorted.
pub const ROUTES: &[RouteRow] = &[
    // Provenance: each row was measured against the real router
    // (all flags on, stub upstream) and spot-checked against the
    // registering source; `pin_routes_match_router` below re-probes
    // every row on each `cargo test` run, so drift fails here first.
    RouteRow { group: "web", path: "/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "web", path: "/robots.txt",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/static/{asset_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/user-assets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/user-assets/{asset_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/check/{asset_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/download/{asset_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/duplicate-assets/{asset_id}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{entity_id}/bulk/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/restore/{asset_id}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/assets/v2/workspaces/{slug}/{asset_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/integrations/github/app/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/integrations/github/app/webhook/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/schema",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"], proxied: &[] },
    RouteRow { group: "app", path: "/api/schema/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/schema/redoc/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/schema/swagger-ui/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/timezones/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/unsplash/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/api-tokens/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/api-tokens/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/file-assets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/file-assets/{asset_key}/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/last-visited-workspace/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/accounts/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/accounts/{pk}/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/activities/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/email/",
        owned: &["PATCH"], proxied: &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/email/generate-code/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/instance-admin/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/integrations/github/app/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/integrations/github/app/install/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/integrations/github/app/refresh/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/notification-preferences/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/onboard/",
        owned: &["PATCH"], proxied: &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/profile/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/settings/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/tour-completed/",
        owned: &["PATCH"], proxied: &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/invitations/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/join-requests/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/{slug}/activity-graph/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/{slug}/dashboard/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/{slug}/issues-completed-graph/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/{slug}/project-roles/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/me/workspaces/{slug}/projects/invitations/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/users/session/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspace-slug-check/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/file-assets/{workspace_id}/{asset_key}/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/file-assets/{workspace_id}/{asset_key}/restore/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/advance-analytics-charts/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/advance-analytics-stats/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/advance-analytics/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/ai-assistant/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/analytic-view/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/analytic-view/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/analytics/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/cycles/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/default-analytics/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/draft-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/draft-issues/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/draft-to-issue/{draft_id}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/entity-search/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/estimates/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/export-analytics/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/export-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/file-assets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/home-preferences/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/home-preferences/{key}/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/git/accounts/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/git/accounts/{account_id}/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/git/accounts/{account_id}/repos/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/git/providers/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/github/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/github/connect/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/github/disconnect/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/integrations/github/repos/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/invitations/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/invitations/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/invitations/{pk}/join/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/join-requests/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/join-requests/{pk}/approve/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/join-requests/{pk}/deny/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/labels/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/members/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/members/leave/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/members/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/modules/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/project-identifiers/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/project-members/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/project-stats/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/details/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-charts/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-stats/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/advance-analytics/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/ai-assistant/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archived-issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archived-modules/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/bulk-archive-issues/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/bulk-create-labels/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/bulk-delete-issues/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/{reaction_code}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/analytics/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/user-properties/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/deleted-issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/estimates/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/estimate-points/{estimate_point_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/favorite-pages/{page_id}/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/github/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/github/bind/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/inboxes/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intake-state/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intakes/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/invitations/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/",
        owned: &["GET", "DELETE", "HEAD"], proxied: &["POST", "PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issue-dates/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issue-labels/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues-detail/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/list/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/code-reviews/{pk}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE"], proxied: &["POST", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/github-pull-requests/{pk}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/history/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE"], proxied: &["POST", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/{subscriber_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/modules/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/{reaction_code}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/remove-relation/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/",
        owned: &["GET", "POST", "DELETE"], proxied: &["PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/",
        owned: &["GET", "POST", "DELETE", "HEAD"], proxied: &["PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/join/{pk}/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/members/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/members/leave/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/members/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/",
        owned: &["GET", "POST", "DELETE", "HEAD"], proxied: &["PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/user-properties/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages-summary/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/access/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/duplicate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/lock/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/project-estimates/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/project-members/me/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/project-views/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/repository/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/repository/bind/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/occurrences/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{binding_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/search-issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/states/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/mark-default/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{cycle_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/{module_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/{view_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/user-properties/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/v2/issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/views/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/quick-links/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/quick-links/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/recent-visits/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/schedulers/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/schedulers/{scheduler_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/search/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/sidebar-preferences/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/states/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/stickies/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/stickies/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-activity/{user_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-activity/{user_id}/export/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-favorite-projects/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-favorite-projects/{project_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-favorites/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-favorites/{favorite_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-favorites/{favorite_id}/group/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-issues/{user_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-profile/{user_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-properties/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/user-stats/{user_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/mark-all-read/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/unread/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/{pk}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/users/notifications/{pk}/read/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/views/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/views/{pk}/",
        owned: &["GET", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/webhook-logs/{webhook_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/webhooks/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/webhooks/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/webhooks/{pk}/regenerate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/work-items/{tail}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/workspace-members/me/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/workspace-themes/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/workspace-themes/{pk}/",
        owned: &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"], proxied: &["OPTIONS"] },
    RouteRow { group: "app", path: "/api/workspaces/{slug}/workspace-views/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/agent-profile/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/agent-token/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/config/",
        owned: &["GET", "PUT", "DELETE"], proxied: &["POST", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/config/test/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/mcp-servers/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/mcp-servers/{server_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/stt-config/",
        owned: &["GET", "PUT", "DELETE"], proxied: &["POST", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/stt-config/test/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/users/me/ai-assistant/transcribe/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/generate-title/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/threads/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/cancel/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/events/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "assistant", path: "/api/workspaces/{slug}/ai-assistant/threads/{thread_id}/messages/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "loop", path: "/api/users/me/auto-pm/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "loop", path: "/api/users/me/auto-pm/jobs/{slug}/",
        owned: &["PATCH"], proxied: &["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "prompting", path: "/api/workspaces/{slug}/prompt-sections",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "prompting", path: "/api/workspaces/{slug}/prompt-sections/{section_key}",
        owned: &["PUT", "DELETE"], proxied: &["GET", "POST", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "prompting", path: "/api/workspaces/{slug}/prompts/{kind}/compiled",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "prompting", path: "/api/workspaces/{slug}/prompts/{kind}/preview",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/{reaction_code}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/cycles/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/intakes/{intake_id}/inbox-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/comments/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/{reaction_code}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/issues/{issue_id}/votes/",
        owned: &["GET", "POST", "DELETE", "HEAD"], proxied: &["PUT", "PATCH", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/labels/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/members/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/meta/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/modules/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/settings/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/anchor/{anchor}/states/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/assets/v2/anchor/{anchor}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/assets/v2/anchor/{anchor}/restore/{pk}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/assets/v2/anchor/{anchor}/{entity_id}/bulk/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/assets/v2/anchor/{anchor}/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/workspaces/{slug}/project-boards/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "space", path: "/api/public/workspaces/{slug}/projects/{project_id}/anchor/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/admins/sign-up-screen-visited/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/configurations/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/configurations/disable-email-feature/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/email-credentials-check/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/loop/jobs/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/loop/jobs/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/loop/jobs/{pk}/targets/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/workspace-slug-check/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "license", path: "/api/instances/workspaces/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/approvals/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/approvals/{approval_id}/decide/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/approvals/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/approvals/{approval_id}/decide/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/cancel/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/close/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/events/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/messages/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/chat/sessions/{session_id}/warm/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/{machine_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/{machine_id}/create-runner/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/{machine_id}/create-runner/{request_id}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/{machine_id}/revoke/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/dev-machines/{machine_id}/rotate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/invites/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/machine-tokens/{workspace_id}/tickets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/pods/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/pods/{pod_id}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/projects/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/re-tick/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/runs/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/runs/{run_id}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/runs/{run_id}/cancel/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/runs/{run_id}/release-pin/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/{runner_id}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/{runner_id}/revive/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner_web", path: "/api/runners/{runner_id}/revoke/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/assets/user-assets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/assets/user-assets/server/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/assets/user-assets/{asset_id}/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/assets/user-assets/{asset_id}/server/",
        owned: &["PATCH", "DELETE"], proxied: &["GET", "POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/device/approve/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/device/start/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/device/token/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/machine-token/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/revoke/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/auth/workspaces/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/runners/{runner_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/users/me/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/agent-runs/{run_id}/yield/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/assets/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/assets/{asset_id}/",
        owned: &["GET", "PATCH", "HEAD"], proxied: &["POST", "PUT", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/invitations/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/invitations/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/issues/search/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/issues/{segment}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/members/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/unarchive/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/unarchive/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/",
        owned: &["GET", "DELETE"], proxied: &["POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/transfer-issues/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/archive/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/labels/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/members/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/modules/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/{issue_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/archive/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/pages/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        owned: &["GET", "PATCH"], proxied: &["POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/pages/{page_id}/archive/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/states/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/summary/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/code-reviews/{pk}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/github/pull-requests/{pk}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/",
        owned: &["GET", "POST"], proxied: &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/grouped/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/relate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/unrelate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/workpad/",
        owned: &["GET", "PATCH"], proxied: &["POST", "PUT", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/",
        owned: &["GET", "PATCH", "DELETE"], proxied: &["POST", "PUT", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/move/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/re-tick/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/run-ai/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/wait/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/stickies/",
        owned: &["GET", "POST", "HEAD"], proxied: &["PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/stickies/{pk}/",
        owned: &["GET", "PATCH", "DELETE", "HEAD"], proxied: &["POST", "PUT", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/work-items/search/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/work-items/search/advanced/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "api_v1", path: "/api/v1/workspaces/{slug}/work-items/{segment}/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/approvals/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/closed/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/events/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/failed/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/complete/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/messages/{message_id}/started/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/chat/sessions/{session_id}/started/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/dev-machines/desktop-enroll/",
        owned: &["POST", "DELETE"], proxied: &["GET", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/dev-machines/{dev_machine_id}/commands/{request_id}/result/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/poll",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/health/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/machine-tokens/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/metrics/",
        owned: &["GET"], proxied: &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/projects/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/enroll/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/{runner_id}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/{runner_id}/refresh/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/{runner_id}/sessions/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/{runner_id}/sessions/{sid}/",
        owned: &["DELETE"], proxied: &["GET", "POST", "PUT", "PATCH", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runners/{runner_id}/sessions/{sid}/poll",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/accept/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/approvals/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/awaiting-reauth/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/cancelled/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/complete/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/events/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/fail/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/pause/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/queued/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/resumed/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/started/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "runner", path: "/api/v1/runner/runs/{run_id}/stream/upgrade/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/change-password/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/email-check/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/forgot-password/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/get-csrf-token/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/gitea/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/gitea/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/github/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/github/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/gitlab/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/gitlab/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/google/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/google/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/magic-generate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/magic-sign-in/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/magic-sign-up/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/reset-password/{uidb64}/{token}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/set-password/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/sign-in/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/sign-out/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/sign-up/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/email-check/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/forgot-password/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/gitea/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/gitea/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/github/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/github/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/gitlab/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/gitlab/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/google/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/google/callback/",
        owned: &["GET", "HEAD"], proxied: &["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/magic-generate/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/magic-sign-in/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/magic-sign-up/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/reset-password/{uidb64}/{token}/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/sign-in/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/sign-out/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "auth", path: "/auth/spaces/sign-up/",
        owned: &["POST"], proxied: &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] },
    RouteRow { group: "foundation", path: "/healthz",
        owned: &["GET", "HEAD"], proxied: &[] },
];

/// Who executes a task name: the Rust worker (registered handler) or
/// the Python plane (forwarded over AMQP in Celery protocol v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Rust,
    Python,
}

impl Owner {
    fn as_str(self) -> &'static str {
        match self {
            Owner::Rust => "rust",
            Owner::Python => "python",
        }
    }
}

/// One task name with its owner. Covers every task Django defines
/// (registered at worker boot or not) plus the Celery builtins.
pub struct TaskRow {
    pub name: &'static str,
    pub owner: Owner,
}

/// The claimed worker registry, name-sorted.
pub const TASKS: &[TaskRow] = &[
    // Provenance: every name Django defines (worker-boot registered
    // or statically defined, plus the Celery builtins); owners mirror
    // `worker()`'s registrations, re-verified by
    // `mirror_registry_matches_tasks_table` on each `cargo test` run.
    TaskRow {
        name: "assistant.run_turn",
        owner: Owner::Python,
    },
    TaskRow {
        name: "assistant.sweep_stale_turns",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.accumulate",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.backend_cleanup",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.chain",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.chord",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.chord_unlock",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.chunks",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.group",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.map",
        owner: Owner::Python,
    },
    TaskRow {
        name: "celery.starmap",
        owner: Owner::Python,
    },
    TaskRow {
        name: "cloud_agent.run_agent_run",
        owner: Owner::Python,
    },
    TaskRow {
        name: "cloud_agent.scan_queued_runs",
        owner: Owner::Python,
    },
    TaskRow {
        name: "cloud_agent.sweep_stale_runs",
        owner: Owner::Python,
    },
    TaskRow {
        name: "managed_runner.expire_waiting_runs",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.agent_ticker.fire_tick",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.agent_ticker.scan_due_tickers",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.analytic_plot_export.analytic_export_task",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.cleanup_task.delete_api_logs",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.cleanup_task.delete_email_notification_logs",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.cleanup_task.delete_issue_description_versions",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.cleanup_task.delete_page_versions",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.cleanup_task.delete_webhook_logs",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.deletion_task.hard_delete",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.deletion_task.soft_delete_related_objects",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.dummy_data_task.create_dummy_data",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.email_notification_task.send_email_notification",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.email_notification_task.stack_email_notification",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.event_tracking_task.track_event",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.export_task.issue_export_task",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.file_asset_task.delete_unuploaded_file_asset",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.forgot_password_task.forgot_password",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.git_sync_task.post_completion_comment",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.git_sync_task.sync_all_bindings",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.git_sync_task.sync_one_binding",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.github_sync_task.post_completion_comment",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.github_sync_task.sync_all_repos",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.github_sync_task.sync_one_repo",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_activities_task.issue_activity",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_description_version_sync.schedule_issue_description_version",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_description_version_sync.sync_issue_description_version",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_description_version_task.issue_description_version_task",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_version_sync.issue_task",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_version_sync.schedule_issue_version",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.issue_version_sync.sync_issue_version",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.logger_task.process_logs",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.loop.fire_loop_target",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.loop.scan_due_targets",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.magic_link_code_task.magic_link",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.notification_task.notifications",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.page_transaction_task.page_transaction",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.page_version_task.track_page_version",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.project_add_user_email_task.project_add_user_email",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.project_invitation_task.project_invitation",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.recent_visited_task.recent_visited_task",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.scheduler.fire_scheduler_binding",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.scheduler.scan_due_bindings",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.storage_metadata_task.get_asset_object_metadata",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.bgtasks.user_activation_email_task.user_activation_email",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.webhook_task.model_activity",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.webhook_task.webhook_activity",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.webhook_task.webhook_send_task",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.workspace_invitation_task.workspace_invitation",
        owner: Owner::Python,
    },
    TaskRow {
        name: "pi_dash.bgtasks.workspace_seed_task.workspace_seed",
        owner: Owner::Rust,
    },
    TaskRow {
        name: "pi_dash.license.bgtasks.tracer.instance_traces",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.apply_agent_run_terminal_effects",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.expire_stale_approvals",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.mark_offline_runners",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.reconcile_agent_run_terminal_effects",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.reconcile_stalled_runs",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_agent_chat_state",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_chat_message_dedupe",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_idle_sessions",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_old_streams",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_run_message_dedupe",
        owner: Owner::Python,
    },
    TaskRow {
        name: "runner.sweep_stale_runners",
        owner: Owner::Python,
    },
];

/// The `routes --json` document.
pub fn routes_json() -> serde_json::Value {
    let mut groups = serde_json::Map::new();
    for (name, prefix) in GROUPS {
        let rows: Vec<serde_json::Value> = ROUTES
            .iter()
            .filter(|row| row.group == *name)
            .map(|row| {
                serde_json::json!({
                    "path": row.path,
                    "owned": row.owned,
                    "proxied": row.proxied,
                })
            })
            .collect();
        groups.insert(
            (*name).to_string(),
            serde_json::json!({"prefix": prefix, "routes": rows}),
        );
    }
    serde_json::json!({
        "meta": {
            "route_count": ROUTES.len(),
            "groups": GROUPS.len(),
        },
        "groups": groups,
    })
}

/// One-line human summary of the route inventory (bare `routes`).
pub fn routes_text() -> String {
    let mut out = format!("{} routes in {} groups\n", ROUTES.len(), GROUPS.len());
    for (name, prefix) in GROUPS {
        let count = ROUTES.iter().filter(|row| row.group == *name).count();
        out.push_str(&format!("{name} ({prefix}): {count}\n"));
    }
    out
}

/// Serialize one scheduler entry; crontab fields become match sets so
/// the checker compares semantics, not spellings (`*/1` vs `*`).
fn schedule_json(entry: &BeatEntry) -> serde_json::Value {
    let cadence = match &entry.cadence {
        Cadence::IntervalSecs(secs) => {
            serde_json::json!({"type": "interval_secs", "secs": secs})
        }
        Cadence::Crontab(cron) => {
            let field = |f: &pidash_jobs::schedule::CronField, min: u32, max: u32| {
                (min..=max).filter(|v| f.matches(*v)).collect::<Vec<_>>()
            };
            serde_json::json!({
                "type": "crontab",
                "minute": field(&cron.minute, 0, 59),
                "hour": field(&cron.hour, 0, 23),
                "day_of_month": field(&cron.day_of_month, 1, 31),
                "month_of_year": field(&cron.month_of_year, 1, 12),
                "day_of_week": field(&cron.day_of_week, 0, 6),
            })
        }
    };
    serde_json::json!({
        "name": entry.name,
        "task": entry.task,
        "cadence": cadence,
    })
}

/// The `jobs --json` document: the claimed registry plus the live
/// scheduler table (env overrides applied, values recorded).
pub fn jobs_json() -> serde_json::Value {
    let schedule = default_schedule();
    let rust_owned = TASKS.iter().filter(|row| row.owner == Owner::Rust).count();
    serde_json::json!({
        "meta": {
            "task_count": TASKS.len(),
            "rust_owned": rust_owned,
            "schedule_entries": schedule.len(),
        },
        "tasks": TASKS.iter().map(|row| serde_json::json!({
            "name": row.name,
            "owner": row.owner.as_str(),
        })).collect::<Vec<_>>(),
        "schedule": schedule.iter().map(schedule_json).collect::<Vec<_>>(),
    })
}

/// One-line human summary of the job inventory (bare `jobs`).
pub fn jobs_text() -> String {
    let rust_owned = TASKS.iter().filter(|row| row.owner == Owner::Rust).count();
    format!(
        "{} tasks ({} rust-owned), {} schedule entries\n",
        TASKS.len(),
        rust_owned,
        default_schedule().len(),
    )
}

/// Substitute every `{param}` / `{*param}` capture in `path` with `value`.
#[cfg(test)]
fn probe_path(path: &str, value: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        match rest[open..].find('}') {
            Some(end) => {
                out.push_str(value);
                rest = &rest[open + end + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Probe values, one per Django converter family: `x` (slug/str/path),
/// `1` (int), a lowercase UUID (uuid). Handlers proxy shapes Django's
/// converters would reject, so ownership needs a valid-shape probe.
#[cfg(test)]
const PROBE_VALUES: &[&str] = &["x", "1", "8dbd5acc-b3ce-4259-91b2-570768a0b918"];

/// Marker status the stub upstream answers with: proxied requests come
/// back 599, owned handlers never do. Status-only probing stays exact
/// (no body sniffing, so HEAD and streams are safe).
#[cfg(test)]
const PROXY_MARKER: u16 = 599;

/// The pin-test app: every flag on (Rust serves all it owns), a stub
/// upstream answering [`PROXY_MARKER`] (proxied requests are exactly
/// the 599s), no pools (DB-backed handlers answer 500, never 599).
#[cfg(test)]
async fn pin_app() -> axum::Router {
    let stub = axum::Router::new().fallback(|| async {
        (
            axum::http::StatusCode::from_u16(PROXY_MARKER).expect("marker status"),
            "parity-stub",
        )
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("stub bind");
    let addr = listener.local_addr().expect("stub addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, stub.into_make_service())
            .with_graceful_shutdown(std::future::pending::<()>())
            .await;
    });
    let mut flags = EdgeFlags::all_off();
    for prefix in Prefix::ALL {
        flags.set(prefix, true);
    }
    let edge = EdgeHandle::new(format!("http://{addr}"), flags).expect("test edge");
    build_router_with_overlay(AppState::with_edge("parity-test", edge), Overlay::new())
}

/// Probe one method+path; `Ok(status)` or `Err` on the hang guard.
#[cfg(test)]
async fn probe_status(
    app: &axum::Router,
    method: &str,
    path: &str,
) -> Result<axum::http::StatusCode, tokio::time::error::Elapsed> {
    use tower::ServiceExt;
    let request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .body(axum::body::Body::empty())
        .expect("probe request");
    let fut = app.clone().oneshot(request);
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), fut)
        .await?
        .expect("router error is infallible");
    Ok(response.status())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structural sanity: sorted, unique, well-formed rows over known
    /// groups and tracked methods, owned/proxied disjoint.
    #[test]
    fn inventory_rows_are_well_formed() {
        let groups: Vec<&str> = GROUPS.iter().map(|(name, _)| *name).collect();
        let mut seen = std::collections::HashSet::new();
        let mut last_key = (0usize, String::new());
        let mut first = true;
        for row in ROUTES {
            assert!(
                groups.contains(&row.group),
                "{}: unknown group {}",
                row.path,
                row.group
            );
            assert!(row.path.starts_with('/'), "path must be absolute");
            assert!(
                seen.insert((row.group, row.path)),
                "duplicate row {} {}",
                row.group,
                row.path
            );
            let group_index = groups
                .iter()
                .position(|name| *name == row.group)
                .expect("group");
            let key = (group_index, row.path.to_string());
            assert!(
                first || key >= last_key,
                "rows must be grouped and path-sorted ({} {} after {})",
                row.group,
                row.path,
                last_key.1,
            );
            first = false;
            last_key = key;
            for method in row.owned.iter().chain(row.proxied.iter()) {
                assert!(
                    TRACKED_METHODS.contains(method),
                    "{}: untracked method {method}",
                    row.path
                );
            }
            for method in row.owned {
                assert!(
                    !row.proxied.contains(method),
                    "{}: {method} both owned and proxied",
                    row.path
                );
            }
            assert!(
                probe_path(row.path, "x").find('{').is_none(),
                "{}: unclosed capture",
                row.path
            );
        }
        let mut task_seen = std::collections::HashSet::new();
        let mut last_task = "";
        for row in TASKS {
            assert!(task_seen.insert(row.name), "duplicate task {}", row.name);
            assert!(
                row.name >= last_task,
                "tasks must be name-sorted ({} after {last_task})",
                row.name
            );
            last_task = row.name;
        }
    }

    /// The fallback premise: an unregistered path proxies (the stub
    /// marker here).
    #[tokio::test]
    async fn unregistered_paths_proxy() {
        let app = pin_app().await;
        for path in ["/parity-no-such-path/", "/api/parity-no-such-path/"] {
            let status = probe_status(&app, "GET", path)
                .await
                .expect("probe must not hang");
            assert_eq!(status.as_u16(), PROXY_MARKER, "{path}");
        }
    }

    /// Re-probe a 405 for its router fingerprint: axum's 405 carries an
    /// Allow header and an empty body; a handler-made 405 differs and
    /// must be listed as owned (reviewed), never silently unlisted.
    async fn probe_405_details(app: &axum::Router, method: &str, path: &str) -> (bool, usize) {
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .body(axum::body::Body::empty())
            .expect("probe request");
        let fut = app.clone().oneshot(request);
        let Ok(Ok(response)) = tokio::time::timeout(std::time::Duration::from_secs(5), fut).await
        else {
            return (false, usize::MAX);
        };
        let allow = response.headers().contains_key(axum::http::header::ALLOW);
        let len = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .map(|body| body.len())
            .unwrap_or(usize::MAX);
        (allow, len)
    }

    /// The worker registry, rebuilt exactly like `worker()` (mirror it
    /// on any registration change): `PgPool` groups register for real
    /// over a lazy pool (no I/O); `Pools` groups register stubs over
    /// the same name constants the real register fns consume (`Pools`
    /// cannot be built without a database). The table's rust set must
    /// equal the const union, and the registry must own exactly the
    /// table's rust names. Residual: a registration outside these
    /// consts is invisible here (`Registry` has no iterator) — review
    /// catches it, and the parity diff flags the behavior change.
    #[tokio::test]
    async fn mirror_registry_matches_tasks_table() {
        use pidash_jobs::tasks_cleanup;
        use pidash_jobs::worker::{Handler, Registry, Verdict};

        let pool =
            sqlx::PgPool::connect_lazy("postgres://localhost:1/unused").expect("lazy pool builds");
        let mut registry = Registry::new();
        let stub = || {
            let handler: Handler = std::sync::Arc::new(|_| Box::pin(async { Ok(Verdict::Ack) }));
            handler
        };
        // Stub-backed: the exact names the `Pools`-taking registers own
        // (same precedent as `worker_registry_owns_all_d09_local_tasks`).
        for spec in tasks_cleanup::cleanup::TASKS {
            registry.register(spec.task, stub());
        }
        for name in [
            tasks_cleanup::SOFT_DELETE_TASK,
            tasks_cleanup::HARD_DELETE_TASK,
        ] {
            registry.register(name, stub());
        }
        registry.register(pidash_jobs::tasks_webhooks::PROCESS_LOGS_TASK, stub());
        // Real registrations (the same calls `worker()` makes).
        tasks_cleanup::register_versions(&mut registry, pool.clone());
        tasks_cleanup::assets::register_assets(
            &mut registry,
            pool.clone(),
            std::sync::Arc::new(tasks_cleanup::assets::UnavailableObjectStore),
            std::sync::Arc::new(tasks_cleanup::assets::NoopLiveConvert),
        );
        tasks_cleanup::register_workspace_seed(&mut registry, pool.clone(), std::env::temp_dir());
        tasks_cleanup::dummy_data::register(&mut registry, pool.clone());
        pidash_jobs::integrations::git_sync::register_git_sync_tasks(
            &mut registry,
            pool.clone(),
            pidash_jobs::integrations::git_sync::LiveProviders::from_env(),
        );
        pidash_jobs::integrations::github_sync::register_github_sync_tasks(
            &mut registry,
            pool.clone(),
            pidash_jobs::integrations::github_sync::LiveTransports::from_env(),
        );

        // The const union, independent of the table.
        let mut expected_rust = std::collections::BTreeSet::new();
        expected_rust.extend(tasks_cleanup::cleanup::TASKS.iter().map(|spec| spec.task));
        expected_rust.extend(tasks_cleanup::versions::ALL_VERSION_TASKS);
        expected_rust.extend([
            tasks_cleanup::assets::TASK_DELETE_UNUPLOADED,
            tasks_cleanup::assets::TASK_GET_METADATA,
            tasks_cleanup::assets::TASK_COPY_S3_OBJECTS,
            tasks_cleanup::SOFT_DELETE_TASK,
            tasks_cleanup::HARD_DELETE_TASK,
            tasks_cleanup::WORKSPACE_SEED_TASK_NAME,
            tasks_cleanup::dummy_data::TASK_NAME,
            pidash_jobs::tasks_webhooks::PROCESS_LOGS_TASK,
        ]);
        expected_rust.extend(pidash_jobs::integrations::git_sync::TASK_NAMES);
        expected_rust.extend(pidash_jobs::integrations::github_sync::TASK_NAMES);
        // Count pins: a const that grows must update the table too.
        assert_eq!(
            tasks_cleanup::cleanup::TASKS.len(),
            5,
            "cleanup group drifted"
        );
        assert_eq!(
            tasks_cleanup::versions::ALL_VERSION_TASKS.len(),
            7,
            "versions group drifted"
        );
        assert_eq!(
            pidash_jobs::integrations::git_sync::TASK_NAMES.len(),
            3,
            "git_sync group drifted"
        );
        assert_eq!(
            pidash_jobs::integrations::github_sync::TASK_NAMES.len(),
            3,
            "github_sync group drifted"
        );
        assert_eq!(expected_rust.len(), 26, "worker-owned count drifted");

        let table_rust: std::collections::BTreeSet<&str> = TASKS
            .iter()
            .filter(|row| row.owner == Owner::Rust)
            .map(|row| row.name)
            .collect();
        assert_eq!(table_rust, expected_rust, "table rust set != const union");

        for row in TASKS {
            assert_eq!(
                registry.owns(row.name),
                row.owner == Owner::Rust,
                "registry ownership of {}",
                row.name
            );
        }
        // Deliberately unregistered: restore (BUG-DEL-2) is not a Django
        // task at all, so it is not in the table either.
        assert!(!registry.owns(tasks_cleanup::RESTORE_TASK_NAME));
    }

    /// The schedule serializer: spot-check set shapes (full content is
    /// diffed against Django by the parity checker).
    #[test]
    fn schedule_sets_serialize() {
        let doc = jobs_json();
        let schedule = doc
            .get("schedule")
            .and_then(|v| v.as_array())
            .expect("schedule");
        assert_eq!(schedule.len(), 26);
        let find = |name: &str| {
            schedule
                .iter()
                .find(|entry| entry.get("name").and_then(|v| v.as_str()) == Some(name))
                .unwrap_or_else(|| panic!("{name}"))
        };
        let every_minute = find("scan-due-agent-tickers");
        assert_eq!(
            every_minute.get("task").and_then(|v| v.as_str()).unwrap(),
            "pi_dash.bgtasks.agent_ticker.scan_due_tickers"
        );
        let cadence = every_minute.get("cadence").unwrap();
        assert_eq!(
            cadence.get("type").and_then(|v| v.as_str()).unwrap(),
            "crontab"
        );
        assert_eq!(
            cadence
                .get("minute")
                .and_then(|v| v.as_array())
                .unwrap()
                .len(),
            60
        );
        let daily = find("check-every-day-to-delete-hard-delete");
        let cadence = daily.get("cadence").unwrap();
        assert_eq!(
            cadence.get("minute").and_then(|v| v.as_array()).unwrap(),
            &vec![serde_json::Value::from(0)]
        );
        assert_eq!(
            cadence.get("hour").and_then(|v| v.as_array()).unwrap(),
            &vec![serde_json::Value::from(0)]
        );
        let interval = find("runner-reconcile-stalled-runs");
        let cadence = interval.get("cadence").unwrap();
        assert_eq!(
            cadence.get("type").and_then(|v| v.as_str()).unwrap(),
            "interval_secs"
        );
        assert_eq!(cadence.get("secs").and_then(|v| v.as_u64()).unwrap(), 30);
    }

    /// Every inventory row, re-probed against the real router: owned
    /// methods serve (any non-marker status — handlers answer 2xx/3xx/
    /// 4xx/5xx from here), proxied methods return the stub marker, and
    /// unlisted methods 405 from the router (Allow header, empty body).
    /// Parameters probe once per converter family (`x`, `1`, UUID):
    /// handlers proxy shapes Django's converters would reject, so one
    /// serving substitution proves ownership. Failures aggregate: one
    /// run shows every drifted row.
    #[tokio::test]
    async fn pin_routes_match_router() {
        let app = pin_app().await;
        let mut failures = Vec::new();
        for row in ROUTES {
            for method in TRACKED_METHODS {
                let owned = row.owned.contains(method);
                let proxied = row.proxied.contains(method);
                // Owned is existential: handlers proxy shapes Django's
                // converters would reject, so one serving substitution
                // proves ownership (a hang also proves a handler ran).
                let mut owned_all_marker = owned;
                for value in PROBE_VALUES {
                    let path = probe_path(row.path, value);
                    let status = match probe_status(&app, method, &path).await {
                        Ok(status) => status,
                        Err(_) if owned => {
                            eprintln!("note: {method} {path} hung (owned)");
                            owned_all_marker = false;
                            continue;
                        }
                        Err(_) => {
                            failures.push(format!(
                                "{method} {} [{}]: hung (expected {})",
                                row.path,
                                value,
                                if proxied { "proxied" } else { "router 405" },
                            ));
                            continue;
                        }
                    };
                    let code = status.as_u16();
                    if owned {
                        if code != PROXY_MARKER {
                            owned_all_marker = false;
                        }
                    } else if proxied {
                        if code != PROXY_MARKER {
                            failures.push(format!(
                                "{method} {} [{value}]: status {code}, table says proxied",
                                row.path
                            ));
                        }
                    } else if code != 405 {
                        failures.push(format!(
                            "{method} {} [{value}]: status {code}, table lists neither (expected router 405)",
                            row.path
                        ));
                    } else {
                        let (allow, len) = probe_405_details(&app, method, &path).await;
                        if !allow || len != 0 {
                            failures.push(format!(
                                "{method} {} [{value}]: 405 is handler-made (allow={allow} len={len}), list it as owned",
                                row.path
                            ));
                        }
                    }
                }
                if owned && owned_all_marker {
                    failures.push(format!(
                        "{method} {}: proxied every substitution, table says owned",
                        row.path
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} drifted probes:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
