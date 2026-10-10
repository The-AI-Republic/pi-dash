# Expected parity differences (PIDASHCONV-820)

Every row below is one live difference between Django and Rust that has
been reviewed and classified. The CI job
(`.github/workflows/rust-api-parity.yml`) runs

```sh
python rust-api/contract-tests/parity/route_inventory.py check --binary <pidash-api>
python rust-api/contract-tests/parity/job_inventory.py check --binary <pidash-api>
```

and fails when the live diff contains anything not listed here, or when
a listed row no longer appears live (a fix landed: delete its lines).
The ```` ```expected-routes ```` / ```` ```expected-jobs ```` fenced
blocks are machine-read (one `gap :: subject :: detail` signature per
line, sorted); the prose around them is for reviewers. Never add a
`G0`/`W0` line: unclassified rows must be resolved by hand, given a gap
id, and filed as a fix issue first.

Counts (2026-10-10, `pi_dash.settings.test`):

- Routes: 528 Django rows — 261 OWNED, 267 MISSING (all listed below),
  5 Rust-only (all justified, in `RUST_ONLY_JUSTIFICATIONS`).
- Jobs: 85 tasks — 26 OWNED, 34 proxied by design, 25 MISSING;
  beat 26/26 OWNED.

## Routes

### G1 — axum auto-HEAD serves what Django 405s (247 rows)

Fix: PIDASHCONV-826. Axum auto-serves HEAD wherever GET is routed;
Django has no HEAD arm, so HEAD 405s there. Every row below is a GET
route whose only gap is `HEAD:serves-but-django-405`. The two further
HEAD gaps that lived on the magic-generate routes (formerly shared
with G4 under G1+G4) were resolved by PIDASHCONV-829: HEAD now proxies
to Django through axum's `get` handling, like the other arms.

