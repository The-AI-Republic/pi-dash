# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: v1 scheduler write flows (NEWPIDASH-32).

Built from the ``v1_schedulers`` contract suite (test_writes.py):
definition create (POST workspaces/<slug>/schedulers/), definition
edit (PATCH .../schedulers/<id>/), definition delete (DELETE
.../schedulers/<id>/), project install (POST
.../projects/<p>/schedulers/), project uninstall (DELETE
.../schedulers/<id>/binding/), binding edit (PATCH .../binding/),
and run-now (POST .../run-now/). API-key auth throughout (the
suite's ``http`` harness).

Determinism: installs use a far-future ``dtstart`` (2030) with yearly
rules — the next fire is the dtstart itself on both sides no matter
when each backend answers (same trick as the app_scheduler corpus).
Run-now's happy path stamps ``next_run_at = now()`` per backend, so
this session covers that pair with its 404 (not installed) and 412
(stale precondition) legs only; the 200 is pinned by the contract
suite (test_run_now_marks_due).
"""

from ..session import Request, Session

# Far-future dtstart: the next fire resolves to the dtstart itself on
# both backends regardless of request-time skew (see module docstring).
DTSTART = "2030-06-01T12:00:00Z"

_WS = "/api/v1/workspaces/{ws}/schedulers"
_PROJ = "/api/v1/workspaces/{ws}/projects/{project}/schedulers"


def _install(ref: str, **extra) -> dict:
    payload = {
        "scheduler": ref,
        "dtstart": DTSTART,
        "tzid": "UTC",
        "rrule": "FREQ=YEARLY",
    }
    payload.update(extra)
    return payload


SESSIONS = (
    Session(
        name="v1_schedulers_lifecycle",
        domain="v1_schedulers",
        source="v1_schedulers/test_writes.py (definition create/edit/delete, "
        "project install/uninstall, binding edit, run-now 404/412)",
        auth_users=("api_key",),
        # process_logs embeds two timezone.now() stamps per backend
        # (Django micros, Rust seconds): each side mints its own now,
        # so both blank via the explicit time map (same as v1_projects).
        payload_time_paths={
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        requests=(
            # Definitions: create full -> validation 400s -> duplicate
            # 400 -> create minimal.
            Request(
                "POST", _WS + "/",
                route="/api/v1/workspaces/{slug}/schedulers/",
                json={"slug": "nightly", "name": "Nightly",
                      "description": "Looks for regressions.",
                      "prompt": "Scan the project for stalled issues."},
                auth="api_key",
                capture={"sched": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _WS + "/",
                route="/api/v1/workspaces/{slug}/schedulers/",
                json={},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _WS + "/",
                route="/api/v1/workspaces/{slug}/schedulers/",
                json={"slug": "nightly", "name": "Nightly", "prompt": "x"},
                auth="api_key", expect_status=400,
            ),
            Request(
                "POST", _WS + "/",
                route="/api/v1/workspaces/{slug}/schedulers/",
                json={"slug": "plain", "name": "Plain", "prompt": "x"},
                auth="api_key",
                capture={"sched2": "id"},
                expect_status=201,
            ),
            # Installs: by slug -> by id -> duplicate 409 -> unknown 404.
            Request(
                "POST", _PROJ + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/",
                json=_install("nightly", extra_context="Only the API."),
                auth="api_key",
                capture={"bind": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _PROJ + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/",
                json=_install("{sched2}"),
                auth="api_key",
                capture={"bind2": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _PROJ + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/",
                json=_install("nightly"),
                auth="api_key", expect_status=409,
            ),
            Request(
                "POST", _PROJ + "/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/",
                json=_install("no-such-scheduler"),
                auth="api_key", expect_status=404,
            ),
            # Binding PATCH: extra_context -> enable toggle -> validation 400.
            Request(
                "PATCH", _PROJ + "/{sched}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                json={"extra_context": "## Rules\n\nRule D: queued runs are not stuck."},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _PROJ + "/{sched}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                json={"enabled": False},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _PROJ + "/{sched}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                json={"enabled": True},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _PROJ + "/{sched}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                json={"rrule": "FREQ=NEVER"},
                auth="api_key", expect_status=400,
            ),
            # Run-now: 404 when not installed -> 412 on stale precondition.
            Request(
                "POST", _WS + "/",
                route="/api/v1/workspaces/{slug}/schedulers/",
                json={"slug": "third", "name": "Third", "prompt": "x"},
                auth="api_key",
                capture={"sched3": "id"},
                expect_status=201,
            ),
            Request(
                "POST", _PROJ + "/{sched3}/run-now/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/run-now/",
                auth="api_key", expect_status=404,
            ),
            Request(
                "POST", _PROJ + "/{sched}/run-now/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/run-now/",
                headers={"If-Match": "2020-01-01T00:00:00Z"},
                auth="api_key", expect_status=412,
            ),
            # Definition PATCH: prompt -> name/description -> blank 400.
            Request(
                "PATCH", _WS + "/{sched}/",
                route="/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
                json={"prompt": "Line one.\n\nLine two, with \"quotes\" and `code`."},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/{sched}/",
                route="/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
                json={"description": "", "name": "  Renamed  "},
                auth="api_key", expect_status=200,
            ),
            Request(
                "PATCH", _WS + "/{sched}/",
                route="/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
                json={"prompt": "   "},
                auth="api_key", expect_status=400,
            ),
            # Uninstalls: binding -> 404 -> definition with a live binding
            # (bindings soft-delete inline) -> unknown 404.
            Request(
                "DELETE", _PROJ + "/{sched2}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _PROJ + "/{sched2}/binding/",
                route="/api/v1/workspaces/{slug}/projects/{project_id}/schedulers/{scheduler_id}/binding/",
                auth="api_key", expect_status=404,
            ),
            Request(
                "DELETE", _WS + "/{sched}/",
                route="/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
                auth="api_key", expect_status=204,
            ),
            Request(
                "DELETE", _WS + "/00000000-0000-0000-0000-000000000000/",
                route="/api/v1/workspaces/{slug}/schedulers/{scheduler_id}/",
                auth="api_key", expect_status=404,
            ),
        ),
    ),
)
