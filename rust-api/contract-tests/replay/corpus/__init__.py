# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Request corpus: realistic multi-step write sessions per domain."""

from . import (
    app_cycles,
    app_issues,
    app_modules,
    app_project,
    auth_device,
    auth_magic,
    auth_session,
    license,
    space,
    v1_work_items,
)

ALL_SESSIONS = [
    *app_cycles.SESSIONS,
    *app_issues.SESSIONS,
    *app_modules.SESSIONS,
    *app_project.SESSIONS,
    *auth_device.SESSIONS,
    *auth_magic.SESSIONS,
    *auth_session.SESSIONS,
    *license.SESSIONS,
    *space.SESSIONS,
    *v1_work_items.SESSIONS,
]

BY_NAME = {s.name: s for s in ALL_SESSIONS}
assert len(BY_NAME) == len(ALL_SESSIONS), "duplicate session names"
