# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

import pytest

from pi_dash.celery import app


pytestmark = pytest.mark.unit


def test_generic_git_sync_reuses_legacy_github_beat_schedule_name():
    schedule = app.conf.beat_schedule

    assert "github-issue-sync-every-4h" in schedule
    assert "git-issue-sync-every-4h" not in schedule
    assert (
        schedule["github-issue-sync-every-4h"]["task"]
        == "pi_dash.bgtasks.git_sync_task.sync_all_bindings"
    )
