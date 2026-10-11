# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier intake write flows (PIDASHCONV-823).

Built from the ``app_intake`` contract suite: test_intake.py
(intakes, intake-issues, inbox aliases) and
test_intake_issue_detail.py (detail PATCH/DELETE, versions).
Session-cookie auth (admin), like the suite.

Two suite legs behave differently here than in the suite tenant:
the replay RPJ project HAS a default state, so ``status: 1``
accepts the intake issue instead of 400ing
(``test_patch_accept_without_default_400`` covers the stateless
project); and intake creates 500 identically both sides (the
ported perform_create bug the suite pins byte for byte).
"""

from psycopg.rows import dict_row

from .. import effects
from ..session import Request, Session

_SOFT_DELETE_TASK = "pi_dash.bgtasks.deletion_task.soft_delete_related_objects"


def _intake_issue_soft_delete_pins(issue_placeholder: str, bridge_placeholder: str):
    """Exact-shape pins for PIDASHCONV-913 (intake-issue DELETE legs).

    The destroy emits two soft_delete enqueues (``issue`` + the
    ``intakeissue`` bridge); Django sends ``using`` as a null kwarg
    on both, Rust as a null fourth positional.
    """
    pins = []
    for model, pk in (("issue", issue_placeholder),
                      ("intakeissue", bridge_placeholder)):
        pins.append((
            _SOFT_DELETE_TASK,
            "PIDASHCONV-913",
            {
                "task": _SOFT_DELETE_TASK,
                "args": ["db", model, pk],
                "kwargs": {"using": None},
                "eta": None,
                "retries": 0,
                "queue": "celery",
            },
            {
                "task": _SOFT_DELETE_TASK,
                "args": ["db", model, pk, None],
                "kwargs": {},
                "eta": None,
                "retries": 0,
                "queue": "celery",
            },
        ))
    return pins


def _intake_soft_delete_pin(pk_placeholder: str):
    """Exact-shape pin for PIDASHCONV-910 (intake DELETE legs).

    Django sends ``using`` as a null kwarg (``mixins.py:78``); the
    intake Rust handler sends a null fourth positional. Any byte
    change on either side fails loudly.
    """
    return (
        _SOFT_DELETE_TASK,
        "PIDASHCONV-910",
        {
            "task": _SOFT_DELETE_TASK,
            "args": ["db", "intake", pk_placeholder],
            "kwargs": {"using": None},
            "eta": None,
            "retries": 0,
            "queue": "celery",
        },
        {
            "task": _SOFT_DELETE_TASK,
            "args": ["db", "intake", pk_placeholder, None],
            "kwargs": {},
            "eta": None,
            "retries": 0,
            "queue": "celery",
        },
    )

# Fixed intake ids seeded by _seed_intake (addressed via static_context).
INTAKE_DEFAULT_ID = "44444444-0000-4000-8000-000000000001"
INTAKE_DISP_ID = "44444444-0000-4000-8000-000000000002"
INTAKE_DISP2_ID = "44444444-0000-4000-8000-000000000003"

MISSING_ID = "11111111-1111-4111-8111-111111111111"

_I = "/api/workspaces/{ws}/projects/{project}"


def _seed_intake(conn) -> None:
    """Default + two disposable intakes on RPJ.

    The INSERT mirrors ``_harness.seed.create_intake`` (same
    columns). Disposables back the DELETE legs (creates 500
    upstream, so the suite seeds these straight into Postgres
    too).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        for intake_id, name, is_default in (
            (INTAKE_DEFAULT_ID, "Contract intake", True),
            (INTAKE_DISP_ID, "Disposable", False),
            (INTAKE_DISP2_ID, "Disposable 2", False),
        ):
            cur.execute(
                """
                INSERT INTO intakes
                   (id, workspace_id, project_id, name, description, is_default,
                    view_props, logo_props, created_at, updated_at)
                VALUES (%s, %s, %s, %s, '', %s, '{}', '{}', now(), now())
                """,
                (intake_id, ws_id, project_id, name, is_default),
            )
    conn.commit()