```expected-routes
G1 :: /api/assets/v2/static/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/check/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/download/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/assets/v2/workspaces/{slug}/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/instances/ :: HEAD:serves-but-django-405
G1 :: /api/instances/configurations/ :: HEAD:serves-but-django-405
G1 :: /api/instances/loop/jobs/ :: HEAD:serves-but-django-405
G1 :: /api/instances/loop/jobs/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/instances/loop/jobs/{pk}/targets/ :: HEAD:serves-but-django-405
G1 :: /api/instances/workspace-slug-check/ :: HEAD:serves-but-django-405
G1 :: /api/instances/workspaces/ :: HEAD:serves-but-django-405
G1 :: /api/integrations/github/app/callback/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/comments/{comment_id}/reactions/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/cycles/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/intakes/{intake_id}/inbox-issues/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/{issue_id}/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/{issue_id}/comments/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/{issue_id}/reactions/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/issues/{issue_id}/votes/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/labels/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/members/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/meta/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/modules/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/settings/ :: HEAD:serves-but-django-405
G1 :: /api/public/anchor/{anchor}/states/ :: HEAD:serves-but-django-405
G1 :: /api/public/assets/v2/anchor/{anchor}/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/public/workspaces/{slug}/project-boards/ :: HEAD:serves-but-django-405
G1 :: /api/public/workspaces/{slug}/projects/{project_id}/anchor/ :: HEAD:serves-but-django-405
G1 :: /api/runners/chat/approvals/ :: HEAD:serves-but-django-405
G1 :: /api/runners/chat/sessions/ :: HEAD:serves-but-django-405
G1 :: /api/runners/chat/sessions/{session_id}/ :: HEAD:serves-but-django-405
G1 :: /api/runners/chat/sessions/{session_id}/messages/ :: HEAD:serves-but-django-405
G1 :: /api/runners/projects/ :: HEAD:serves-but-django-405
G1 :: /api/timezones/ :: HEAD:serves-but-django-405
G1 :: /api/unsplash/ :: HEAD:serves-but-django-405
G1 :: /api/users/api-tokens/ :: HEAD:serves-but-django-405
G1 :: /api/users/api-tokens/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/users/file-assets/{asset_key}/ :: HEAD:serves-but-django-405
G1 :: /api/users/last-visited-workspace/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/accounts/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/accounts/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/activities/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/ai-assistant/agent-profile/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/ai-assistant/mcp-servers/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/auto-pm/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/instance-admin/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/integrations/github/app/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/notification-preferences/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/profile/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/settings/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/invitations/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/join-requests/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/{slug}/activity-graph/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/{slug}/dashboard/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/{slug}/issues-completed-graph/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/{slug}/project-roles/ :: HEAD:serves-but-django-405
G1 :: /api/users/me/workspaces/{slug}/projects/invitations/ :: HEAD:serves-but-django-405
G1 :: /api/users/session/ :: HEAD:serves-but-django-405
G1 :: /api/v1/auth/workspaces/ :: HEAD:serves-but-django-405
G1 :: /api/v1/runner/health/ :: HEAD:serves-but-django-405
G1 :: /api/v1/runner/projects/ :: HEAD:serves-but-django-405
G1 :: /api/v1/users/me/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/assets/{asset_id}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/invitations/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/invitations/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/members/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/members/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/project-members/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/states/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/stickies/ :: HEAD:serves-but-django-405
G1 :: /api/v1/workspaces/{slug}/stickies/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/file-assets/{workspace_id}/{asset_key}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/advance-analytics-charts/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/advance-analytics-stats/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/advance-analytics/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/ai-assistant/threads/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/ai-assistant/threads/{thread_id}/messages/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/analytic-view/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/analytic-view/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/analytics/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/cycles/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/default-analytics/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/draft-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/draft-issues/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/entity-search/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/estimates/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/export-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/home-preferences/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/home-preferences/{key}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/git/accounts/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/git/accounts/{account_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/git/accounts/{account_id}/repos/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/git/providers/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/github/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/integrations/github/repos/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/invitations/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/invitations/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/invitations/{pk}/join/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/join-requests/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/labels/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/members/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/members/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/modules/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/project-identifiers/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/project-members/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/project-stats/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/details/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/advance-analytics-charts/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/advance-analytics-stats/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/advance-analytics/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/archived-cycles/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/archived-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/archived-modules/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/analytics/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/user-properties/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/deleted-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/estimates/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/github/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/inbox-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/inboxes/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intake-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intake-state/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intakes/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/invitations/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issue-labels/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues-detail/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/list/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/join/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/members/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/members/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/user-properties/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/modules/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages-summary/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/project-estimates/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/project-members/me/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/repository/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/occurrences/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{binding_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/search-issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/states/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/states/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/user-favorite-views/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/user-properties/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/v2/issues/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/views/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/projects/{project_id}/views/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/prompt-sections :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/prompts/{kind}/compiled :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/quick-links/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/quick-links/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/recent-visits/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/saved-analytic-view/{analytic_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/schedulers/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/schedulers/{scheduler_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/search/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/sidebar-preferences/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/states/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/stickies/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/stickies/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-activity/{user_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-favorite-projects/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-favorites/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-favorites/{favorite_id}/group/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-issues/{user_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-profile/{user_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-properties/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/user-stats/{user_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/users/notifications/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/users/notifications/unread/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/users/notifications/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/views/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/views/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/webhook-logs/{webhook_id}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/webhooks/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/webhooks/{pk}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/ :: HEAD:serves-but-django-405
G1 :: /api/workspaces/{slug}/workspace-members/me/ :: HEAD:serves-but-django-405
G1 :: /auth/get-csrf-token/ :: HEAD:serves-but-django-405
G1 :: /auth/gitea/ :: HEAD:serves-but-django-405
G1 :: /auth/gitea/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/github/ :: HEAD:serves-but-django-405
G1 :: /auth/github/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/gitlab/ :: HEAD:serves-but-django-405
G1 :: /auth/gitlab/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/google/ :: HEAD:serves-but-django-405
G1 :: /auth/google/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/gitea/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/gitea/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/github/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/github/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/gitlab/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/gitlab/callback/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/google/ :: HEAD:serves-but-django-405
G1 :: /auth/spaces/google/callback/ :: HEAD:serves-but-django-405
```

