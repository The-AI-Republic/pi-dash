# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier cycle write flows (PIDASHCONV-823).

Built from the ``app_cycles`` contract suite: test_cycles.py (CRUD,
date-check, cycle-issues, transfer, favorites, user-properties,
archive). Dates are computed at import (same values for both backends
within a run) so past/future ranges never rot.
"""

from datetime import datetime, timedelta, timezone

from ..session import Request, Session

_B = "/api/workspaces/{ws}/projects/{project}"
_CYCLES = _B + "/cycles/"
_CYCLE = _B + "/cycles/{cycle}/"


def _range(days_ago_start: int, days_ago_end: int) -> tuple[str, str]:
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


_PAST_START, _PAST_END = _range(90, 80)
_FIN_START, _FIN_END = _range(30, 1)
# Yesterday, not today: convert_to_utc stamps start_date == today with
# now() on each backend (faithfully ported, but the two nows differ by
# ms), while any other date coerces deterministically. The cycle stays
# active (start <= now <= end) and archive still 400s on end_date.
_ACTIVE_START = (datetime.now(timezone.utc) - timedelta(days=1)).date().isoformat()
_ACTIVE_END = (datetime.now(timezone.utc) + timedelta(days=30)).isoformat()

SESSIONS = (
    Session(
        name="app_cycles_lifecycle",
        domain="app_cycles",
        source="app_cycles/test_cycles.py",
        auth_users=("admin",),
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        payload_row_refs={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": [
                # Fresh CycleIssue row id, doubly nested inside the
                # current_instance JSON string; never in a response.
                ("kwargs/current_instance/created_cycle_issues/0/pk", "cycle_issues"),
            ],
        },
        requests=(
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Second cycle"},
                auth="admin", capture={"cycle": "id"}, expect_status=201,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Half", "start_date": "2026-01-01"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Bad", "start_date": "2026-06-01", "end_date": "2026-01-01"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _CYCLE,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                json={"name": "Renamed cycle"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PUT", _CYCLE,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                json={"name": "Put cycle"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", "/api/workspaces/{ws}/projects/{project}/issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/",
                json={"name": "Cycle issue"},
                auth="admin", capture={"cissue": "id"}, expect_status=201,
            ),
            Request(
                "POST", _CYCLE + "cycle-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/",
                json={"issues": ["{cissue}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _CYCLE + "cycle-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _CYCLE + "cycle-issues/{cissue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/{issue_id}/",
                json={"name": "x"},
                auth="admin", expect_status=500,
            ),
            Request(
                "PUT", _CYCLE + "cycle-issues/{cissue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/{issue_id}/",
                json={"name": "x"},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", _B + "/cycles/date-check/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/",
                json={"start_date": _PAST_START, "end_date": _PAST_END},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Blocker", "start_date": _PAST_START, "end_date": _PAST_END},
                auth="admin", capture={"blocker": "id"}, expect_status=201,
            ),
            Request(
                "POST", _B + "/cycles/date-check/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/",
                json={"start_date": _PAST_START, "end_date": _PAST_END},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _B + "/cycles/date-check/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/date-check/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Source"},
                auth="admin", capture={"src": "id"}, expect_status=201,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Dest"},
                auth="admin", capture={"dst": "id"}, expect_status=201,
            ),
            Request(
                "POST", _B + "/cycles/{src}/cycle-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/",
                json={"issues": ["{cissue}"]},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _B + "/cycles/{src}/transfer-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/transfer-issues/",
                json={"new_cycle_id": "{dst}"},
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _B + "/cycles/{src}/transfer-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/transfer-issues/",
                json={},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _B + "/cycles/{src}/transfer-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/transfer-issues/",
                json={"new_cycle_id": "00000000-0000-0000-0000-000000000000"},
                auth="admin", expect_status=500,
            ),
            Request(
                "GET", _CYCLE + "user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/user-properties/",
                auth="admin", capture={"cuprops": "id"}, expect_status=200,
            ),
            Request(
                "PATCH", _CYCLE + "user-properties/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/user-properties/",
                json={"filters": {"priority": "high"}},
                auth="admin", expect_status=201,
            ),
            Request(
                "POST", _B + "/user-favorite-cycles/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/",
                json={"cycle": "{cycle}"},
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/user-favorite-cycles/{cycle}/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/user-favorite-cycles/{cycle}/",
                route="/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Finish me", "start_date": _FIN_START, "end_date": _FIN_END},
                auth="admin", capture={"fin": "id"}, expect_status=201,
            ),
            Request(
                "POST", _B + "/cycles/{fin}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _B + "/cycles/{fin}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/archive/",
                auth="admin", expect_status=204,
            ),
            Request(
                "POST", _CYCLES,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/",
                json={"name": "Active", "start_date": _ACTIVE_START, "end_date": _ACTIVE_END},
                auth="admin", capture={"active": "id"}, expect_status=201,
            ),
            Request(
                "POST", _B + "/cycles/{active}/archive/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/archive/",
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _B + "/cycles/{dst}/cycle-issues/{cissue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/cycle-issues/{issue_id}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/cycles/{fin}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/cycles/{active}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/cycles/{blocker}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/cycles/{src}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _B + "/cycles/{dst}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", _CYCLE,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/projects/{project}/issues/{cissue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
