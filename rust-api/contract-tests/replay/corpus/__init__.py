# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Request corpus: realistic multi-step write sessions per domain."""

from . import app_issues

ALL_SESSIONS = [
    *app_issues.SESSIONS,
]

BY_NAME = {s.name: s for s in ALL_SESSIONS}
assert len(BY_NAME) == len(ALL_SESSIONS), "duplicate session names"