### G1+G4 — magic-generate routes (resolved by PIDASHCONV-829)

Both rows are gone: the proxy arms (including HEAD via axum's `get`
handling) make the generate routes match Django on every method, so
PIDASHCONV-826 has nothing left to do here.

### G2 — license instance-admin session/auth endpoints unported (7 rows)

Fix: PIDASHCONV-827. No Rust route at all.

```expected-routes
G2 :: /api/instances/admins/ :: no-rust-route
G2 :: /api/instances/admins/me/ :: no-rust-route
G2 :: /api/instances/admins/session/ :: no-rust-route
G2 :: /api/instances/admins/sign-in/ :: no-rust-route
G2 :: /api/instances/admins/sign-out/ :: no-rust-route
G2 :: /api/instances/admins/sign-up/ :: no-rust-route
G2 :: /api/instances/admins/{pk}/ :: no-rust-route
```

### G3 — DRF router artifacts: api-root index + format suffixes (4 rows)

Fix: PIDASHCONV-828. The `/api/v1/workspaces/{slug}/` api-root index
and the `.{format}` suffix variants Django's router generates; no Rust
route.

```expected-routes
G3 :: /api/v1/workspaces/{slug}/ :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/invitations.{format}/? :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/stickies.{format}/? :: no-rust-route
G3 :: /api/v1/workspaces/{slug}/{format} :: no-rust-route
```

### G4 — magic-link sign-in/up routes missing proxy arms (resolved by PIDASHCONV-829)

All four rows are gone: the proxy arms make the sign-in/up routes
match Django on every method (OPTIONS 200 `Allow`, `Allow`-bearing
405s). The two generate routes were under G1+G4 above.

### G5 — DRF format suffixes on detail routes (2 rows)

Fix: PIDASHCONV-828 (with G3). The `{pk}.{format}` detail variants; no
Rust route.

```expected-routes
G5 :: /api/v1/workspaces/{slug}/invitations/{pk}.{format}/? :: no-rust-route
G5 :: /api/v1/workspaces/{slug}/stickies/{pk}.{format}/? :: no-rust-route
```

### G6 — handler-denial 405s pending contract proof (7 rows)

Fix: PIDASHCONV-830. Explicit `*_not_allowed` handlers replay the
view's auth prelude and answer DRF's 405 bytes; read-verified, but no
contract test pins the bytes yet, so the diff conservatively reports
`serves-but-django-405`.

```expected-routes
G6 :: /api/users/me/workspaces/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspace-slug-check/ :: POST:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/ :: POST:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/user-activity/{user_id}/export/ :: GET:serves-but-django-405;PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/ :: PUT:serves-but-django-405;PATCH:serves-but-django-405;DELETE:serves-but-django-405;HEAD:serves-but-django-405
G6 :: /api/workspaces/{slug}/workspace-themes/{pk}/ :: POST:serves-but-django-405;PUT:serves-but-django-405;HEAD:serves-but-django-405
```

### Rust-only routes (5, all justified in-script)

New Rust-only surface fails the check until a justification is added to
`RUST_ONLY_JUSTIFICATIONS` in `route_inventory.py` (reviewed). Current:

- `/healthz` — foundation liveness probe; Django answers its 404 page
  here by design.
- `/api/schema`, `/api/schema/`, `/api/schema/redoc/`,
  `/api/schema/swagger-ui/` — served when the v1-openapi flags are on;
  Django only with `ENABLE_DRF_SPECTACULAR=1` (off in contract envs).
- `/api/v1/workspaces/{slug}/issues/{segment}/`,
  `/api/v1/workspaces/{slug}/work-items/{segment}/`,
  `/api/workspaces/{slug}/work-items/{tail}/` — single-param capture of
  Django's compound `{project_identifier}-{issue_identifier}` (the
  `ALIASES` join; dashless segments proxy so Django answers its own
  404 — behaviorally exact, hence not rust-only).

