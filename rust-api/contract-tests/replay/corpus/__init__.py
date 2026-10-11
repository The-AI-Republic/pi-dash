# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Request corpus: realistic multi-step write sessions per domain."""

from . import (
    app_cycles,
    app_intake,
    app_issues,
    app_modules,
    app_notifications,
    app_pages,
    app_project,
    app_scheduler,
    app_views,
    app_webhooks,
    app_workspace,
    assistant,
    auth_device,
    auth_magic,
    auth_session,
    license,
    loop,
    prompting,
    runner,
    runner_web,
    space,
    v1_assets,
    v1_cycles_modules,
    v1_projects,
    v1_work_items,
)

ALL_SESSIONS = [
    *app_cycles.SESSIONS,
    *app_intake.SESSIONS,
    *app_issues.SESSIONS,
    *app_modules.SESSIONS,
    *app_notifications.SESSIONS,
    *app_pages.SESSIONS,
    *app_project.SESSIONS,
    *app_scheduler.SESSIONS,
    *app_views.SESSIONS,
    *app_webhooks.SESSIONS,
    *app_workspace.SESSIONS,
    *assistant.SESSIONS,
    *auth_device.SESSIONS,
    *auth_magic.SESSIONS,
    *auth_session.SESSIONS,
    *license.SESSIONS,
    *loop.SESSIONS,
    *prompting.SESSIONS,
    *runner.SESSIONS,
    *runner_web.SESSIONS,
    *space.SESSIONS,
    *v1_assets.SESSIONS,
    *v1_cycles_modules.SESSIONS,
    *v1_projects.SESSIONS,
    *v1_work_items.SESSIONS,
]

BY_NAME = {s.name: s for s in ALL_SESSIONS}
assert len(BY_NAME) == len(ALL_SESSIONS), "duplicate session names"
