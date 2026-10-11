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
    ('/api/workspaces/file-assets/{}/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/workspaces/file-assets/{}/{}/restore/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.FileAssetViewSet
    ('/api/workspaces/{}/ai-assistant/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.external.base.view
    ('/api/workspaces/{}/analytic-view/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/analytic-view/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/analytic-view/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.AnalyticViewViewset
    ('/api/workspaces/{}/export-analytics/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.analytic.base.view
    ('/api/workspaces/{}/export-issues/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.exporter.base.view
    ('/api/workspaces/{}/file-assets/', 'POST'): "owned; no corpus session yet — batch 5 (app_assets session)",  # app :: pi_dash.app.views.asset.base.view
    ('/api/workspaces/{}/integrations/git/accounts/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/integrations/git/accounts/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/integrations/github/connect/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/integrations/github/disconnect/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/ai-assistant/', 'POST'): "owned; no corpus session yet — batch 5 (app misc session)",  # app :: pi_dash.app.views.external.base.view
    ('/api/workspaces/{}/projects/{}/github/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/github/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/github/bind/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.github.view
    ('/api/workspaces/{}/projects/{}/repository/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/repository/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/repository/bind/', 'POST'): "owned; no corpus session yet — batch 5 (app_integrations session)",  # app :: pi_dash.app.views.integration.git.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/', 'POST'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/scheduler-bindings/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/projects/{}/user-favorite-views/', 'POST'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/user-favorite-views/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewFavoriteViewSet
    ('/api/workspaces/{}/projects/{}/views/', 'POST'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/projects/{}/views/{}/', 'PUT'): "owned; no corpus session yet — batch 5 (app_views_search session)",  # app :: pi_dash.app.views.view.base.IssueViewViewSet
    ('/api/workspaces/{}/schedulers/', 'POST'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/schedulers/{}/', 'DELETE'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
    ('/api/workspaces/{}/schedulers/{}/', 'PATCH'): "owned; no corpus session yet — batch 5 (app_scheduler session)",  # app :: pi_dash.app.views.scheduler.views.view
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
}
