# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier scheduler write flows (PIDASHCONV-823).

Built from the ``app_scheduler`` contract suite:
test_workspace_schedulers.py (list/create), test_workspace_scheduler_detail.py
(get/patch/delete), test_project_bindings.py (install), and
test_project_binding_detail.py (detail/patch/delete), plus one member-403
leg from test_permissions.py. Session-cookie auth (admin), like the suite.

Determinism: the install computes ``next_run_at`` from the RRULE bundle
at request time (``scheduler/views.py`` via ``bgtasks/scheduler.py``),
so the session uses a far-future ``dtstart`` (2030) with yearly/daily
rules — the next fire is the dtstart itself on both sides no matter
when each backend answers, and PATCH legs still exercise the recompute
path (``test_patch_rrule_recomputes_next_run``).
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed pod ids seeded by _seed_pods (addressed via static_context).
POD_ID = "55555555-0000-4000-8000-000000000001"
POD_FOREIGN_ID = "55555555-0000-4000-8000-000000000002"

MISSING_ID = "00000000-0000-0000-0000-000000000000"

# Far-future dtstart: the next fire resolves to the dtstart itself on
# both backends regardless of request-time skew (see module docstring).
DTSTART = "2030-06-01T12:00:00Z"

_S = "/api/workspaces/{ws}/schedulers"
_B = "/api/workspaces/{ws}/projects/{project}/scheduler-bindings"


def _seed_pods(conn) -> None:
    """One pod on RPJ (same-project PATCH leg) and one on RPX (foreign 400).

    The INSERT mirrors ``app_scheduler/seed_scheduler.py::create_pod``
    (same columns).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-other'")
        other_ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPX'")
        other_project_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        for pod_id, name, wid, pid in (
            (POD_ID, "runner-pod", ws_id, project_id),
            (POD_FOREIGN_ID, "foreign-pod", other_ws_id, other_project_id),
        ):
            cur.execute(
                """
                INSERT INTO pod
                   (id, name, description, is_default, created_at,
                    updated_at, workspace_id, project_id)
                VALUES (%s, %s, 'Seeded for write-replay', false, now(), now(), %s, %s)
                """,
                (pod_id, name, wid, pid),
            )
    conn.commit()


def _install(scheduler_placeholder: str, **extra) -> dict:
    payload = {
        "scheduler": scheduler_placeholder,
        "project": "{project}",
        "dtstart": DTSTART,
        "tzid": "UTC",
        "rrule": "FREQ=YEARLY",
    }
    payload.update(extra)
    return payload


SESSIONS = (
    Session(
        name="app_scheduler_lifecycle",
        domain="app_scheduler",
        source="app_scheduler/test_workspace_schedulers.py, test_workspace_scheduler_detail.py, test_project_bindings.py, test_project_binding_detail.py, test_permissions.py",
        auth_users=("admin", "member"),
        seed=_seed_pods,
        static_context={
            "pod": POD_ID,
            "pod_foreign": POD_FOREIGN_ID,
        },
        requests=(
            # Schedulers: empty list -> member 403 -> create full ->
            # create minimal -> validation 400s -> list -> detail.
            Request(
                "GET", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "m", "name": "M", "prompt": "x"},
                auth="member", expect_status=403,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "nightly", "name": "Nightly", "description": "d",
                      "prompt": "Scan.", "color": "#10B981"},
                auth="admin",
                capture={"sched": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "plain", "name": "Plain", "prompt": "x"},
                auth="admin",
                capture={"sched2": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "noprompt", "name": "NoPrompt"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "bad", "name": "Bad", "prompt": "x", "color": "red"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "nightly", "name": "Nightly", "prompt": "x"},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _S + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", _S + "/{sched}/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                auth="admin", expect_status=200,
            ),
            # Scheduler PATCH: rename/recolor/disable (the disable
            # arms the install-404 leg below) -> bad color 400 ->
            # duplicate slug 400.
            Request(
                "PATCH", _S + "/{sched}/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                json={"name": "Renamed", "color": "#FF0000", "is_enabled": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _S + "/{sched}/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                json={"color": "not-a-color"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _S + "/{sched}/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                json={"slug": "plain"},
                auth="admin", expect_status=400,
            ),
            # Bindings: install on the disabled scheduler 404s ->
            # install -> validation 400s -> duplicate 400 ->
            # RRULE:-prefixed install on a third scheduler.
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched}"),
                auth="admin", expect_status=404,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched2}"),
                auth="admin",
                capture={"bind": "id", "nxt": "next_run_at"},
                expect_status=201,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json={"scheduler": "{sched2}", "dtstart": DTSTART,
                      "rrule": "FREQ=YEARLY"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched2}", rrule="FREQ=NEVER"),
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched2}", tzid="Mars/Olympus"),
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched2}", rdates="tomorrow"),
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched2}"),
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _S + "/",
                route="/api/workspaces/{slug}/schedulers/",
                json={"slug": "third", "name": "Third", "prompt": "x"},
                auth="admin",
                capture={"sched3": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                json=_install("{sched3}", rrule="RRULE:FREQ=DAILY"),
                auth="admin",
                capture={"bind2": "id"},
                expect_status=201,
            ),
            Request(
                "GET", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                auth="admin", expect_status=200,
            ),
            # Binding PATCH: toggle -> rrule recompute -> locked
            # scheduler 400 -> same-project pod -> foreign pod 400.
            Request(
                "PATCH", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                json={"enabled": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                json={"rrule": "FREQ=DAILY"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                json={"scheduler": "{sched3}"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                json={"pod": "{pod}"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                json={"pod": "{pod_foreign}"},
                auth="admin", expect_status=400,
            ),
            # Deletes: binding -> 404 -> scheduler delete with a live
            # binding (bindings soft-delete inline) -> unknown 404.
            Request(
                "DELETE", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _B + "/{bind}/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _S + "/{sched3}/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _B + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/scheduler-bindings/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _S + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/schedulers/{pk}/",
                auth="admin", expect_status=404,
            ),
        ),
    ),
)
