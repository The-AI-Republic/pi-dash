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


_SOFT_DELETE_TASK = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"


def _soft_delete_pin(model: str, pk_placeholder: str):
    """Exact-shape pin for PIDASHCONV-872 (one per DELETE request).

    Django sends ``using`` as a null kwarg (``mixins.py:78``); the
    app_cycles Rust handler sends a positional ``"default"``. Any byte
    change on either side fails loudly.
    """
    return (
        _SOFT_DELETE_TASK,
        "PIDASHCONV-872",
        {
            "task": _SOFT_DELETE_TASK,
            "args": ["db", model, pk_placeholder],
            "kwargs": {"using": None},
            "eta": None,
            "retries": 0,
            "queue": "celery",
        },
        {
            "task": _SOFT_DELETE_TASK,
            "args": ["db", model, pk_placeholder, "default"],
            "kwargs": {},
            "eta": None,
            "retries": 0,
            "queue": "celery",
        },
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
                # PIDASHCONV-871: Rust renders current_instance from the
                # fetched row map (extra keys archived_at/assignee_ids/
                # created_by/version, int sort_order); Django serializes
                # through CycleSerializer (24 fields, float sort_order).
                # Exact pinned bytes per side; any change fails loudly.
                known_job_shapes=[
                    (
                        "pi_dash.bgtasks.webhook_task.model_activity",
                        "PIDASHCONV-871",
                        {'task': 'pi_dash.bgtasks.webhook_task.model_activity', 'args': [], 'kwargs': {'model_name': 'cycle', 'model_id': '{cycle}', 'requested_data': {'name': 'Renamed cycle'}, 'current_instance': '{{\"cancelled_issues\":0,\"completed_issues\":0,\"description\":\"\",\"end_date\":null,\"external_id\":null,\"external_source\":null,\"id\":\"{cycle}\",\"is_favorite\":false,\"logo_props\":{{}},\"name\":\"Second cycle\",\"owned_by_id\":\"{admin}\",\"progress_snapshot\":{{}},\"project_id\":\"{project}\",\"sort_order\":65535.0,\"start_date\":null,\"status\":\"DRAFT\",\"total_issues\":0,\"view_props\":{{}},\"workspace_id\":\"{ws_id}\"}}', 'actor_id': '{admin}', 'slug': '{ws}', 'origin': 'http://localhost'}, 'eta': None, 'retries': 0, 'queue': 'celery'},
                        {'task': 'pi_dash.bgtasks.webhook_task.model_activity', 'args': [], 'kwargs': {'slug': '{ws}', 'origin': 'http://localhost', 'actor_id': '{admin}', 'model_id': '{cycle}', 'model_name': 'cycle', 'requested_data': {'name': 'Renamed cycle'}, 'current_instance': '{{\"archived_at\":null,\"assignee_ids\":[],\"cancelled_issues\":0,\"completed_issues\":0,\"created_by\":\"{admin}\",\"description\":\"\",\"end_date\":null,\"external_id\":null,\"external_source\":null,\"id\":\"{cycle}\",\"is_favorite\":false,\"logo_props\":{{}},\"name\":\"Second cycle\",\"owned_by_id\":\"{admin}\",\"progress_snapshot\":{{}},\"project_id\":\"{project}\",\"sort_order\":65535,\"start_date\":null,\"status\":\"DRAFT\",\"total_issues\":0,\"version\":1,\"view_props\":{{}},\"workspace_id\":\"{ws_id}\"}}'}, 'eta': None, 'retries': 0, 'queue': 'celery'},
                    ),
                ],
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
                known_job_shapes=[_soft_delete_pin("cycle", "{fin}")],
            ),
            Request(
                "DELETE", _B + "/cycles/{active}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
                known_job_shapes=[_soft_delete_pin("cycle", "{active}")],
            ),
            Request(
                "DELETE", _B + "/cycles/{blocker}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
                known_job_shapes=[_soft_delete_pin("cycle", "{blocker}")],
            ),
            Request(
                "DELETE", _B + "/cycles/{src}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
                known_job_shapes=[_soft_delete_pin("cycle", "{src}")],
            ),
            Request(
                "DELETE", _B + "/cycles/{dst}/",
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
                known_job_shapes=[_soft_delete_pin("cycle", "{dst}")],
            ),
            Request(
                "DELETE", _CYCLE,
                route="/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
                auth="admin", expect_status=204,
                known_job_shapes=[_soft_delete_pin("cycle", "{cycle}")],
            ),
            Request(
                "DELETE", "/api/workspaces/{ws}/projects/{project}/issues/{cissue}/",
                route="/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
                auth="admin", expect_status=204,
            ),
        ),
    ),
)