SESSIONS = (
    Session(
        name="app_intake_lifecycle",
        domain="app_intake",
        source="app_intake/test_intake.py, test_intake_issue_detail.py",
        auth_users=("admin",),
        seed=_seed_intake,
        static_context={
            "intake": INTAKE_DEFAULT_ID,
            "disp": INTAKE_DISP_ID,
            "disp2": INTAKE_DISP2_ID,
        },
        # The Rust-only sync version row stamps last_saved_at =
        # now() (PIDASHCONV-911); blanked so the exact-row pins
        # below can match.
        timestamp_columns={
            "issue_description_versions": ["last_saved_at"],
        },
        payload_time_paths={
            "pi_dash.bgtasks.issue_activities_task.issue_activity": ["kwargs/epoch"],
        },
        # Known divergence (PIDASHCONV-911): Rust's intake-issue
        # create writes a sync issue_description_versions row
        # Django leaves to the async worker. One exact-row
        # declaration per created issue; each must match >= once
        # and any unmatched row still fails.
        known_rust_extra_rows=[
            ("issue_description_versions", "PIDASHCONV-911", {
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "created_by_id": "{admin}",
                "deleted_at": None,
                "description_binary": None,
                "description_html": "<p></p>",
                "description_json": {},
                "description_stripped": "",
                "id": effects.VOLATILE_PLACEHOLDER,
                "issue_id": "{i1}",
                "last_saved_at": effects.VOLATILE_PLACEHOLDER,
                "owned_by_id": "{admin}",
                "project_id": "{project}",
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "updated_by_id": None,
                "workspace_id": "{ws_id}",
            }),
            ("issue_description_versions", "PIDASHCONV-911", {
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "created_by_id": "{admin}",
                "deleted_at": None,
                "description_binary": None,
                "description_html": "<p></p>",
                "description_json": {},
                "description_stripped": "",
                "id": effects.VOLATILE_PLACEHOLDER,
                "issue_id": "{i2}",
                "last_saved_at": effects.VOLATILE_PLACEHOLDER,
                "owned_by_id": "{admin}",
                "project_id": "{project}",
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "updated_by_id": None,
                "workspace_id": "{ws_id}",
            }),
            ("issue_description_versions", "PIDASHCONV-911", {
                "created_at": effects.VOLATILE_PLACEHOLDER,
                "created_by_id": "{admin}",
                "deleted_at": None,
                "description_binary": None,
                "description_html": "<p></p>",
                "description_json": {},
                "description_stripped": "",
                "id": effects.VOLATILE_PLACEHOLDER,
                "issue_id": "{i3}",
                "last_saved_at": effects.VOLATILE_PLACEHOLDER,
                "owned_by_id": "{admin}",
                "project_id": "{project}",
                "updated_at": effects.VOLATILE_PLACEHOLDER,
                "updated_by_id": None,
                "workspace_id": "{ws_id}",
            }),
        ],
        requests=(
            # Intakes: read -> create-500 (ported bug, pinned
            # byte-for-byte both sides) -> inbox-create-500 ->
            # missing-deleted_at 400 -> patch -> delete-default
            # 400 -> delete-disposable 204 -> 404.
            Request(
                "GET", _I + "/intakes/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _I + "/intakes/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/",
                json={"name": "Second intake", "deleted_at": None},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", _I + "/inboxes/",
                route="/api/workspaces/{slug}/projects/{project_id}/inboxes/",
                json={"name": "Second intake", "deleted_at": None},
                auth="admin", expect_status=500,
            ),
            Request(
                "POST", _I + "/intakes/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/",
                json={"name": "No deleted_at"},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _I + "/intakes/{intake}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
                json={"name": "Renamed"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _I + "/inboxes/{intake}/",
                route="/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/",
                json={"name": "Renamed again"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _I + "/intakes/{intake}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
                auth="admin", expect_status=400,
            ),
            Request(
                "DELETE", _I + "/intakes/{disp}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-910): Rust sends
                # soft_delete `using` as a null positional,
                # Django as a null kwarg.
                known_job_shapes=[_intake_soft_delete_pin("{disp}")],
            ),
            Request(
                "DELETE", _I + "/inboxes/{disp2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-910): same null
                # positional vs null kwarg through the alias.
                known_job_shapes=[_intake_soft_delete_pin("{disp2}")],
            ),
            Request(
                "GET", _I + "/intakes/{disp}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Intake issues: create -> validation 400s -> list ->
            # retrieve + alias -> rename -> alias-priority ->
            # blank-name 400 -> bad-status 400 -> silent
            # migration path.
            Request(
                "POST", _I + "/intake-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"name": "II1", "priority": "high"}},
                auth="admin",
                # The first create auto-mints the Triage state per
                # backend (intake/base.py:239-249); captured so
                # later bodies substitute (later creates reuse it).
                capture={"ii1": "id", "i1": "issue/id",
                         "triage": "issue/state_id"},
                expect_status=200,
            ),
            Request(
                "POST", _I + "/intake-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {}},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _I + "/intake-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"name": "x", "priority": "bogus"}},
                auth="admin", expect_status=400,
            ),
            Request(
                "GET", _I + "/intake-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _I + "/inbox-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"issue": {"name": "II1b"}},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _I + "/inbox-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/",
                json={"issue": {"priority": "low"}},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"issue": {"name": ""}},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"status": 7},
                auth="admin", expect_status=400,
            ),
            Request(
                "PATCH", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={
                    "skip_activity": True,
                    "description_html": "<p>migrated</p>",
                    "issue": {"description_html": "<p>migrated</p>"},
                },
                auth="admin", expect_status=200,
            ),
            # Second issue via the inbox alias; triage-duplicate
            # rejection; accept flow (RPJ has a default state);
            # deletes (direct + alias + missing).
            Request(
                "POST", _I + "/inbox-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/inbox-issues/",
                json={"issue": {"name": "II2", "priority": "high"}},
                auth="admin",
                capture={"ii2": "id", "i2": "issue/id"},
                expect_status=200,
            ),
            Request(
                "PATCH", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                json={"duplicate_to": "{i2}"},
                auth="admin", expect_status=400,
                # The 400 text embeds the per-side target pk.
                body_regex_blanks=[
                    'Invalid pk "[0-9a-f-]{36}"',
                ],
            ),
            Request(
                "POST", _I + "/intake-issues/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/",
                json={"issue": {"name": "II3", "priority": "high"}},
                auth="admin",
                capture={"ii3": "id", "i3": "issue/id"},
                expect_status=200,
            ),
            # No accept leg (PATCH status 1): Rust 500s where
            # Django 200s — PIDASHCONV-912 keeps the repro, 904
            # precedent. II3 still exercises a third create.
            Request(
                "DELETE", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-913): Rust sends
                # soft_delete `using` as a null positional on both
                # enqueues, Django as a null kwarg.
                known_job_shapes=_intake_issue_soft_delete_pins("{i1}", "{ii1}"),
            ),
            Request(
                "GET", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _I + "/intake-issues/{i1}/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _I + "/inbox-issues/{i2}/",
                route="/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/",
                auth="admin", expect_status=204,
                # Known divergence (PIDASHCONV-913): same double
                # null-positional vs null-kwarg through the alias.
                known_job_shapes=_intake_issue_soft_delete_pins("{i2}", "{ii2}"),
            ),
            Request(
                "DELETE", _I + "/intake-issues/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/",
                auth="admin", expect_status=404,
            ),
        ),
    ),
)