## Jobs

Task dispositions: OWNED (Rust runs it), PROXIED (Rust forwards to the
Python worker and Django's outcome is preserved — each row says why),
MISSING (live registered task Rust forwards that would be lost at
switchover — one fix issue per register fn).

Note: 6 OWNED tasks are Rust-superset executions — called in Django
but unregistered there (dropped; PDASHOSS01-292), while the Rust
worker runs them: `copy_s3_objects_of_description_and_assets`,
`issue_description_version_task`, `process_logs`, `track_page_version`,
`get_asset_object_metadata`, `workspace_seed`. Deliberate (accepted by
their domain gates); recorded here and in the V-01 PR, for the V-05
ported-bug register to catalog.

### W-assistant (2 rows)

Fix: PIDASHCONV-831.

```expected-jobs
W-assistant :: assistant.run_turn :: must-wire (lost without python worker)
W-assistant :: assistant.sweep_stale_turns :: must-wire (lost without python worker)
```

### W-cloud-agent (3 rows)

Fix: PIDASHCONV-832.

```expected-jobs
W-cloud-agent :: cloud_agent.run_agent_run :: must-wire (lost without python worker)
W-cloud-agent :: cloud_agent.scan_queued_runs :: must-wire (lost without python worker)
W-cloud-agent :: cloud_agent.sweep_stale_runs :: must-wire (lost without python worker)
```

### W-ticker + W-scheduler (4 rows)

Fix: PIDASHCONV-833 (one issue: the ticker and scheduler scan/fire
register fns are wired together).

```expected-jobs
W-ticker :: pi_dash.bgtasks.agent_ticker.fire_tick :: must-wire (lost without python worker)
W-ticker :: pi_dash.bgtasks.agent_ticker.scan_due_tickers :: must-wire (lost without python worker)
W-scheduler :: pi_dash.bgtasks.scheduler.fire_scheduler_binding :: must-wire (lost without python worker)
W-scheduler :: pi_dash.bgtasks.scheduler.scan_due_bindings :: must-wire (lost without python worker)
```

### W-mail (3 rows)

Fix: PIDASHCONV-834.

```expected-jobs
W-mail :: pi_dash.bgtasks.email_notification_task.send_email_notification :: must-wire (lost without python worker)
W-mail :: pi_dash.bgtasks.email_notification_task.stack_email_notification :: must-wire (lost without python worker)
W-mail :: pi_dash.bgtasks.notification_task.notifications :: must-wire (lost without python worker)
```

### W-export (1 row)

Fix: PIDASHCONV-835.

```expected-jobs
W-export :: pi_dash.bgtasks.exporter_expired_task.delete_old_s3_link :: must-wire (lost without python worker)
```

### W-activity (1 row)

Fix: PIDASHCONV-836.

```expected-jobs
W-activity :: pi_dash.bgtasks.issue_activities_task.issue_activity :: must-wire (lost without python worker)
```

### W-automation (1 row)

Fix: PIDASHCONV-837.

```expected-jobs
W-automation :: pi_dash.bgtasks.issue_automation_task.archive_and_close_old_issues :: must-wire (lost without python worker)
```

### W-loop (2 rows)

Fix: PIDASHCONV-838.

```expected-jobs
W-loop :: pi_dash.bgtasks.loop.fire_loop_target :: must-wire (lost without python worker)
W-loop :: pi_dash.bgtasks.loop.scan_due_targets :: must-wire (lost without python worker)
```

### W-license (1 row)

Fix: PIDASHCONV-839.

```expected-jobs
W-license :: pi_dash.license.bgtasks.tracer.instance_traces :: must-wire (lost without python worker)
```

### W-runner (7 rows)

Fix: PIDASHCONV-840.

```expected-jobs
W-runner :: runner.apply_agent_run_terminal_effects :: must-wire (lost without python worker)
W-runner :: runner.expire_stale_approvals :: must-wire (lost without python worker)
W-runner :: runner.mark_offline_runners :: must-wire (lost without python worker)
W-runner :: runner.reconcile_agent_run_terminal_effects :: must-wire (lost without python worker)
W-runner :: runner.reconcile_stalled_runs :: must-wire (lost without python worker)
W-runner :: runner.sweep_agent_chat_state :: must-wire (lost without python worker)
W-runner :: runner.sweep_chat_message_dedupe :: must-wire (lost without python worker)
```

### Proxied by design (34 rows)

No fix issue: forwarding preserves Django's outcome on every row.

- `celery.*` (9): Celery canvas builtins; no canvas usage in the codebase.
- `django-drops-too` (17): called but unregistered in Django
  (PDASHOSS01-292) — the Python worker drops them; forwarding drops
  them identically. `track_event` is explicitly unregistered in Rust
  because an early-return handler would swallow the forward.
- `dormant` (4): registered but never published; forward preserves
  non-execution.
- `managed_runner.expire_waiting_runs`: cloud-only app; the OSS beat
  entry fires into the void on both sides.
- `project_invitation`: never invoked (`invite.py:105` calls `.delay`
  on a list; PDASHOSS01-292).
- `recent_visited_task`: documented no-op (memory broker, no worker by
  design).
- `export_analytics_to_csv_email`: dead (defined, never referenced).

```expected-jobs
PROXIED :: celery.accumulate :: celery-builtin; no canvas usage
PROXIED :: celery.backend_cleanup :: celery-builtin; no canvas usage
PROXIED :: celery.chain :: celery-builtin; no canvas usage
PROXIED :: celery.chord :: celery-builtin; no canvas usage
PROXIED :: celery.chord_unlock :: celery-builtin; no canvas usage
PROXIED :: celery.chunks :: celery-builtin; no canvas usage
PROXIED :: celery.group :: celery-builtin; no canvas usage
PROXIED :: celery.map :: celery-builtin; no canvas usage
PROXIED :: celery.starmap :: celery-builtin; no canvas usage
PROXIED :: managed_runner.expire_waiting_runs :: cloud-only app; oss beat fires into the void on both sides
PROXIED :: pi_dash.bgtasks.analytic_plot_export.analytic_export_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.analytic_plot_export.export_analytics_to_csv_email :: dead (defined, never referenced)
PROXIED :: pi_dash.bgtasks.event_tracking_task.track_event :: django-drops-too (PDASHOSS01-292); rust explicitly unregistered (early-return would swallow forward)
PROXIED :: pi_dash.bgtasks.export_task.issue_export_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.forgot_password_task.forgot_password :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.magic_link_code_task.magic_link :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.page_transaction_task.page_transaction :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.project_add_user_email_task.project_add_user_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.project_invitation_task.project_invitation :: task never invoked (invite.py:105 calls .delay on a list; PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.recent_visited_task.recent_visited_task :: documented no-op (memory broker, no worker by design)
PROXIED :: pi_dash.bgtasks.user_activation_email_task.user_activation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_deactivation_email_task.user_deactivation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_email_update_task.send_email_update_confirmation :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.user_email_update_task.send_email_update_magic_code :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.model_activity :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.send_webhook_deactivation_email :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.webhook_activity :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.webhook_task.webhook_send_task :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.work_item_link_task.crawl_work_item_link_title :: django-drops-too (PDASHOSS01-292)
PROXIED :: pi_dash.bgtasks.workspace_invitation_task.workspace_invitation :: django-drops-too (PDASHOSS01-292)
PROXIED :: runner.sweep_idle_sessions :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_old_streams :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_run_message_dedupe :: dormant (never published; forward preserves non-execution)
PROXIED :: runner.sweep_stale_runners :: dormant (never published; forward preserves non-execution)
```

## Beat

26/26 entries OWNED: same task, same cadence (the four
settings-backed intervals match Django's `pi_dash.settings.test`
values: 10/30/300/30s). Any beat drift fails the check structurally —
there is no "expected beat miss" list.
