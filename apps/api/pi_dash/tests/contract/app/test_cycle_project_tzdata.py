# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Django's pytz carries the same IANA release as Rust's chrono-tz (PDASHOSS01-331).

While Django pinned pytz 2024.1, a July cycle date in America/Asuncion came
back with -04:00 from Django and -03:00 from Rust, because Paraguay moved to
permanent UTC-3 in tzdata 2025a. Both backends must render offsets from one
release, so the pytz pin and chrono-tz move together.
"""

from datetime import datetime, timezone as dt_timezone
from unittest.mock import patch

import pytest
import pytz
from rest_framework import status

from pi_dash.db.models import Cycle, Project, ProjectMember, User, Workspace, WorkspaceMember

ZONE = "America/Asuncion"
START = datetime(2026, 7, 1, 12, 0, 0, 123456, tzinfo=dt_timezone.utc)
END = datetime(2027, 1, 15, 12, 0, 0, tzinfo=dt_timezone.utc)
START_WIRE = "2026-07-01T09:00:00.123456-03:00"
END_WIRE = "2027-01-15T09:00:00-03:00"
RECENT_VISITED = "pi_dash.app.views.cycle.base.recent_visited_task"


def test_pytz_tzdata_matches_chrono_tz():
    """Mirrors `chrono_tz_tzdata_matches_the_pytz_pin` in `rust-api/.../tz_zones.rs`."""
    assert pytz.OLSON_VERSION == "2025b"


@pytest.mark.parametrize(
    "model,field",
    [(Cycle, "timezone"), (Project, "timezone"), (User, "user_timezone"), (Workspace, "timezone")],
)
def test_timezone_choices_follow_common_timezones(model, field):
    names = {value for value, _ in model._meta.get_field(field).choices}
    assert names == set(pytz.common_timezones)
    assert "America/Coyhaique" in names
    assert "Asia/Choibalsan" not in names


@pytest.mark.contract
@pytest.mark.django_db
class TestCycleDatesUseCurrentParaguayOffset:
    @pytest.fixture
    def asuncion_cycle(self, workspace, project, create_user):
        WorkspaceMember.objects.get_or_create(workspace=workspace, member=create_user, defaults={"role": 20})
        ProjectMember.objects.get_or_create(
            project=project, member=create_user, defaults={"role": 20, "is_active": True}
        )
        project.timezone = ZONE
        project.save(update_fields=["timezone"])
        return Cycle.objects.create(
            name="Asuncion cycle",
            project=project,
            workspace=workspace,
            owned_by=create_user,
            start_date=START,
            end_date=END,
        )

    @patch(RECENT_VISITED)
    def test_retrieve(self, _recent_visited, session_client, workspace, project, asuncion_cycle):
        url = f"/api/workspaces/{workspace.slug}/projects/{project.id}/cycles/{asuncion_cycle.id}/"
        resp = session_client.get(url)
        assert resp.status_code == status.HTTP_200_OK, resp.content
        assert resp.json()["start_date"] == START_WIRE
        assert resp.json()["end_date"] == END_WIRE

    def test_list(self, session_client, workspace, project, asuncion_cycle):
        resp = session_client.get(f"/api/workspaces/{workspace.slug}/projects/{project.id}/cycles/")
        assert resp.status_code == status.HTTP_200_OK, resp.content
        row = next(item for item in resp.json() if item["id"] == str(asuncion_cycle.id))
        assert row["start_date"] == START_WIRE
        assert row["end_date"] == END_WIRE
