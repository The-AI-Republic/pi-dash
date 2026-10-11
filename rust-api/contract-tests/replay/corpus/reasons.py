# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Reasons for corpus-uncovered write pairs (PIDASHCONV-823).

Keys are ``(route shape, METHOD)`` with ``{}`` params (the
``shape_key`` form from ``parity/route_inventory.py``). ``coverage.py
check`` fails when a write pair is neither covered by a session nor
listed here, and fails when a listed pair becomes covered or vanishes
(stale reason — delete the entry).

Buckets:

- ``MISSING_FIX``: the pair has no Rust route/method (parity MISSING);
  the reason names the fix issue. The check verifies the pair is
  actually missing live, so a landed fix fails loudly here until the
  pair is covered or moved.
- ``EXCLUDED``: the pair is owned/proxied but not replayable (live
  third-party provider, single-use flow, ...), each with its evidence.
- ``OWNED_UNCOVERED``: owned pairs with no corpus session yet. Entries
  are deleted as corpus batches land; the batch plan lives in the
  header comment below.

Proxied pairs need no entry: the check verifies them live against
``routes --json`` and reports the uniform proxied-by-design reason.
``__debug__`` pairs likewise (dev-only toolbar).
"""

# Batch plan (batch 1 landed with the runner; later batches delete rows):
# - batch 2: v1_work_items, app_project, app_cycles, app_modules
# - batch 3: auth_session, auth_device, auth_magic, space, license
# - batch 4: assistant, loop, prompting, runner, runner_web
# - batch 5: remaining app_* domains + v1_projects + v1_cycles_modules

EXCLUDED = {}

MISSING_FIX = {
    ('/api/instances/admins/', 'DELETE'): "MISSING parity gap G2 (no Rust route); fix PIDASHCONV-827 (Rust license fix: instance-admin session/auth endpoints)",
    ('/api/instances/admins/{}/', 'POST'): "MISSING parity gap G2 (no Rust route); fix PIDASHCONV-827 (Rust license fix: instance-admin session/auth endpoints)",
}

OWNED_UNCOVERED = {
    ('/api/assets/v2/user-assets/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/user-assets/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/user-assets/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/duplicate-assets/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/projects/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.attachment.view
    ('/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.attachment.view
    ('/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.attachment.view
    ('/api/assets/v2/workspaces/{}/projects/{}/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/projects/{}/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/projects/{}/{}/bulk/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/restore/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/assets/v2/workspaces/{}/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.v2.view
    ('/api/integrations/github/app/webhook/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/users/api-tokens/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.api.view
    ('/api/users/api-tokens/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.api.view
    ('/api/users/api-tokens/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.api.view
    ('/api/users/file-assets/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/users/file-assets/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/users/me/', 'DELETE'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.UserEndpoint
    ('/api/users/me/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.UserEndpoint
    ('/api/users/me/accounts/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.view
    ('/api/users/me/email/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.UserEndpoint
    ('/api/users/me/email/generate-code/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.UserEndpoint
    ('/api/users/me/integrations/github/app/install/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/users/me/integrations/github/app/refresh/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/users/me/notification-preferences/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.view
    ('/api/users/me/onboard/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.view
    ('/api/users/me/profile/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.view
    ('/api/users/me/tour-completed/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.user.base.view
    ('/api/users/me/workspaces/invitations/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.invite.UserWorkspaceInvitationsViewSet
    ('/api/users/me/workspaces/join-requests/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.join_request.UserWorkspaceJoinRequestViewSet
    ('/api/v1/assets/user-assets/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/assets/user-assets/server/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/assets/user-assets/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/assets/user-assets/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/assets/user-assets/{}/server/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/assets/user-assets/{}/server/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/runners/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.runner.view
    ('/api/v1/workspaces/{}/agent-runs/{}/yield/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/assets/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/workspaces/{}/assets/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.asset.view
    ('/api/v1/workspaces/{}/invitations/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.invite.WorkspaceInvitationsViewset
    ('/api/v1/workspaces/{}/invitations/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.invite.WorkspaceInvitationsViewset
    ('/api/v1/workspaces/{}/invitations/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.invite.WorkspaceInvitationsViewset
    ('/api/v1/workspaces/{}/projects/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.project.view
    ('/api/v1/workspaces/{}/projects/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.project.view
    ('/api/v1/workspaces/{}/projects/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.project.view
    ('/api/v1/workspaces/{}/projects/{}/archive/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.project.view
    ('/api/v1/workspaces/{}/projects/{}/archive/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.project.view
    ('/api/v1/workspaces/{}/projects/{}/archived-cycles/{}/unarchive/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/archived-modules/{}/unarchive/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/archive/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/cycle-issues/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/cycle-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/cycles/{}/transfer-issues/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.cycle.view
    ('/api/v1/workspaces/{}/projects/{}/intake-issues/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.intake.view
    ('/api/v1/workspaces/{}/projects/{}/intake-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.intake.view
    ('/api/v1/workspaces/{}/projects/{}/intake-issues/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.intake.view
    ('/api/v1/workspaces/{}/projects/{}/issues/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/comments/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/comments/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/comments/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/issue-attachments/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/issue-attachments/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/issue-attachments/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/links/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/links/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/issues/{}/links/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/members/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/members/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/members/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/modules/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/modules/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/modules/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/modules/{}/archive/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/modules/{}/module-issues/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/modules/{}/module-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.module.view
    ('/api/v1/workspaces/{}/projects/{}/pages/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.page.view
    ('/api/v1/workspaces/{}/projects/{}/pages/{}/archive/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.page.view
    ('/api/v1/workspaces/{}/projects/{}/pages/{}/archive/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.page.view
    ('/api/v1/workspaces/{}/projects/{}/project-members/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/project-members/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/project-members/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.member.view
    ('/api/v1/workspaces/{}/projects/{}/states/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.state.view
    ('/api/v1/workspaces/{}/projects/{}/states/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.state.view
    ('/api/v1/workspaces/{}/projects/{}/states/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.state.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/attachments/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/attachments/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/attachments/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/code-reviews/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.git_code_review.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/code-reviews/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.git_code_review.view
    ('/api/v1/workspaces/{}/projects/{}/work-items/{}/relations/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.issue.view
    ('/api/v1/workspaces/{}/stickies/', 'POST'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.sticky.StickyViewSet
    ('/api/v1/workspaces/{}/stickies/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.sticky.StickyViewSet
    ('/api/v1/workspaces/{}/stickies/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (v1_work_items/v1_projects/v1_cycles_modules/v1_cli_auth sessions)",  # api_v1 :: pi_dash.api.views.sticky.StickyViewSet
    ('/api/workspaces/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkSpaceViewSet
    ('/api/workspaces/file-assets/{}/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/workspaces/file-assets/{}/{}/restore/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.FileAssetViewSet
    ('/api/workspaces/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkSpaceViewSet
    ('/api/workspaces/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkSpaceViewSet
    ('/api/workspaces/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkSpaceViewSet
    ('/api/workspaces/{}/ai-assistant/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.external.base.view
    ('/api/workspaces/{}/analytic-view/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/analytic-view/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/analytic-view/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/draft-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.draft.WorkspaceDraftIssueViewSet
    ('/api/workspaces/{}/draft-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.draft.WorkspaceDraftIssueViewSet
    ('/api/workspaces/{}/draft-issues/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.draft.WorkspaceDraftIssueViewSet
    ('/api/workspaces/{}/draft-to-issue/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.draft.WorkspaceDraftIssueViewSet
    ('/api/workspaces/{}/export-analytics/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.view
    ('/api/workspaces/{}/export-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.exporter.base.view
    ('/api/workspaces/{}/file-assets/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/workspaces/{}/home-preferences/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.home.view
    ('/api/workspaces/{}/home-preferences/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.home.view
    ('/api/workspaces/{}/integrations/git/accounts/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/integrations/git/accounts/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/integrations/github/connect/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/integrations/github/disconnect/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/invitations/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.invite.WorkspaceInvitationsViewset
    ('/api/workspaces/{}/invitations/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.invite.WorkspaceInvitationsViewset
    ('/api/workspaces/{}/invitations/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.invite.WorkspaceInvitationsViewset
    ('/api/workspaces/{}/invitations/{}/join/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.invite.view
    ('/api/workspaces/{}/join-requests/{}/approve/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.join_request.WorkspaceJoinRequestViewSet
    ('/api/workspaces/{}/join-requests/{}/deny/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.join_request.WorkspaceJoinRequestViewSet
    ('/api/workspaces/{}/members/leave/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.member.WorkSpaceMemberViewSet
    ('/api/workspaces/{}/members/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.member.WorkSpaceMemberViewSet
    ('/api/workspaces/{}/members/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.member.WorkSpaceMemberViewSet
    ('/api/workspaces/{}/project-identifiers/', 'DELETE'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.view
    ('/api/workspaces/{}/projects/{}/ai-assistant/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.external.base.view
    ('/api/workspaces/{}/projects/{}/archive/', 'DELETE'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.view
    ('/api/workspaces/{}/projects/{}/bulk-archive-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.archive.view
    ('/api/workspaces/{}/projects/{}/bulk-create-labels/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.label.view
    ('/api/workspaces/{}/projects/{}/bulk-delete-issues/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.base.view
    ('/api/workspaces/{}/projects/{}/estimates/{}/estimate-points/', 'POST'): "owned; no corpus session yet — batch 5 (app_project estimate session)",  # app :: pi_dash.app.views.estimate.base.EstimatePointEndpoint
    ('/api/workspaces/{}/projects/{}/estimates/{}/estimate-points/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_project estimate session)",  # app :: pi_dash.app.views.estimate.base.EstimatePointEndpoint
    ('/api/workspaces/{}/projects/{}/estimates/{}/estimate-points/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_project estimate session)",  # app :: pi_dash.app.views.estimate.base.EstimatePointEndpoint
    ('/api/workspaces/{}/projects/{}/favorite-pages/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/favorite-pages/{}/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/github/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/github/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/github/bind/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/inbox-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/inbox-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/inbox-issues/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/inboxes/', 'POST'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/inboxes/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/inboxes/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/intake-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/intake-issues/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/intake-issues/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeIssueViewSet
    ('/api/workspaces/{}/projects/{}/intakes/', 'POST'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/intakes/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/intakes/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_intake session)",  # app :: pi_dash.app.views.intake.base.IntakeViewSet
    ('/api/workspaces/{}/projects/{}/issue-dates/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.base.view
    ('/api/workspaces/{}/projects/{}/issue-labels/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.label.LabelViewSet
    ('/api/workspaces/{}/projects/{}/issue-labels/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.label.LabelViewSet
    ('/api/workspaces/{}/projects/{}/issue-labels/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.label.LabelViewSet
    ('/api/workspaces/{}/projects/{}/issue-labels/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.label.LabelViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/code-reviews/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.git_code_review.GitCodeReviewLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/code-reviews/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.git_code_review.GitCodeReviewLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/comments/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.comment.IssueCommentViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/github-pull-requests/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.github_pr.GithubPullRequestLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/github-pull-requests/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.github_pr.GithubPullRequestLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-attachments/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.attachment.view
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-attachments/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.attachment.view
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-links/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.link.IssueLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-links/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.link.IssueLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-links/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.link.IssueLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-links/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.link.IssueLinkViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-relation/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.relation.IssueRelationViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-subscribers/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.subscriber.IssueSubscriberViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/issue-subscribers/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.subscriber.IssueSubscriberViewSet
    ('/api/workspaces/{}/projects/{}/issues/{}/remove-relation/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.relation.IssueRelationViewSet
    ('/api/workspaces/{}/projects/{}/join/{}/', 'POST'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.invite.view
    ('/api/workspaces/{}/projects/{}/pages/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/access/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/archive/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/archive/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/description/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PagesDescriptionViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/duplicate/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.view
    ('/api/workspaces/{}/projects/{}/pages/{}/lock/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/pages/{}/lock/', 'POST'): "owned; no corpus session yet — batch 5 (app_pages session)",  # app :: pi_dash.app.views.page.base.PageViewSet
    ('/api/workspaces/{}/projects/{}/preferences/member/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.member.view
    ('/api/workspaces/{}/projects/{}/project-deploy-boards/', 'POST'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.DeployBoardViewSet
    ('/api/workspaces/{}/projects/{}/project-deploy-boards/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.DeployBoardViewSet
    ('/api/workspaces/{}/projects/{}/project-deploy-boards/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.DeployBoardViewSet
    ('/api/workspaces/{}/projects/{}/repository/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/repository/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/repository/bind/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/', 'POST'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/states/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.state.base.StateViewSet
    ('/api/workspaces/{}/projects/{}/states/{}/', 'PATCH'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.state.base.StateViewSet
    ('/api/workspaces/{}/projects/{}/states/{}/mark-default/', 'POST'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.state.base.StateViewSet
    ('/api/workspaces/{}/projects/{}/user-favorite-views/', 'POST'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/user-favorite-views/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/user-properties/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.base.view
    ('/api/workspaces/{}/projects/{}/views/', 'POST'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/work-items/{}/move/', 'POST'): "owned; no corpus session yet — batch 5 (app_issues follow-up: labels/relations/links/move)",  # app :: pi_dash.app.views.issue.move.view
    ('/api/workspaces/{}/quick-links/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.quick_link.QuickLinkViewSet
    ('/api/workspaces/{}/quick-links/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.quick_link.QuickLinkViewSet
    ('/api/workspaces/{}/quick-links/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.quick_link.QuickLinkViewSet
    ('/api/workspaces/{}/schedulers/', 'POST'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/schedulers/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/schedulers/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/sidebar-preferences/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.user_preference.view
    ('/api/workspaces/{}/stickies/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.sticky.WorkspaceStickyViewSet
    ('/api/workspaces/{}/stickies/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.sticky.WorkspaceStickyViewSet
    ('/api/workspaces/{}/stickies/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.sticky.WorkspaceStickyViewSet
    ('/api/workspaces/{}/user-activity/{}/export/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.view
    ('/api/workspaces/{}/user-favorite-projects/', 'POST'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.ProjectFavoritesViewSet
    ('/api/workspaces/{}/user-favorite-projects/{}/', 'DELETE'): "owned; no corpus session yet — batch 2 (app_project session)",  # app :: pi_dash.app.views.project.base.ProjectFavoritesViewSet
    ('/api/workspaces/{}/user-favorites/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.favorite.view
    ('/api/workspaces/{}/user-favorites/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.favorite.view
    ('/api/workspaces/{}/user-favorites/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.favorite.view
    ('/api/workspaces/{}/user-properties/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.user.view
    ('/api/workspaces/{}/users/notifications/mark-all-read/', 'POST'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.MarkAllReadNotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/archive/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/archive/', 'POST'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/read/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/users/notifications/{}/read/', 'POST'): "owned; no corpus session yet — batch 5 (app_notifications session)",  # app :: pi_dash.app.views.notification.base.NotificationViewSet
    ('/api/workspaces/{}/views/', 'POST'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.WorkspaceViewViewSet
    ('/api/workspaces/{}/views/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.WorkspaceViewViewSet
    ('/api/workspaces/{}/views/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.WorkspaceViewViewSet
    ('/api/workspaces/{}/views/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.WorkspaceViewViewSet
    ('/api/workspaces/{}/webhooks/', 'POST'): "owned; no corpus session yet — batch 5 (app webhooks session)",  # app :: pi_dash.app.views.webhook.base.view
    ('/api/workspaces/{}/webhooks/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app webhooks session)",  # app :: pi_dash.app.views.webhook.base.view
    ('/api/workspaces/{}/webhooks/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app webhooks session)",  # app :: pi_dash.app.views.webhook.base.view
    ('/api/workspaces/{}/webhooks/{}/regenerate/', 'POST'): "owned; no corpus session yet — batch 5 (app webhooks session)",  # app :: pi_dash.app.views.webhook.base.view
    ('/api/workspaces/{}/workspace-themes/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkspaceThemeViewSet
    ('/api/workspaces/{}/workspace-themes/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkspaceThemeViewSet
    ('/api/workspaces/{}/workspace-themes/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.base.WorkspaceThemeViewSet
    ('/api/workspaces/{}/workspace-views/', 'POST'): "owned; no corpus session yet — batch 5 (app_workspace session)",  # app :: pi_dash.app.views.workspace.member.view
}
