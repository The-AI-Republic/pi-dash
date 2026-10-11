# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: runner web write flows (PIDASHCONV-823).

Built from the ``runner`` contract suite's web surface:
test_web_catalog.py (pod create/rename/delete + guards),
test_web_machines.py (revoke/rotate/create-runner/delete +
ticket mint), test_web_runners.py (patch/revoke/revive/delete +
invites), test_web_runs.py (create/cancel/release-pin + re-tick),
test_web_approvals.py (decide incl. the 500 bug), test_web_chat.py
(session create/message/warm/cancel/close, chat approvals decide,
all online + offline legs).

Approvals are seeded black-box through the daemon endpoints inside
the flow (as the suites do), so the session also covers the daemon
approval pairs a second time. The offline runner, its machine, and
its ``mt_`` token seed by SQL (mirroring a POST-runners/ row); the
online runner seeds the same way with status flipped, since nothing
in the flow can flip it mid-session. Revoke/delete targets come from
two HTTP runner creates (revoke is idempotent, delete wants an
unrevoked runner).

Framework 500s (malformed work-item ``ValidationError``, decide-read
``NotSupportedError``, offline fan-out ``RunnerOfflineError``) use
``skip_body``: Django renders its DEBUG HTML page (host/ports/
settings/memory addresses) while Rust answers its generic JSON 500
or its proxied upstream-Django page; status, exception-kind presence
pins, DB, jobs, and sinks still compare.
"""

from __future__ import annotations

import os

from ..session import Request, Session
from ._csrf import csrf as _csrf
from ._csrf import prime as _prime

WEB = "/api/runners"
DAEMON = "/api/v1/runner"
_MISSING = "00000000-0000-0000-0000-000000000000"
_NXUUID = "33333333-4444-5555-6666-777777777777"

MT_RAW = "mt_rp823webofff0000000000000000000001"
AIDW = "22222222-3333-4444-5555-666666666666"

_STATIC = {
    "mt": MT_RAW,
    "aidw": AIDW,
    "missing": _MISSING,
    "nxuuid": _NXUUID,
    "k1": "e1" * 16,
    "k2": "e2" * 16,
}

_SETUP_KEYS = (
    "pod", "pod3", "roff", "ron", "moff", "mon", "mspare", "mforeign",
    "w0", "w1", "w2", "w3", "w4", "w5", "s3", "s4", "s5",
)


def _seed_extras(conn) -> None:
    from _harness.seed import Seeder, SeedTracker

    from .space import _ConnDb

    adapter = _ConnDb(conn)
    seeder = Seeder(
        adapter, SeedTracker(adapter), tag="RP823",
        secret_key=os.environ["SECRET_KEY"],
    )
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPJ'")
        project_id = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM users WHERE email = 'replay-outsider@example.com'")
        outsider_id = str(cur.fetchone()[0])

    pod = seeder.create_pod(ws_id, project_id, name="rp-webpod", is_default=True)
    seeder.create_pod(ws_id, project_id, name="rp-busy")

    def _sql_runner(name, host_label, status):
        row = seeder.enroll_runner(
            admin_id, ws_id, pod["id"],
            f"apd_en_{host_label}0000000000000"[:32], name=name,
        )
        machine = seeder.create_dev_machine(admin_id, host_label=host_label)
        with conn.cursor() as cur:
            cur.execute(
                "UPDATE runner SET enrolled_at = now(), status = %s,"
                " host_label = %s, dev_machine_id = %s WHERE id = %s",
                (status, host_label, machine["id"], row["id"]),
            )
        return row, machine

    roff, moff = _sql_runner("rp-wroff", "rp-wmoff", "offline")
    seeder.create_machine_token(
        admin_id, ws_id, MT_RAW, host_label="rp-wmoff",
        dev_machine_id=moff["id"],
    )
    ron, mon = _sql_runner("rp-wron", "rp-wmon", "online")
    mspare = seeder.create_dev_machine(admin_id, host_label="rp-wmspare")
    # Bare machines are out of workspace scope (the view needs a bound
    # runner or token); the spare's token is never presented, it only
    # pulls the machine into scope for rotate/delete.
    seeder.create_machine_token(
        admin_id, ws_id, "mt_rp823spare00000000000000000000001",
        host_label="rp-wmspare", dev_machine_id=mspare["id"],
    )
    seeder.create_dev_machine(outsider_id, host_label="rp-wmforeign")

    with conn.cursor() as cur:
        specs = (
            ("w0", "assigned", roff["id"], None),
            ("w1", "assigned", roff["id"], None),
            ("w2", "queued", roff["id"], "pin"),
            ("w3", "completed", roff["id"], None),
            ("w4", "assigned", roff["id"], None),
            ("w5", "assigned", roff["id"], None),
        )
        for tag, status, runner_id, pin in specs:
            run = seeder.create_daemon_run(
                admin_id, ws_id, pod["id"], runner_id, status=status,
            )
            cur.execute(
                "UPDATE agent_run SET prompt = %s WHERE id = %s",
                (f"rp-{tag}", run["id"]),
            )
            if pin:
                cur.execute(
                    "UPDATE agent_run SET pinned_runner_id = %s WHERE id = %s",
                    (runner_id, run["id"]),
                )
        chats = (
            ("s3", roff["id"]),
            ("s4", roff["id"]),
            ("s5", ron["id"]),
        )
        for stag, runner_id in chats:
            sess = seeder.create_chat_session(
                ws_id, runner_id, pod["id"], admin_id
            )
            cur.execute(
                "UPDATE agent_chat_session SET local_thread_id = %s WHERE id = %s",
                (f"rp-{stag}", sess["id"]),
            )
            seeder.create_chat_message(sess["id"], role="user", content="hello")


def _setup(world, config) -> dict:
    import psycopg

    out = {}
    with psycopg.connect(config.django_db) as conn, conn.cursor() as cur:
        cur.execute("SELECT id FROM pod WHERE name = 'rp-webpod'")
        out["pod"] = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM pod WHERE name = 'rp-busy'")
        out["pod3"] = str(cur.fetchone()[0])
        for name, key in (("rp-wroff", "roff"), ("rp-wron", "ron")):
            cur.execute("SELECT id FROM runner WHERE name = %s", (name,))
            out[key] = str(cur.fetchone()[0])
        for label, key in (
            ("rp-wmoff", "moff"), ("rp-wmon", "mon"),
            ("rp-wmspare", "mspare"), ("rp-wmforeign", "mforeign"),
        ):
            cur.execute("SELECT id FROM dev_machine WHERE host_label = %s", (label,))
            out[key] = str(cur.fetchone()[0])
        for tag in ("w0", "w1", "w2", "w3", "w4", "w5"):
            cur.execute("SELECT id FROM agent_run WHERE prompt = %s", (f"rp-{tag}",))
            out[tag] = str(cur.fetchone()[0])
        for stag in ("s3", "s4", "s5"):
            cur.execute(
                "SELECT id FROM agent_chat_session WHERE local_thread_id = %s",
                (f"rp-{stag}",),
            )
            out[stag] = str(cur.fetchone()[0])
    return out


_MT = {"Authorization": "Bearer {mt}", "X-Runner-Id": "{roff}"}


SESSIONS = (
    Session(
        name="runner_web_lifecycle",
        domain="runner_web",
        source=(
            "runner/test_web_catalog.py (pods), test_web_machines.py, "
            "runner/test_web_runners.py, test_web_runs.py, "
            "test_web_approvals.py, test_web_chat.py"
        ),
        auth_users=("admin", "member", "api_key"),
        static_context=dict(_STATIC),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=_SETUP_KEYS,
        generated_columns={
            "runner": [
                "refresh_token_hash", "refresh_token_fingerprint",
                "previous_refresh_token_hash",
            ],
            "machine_token": ["token_hash", "token_fingerprint"],
        },
        timestamp_columns={
            "runner": ["enrolled_at", "revoked_at", "last_heartbeat_at"],
            # Evidence: machine revoke stamps revoked_at with now()
            # per backend (request 53).
            "dev_machine": ["last_seen_at", "revoked_at"],
            # Evidence: web cancel stamps cancel_requested_at with
            # now() per backend (request 40); the machine-revoke
            # cascade stamps the terminal trio the same way (53).
            "agent_run": [
                "cancel_requested_at", "ended_at",
                "terminal_capacity_released_at",
                "terminal_hooks_applied_at",
            ],
            # Evidence: rotate revokes the machine's tokens with
            # now() per backend (request 52).
            "machine_token": ["last_used_at", "revoked_at"],
            "agent_run_approval": ["requested_at"],
            # Evidence: chat-approval decide stamps decided_at with
            # now() per backend (the persisted half of the offline
            # fan-out 500, request 23).
            "agent_chat_approval": ["requested_at", "decided_at"],
            "agent_chat_session": ["last_message_at", "closed_at"],
        },
        db_cell_refs=[
            ("runner", "dev_machine_id", "dev_machine", ""),
            ("machine_token", "dev_machine_id", "dev_machine", ""),
            ("agent_chat_event", "payload", "agent_chat_approval", "approval_id"),
            # Evidence: message-accept mints a message per backend;
            # the scalar FK and the chat_timing payload nest it
            # (request 17).
            ("agent_chat_event", "message_id", "agent_chat_message", ""),
            ("agent_chat_event", "payload", "agent_chat_message", "message_id"),
        ],
        requests=(
            # -- HTTP runner creates (revoke + delete targets) --
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"project": "RPJ", "workspace_slug": "replay-ws",
                      "host_label": "rp-wvok", "name": "rp-wvok"},
                auth="api_key", expect_status=201,
                capture={"rvok_id": "runner_id", "rvok_mt": "machine_token"},
                body_blank_paths=["access_token_expires_at"],
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
            ),
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"project": "RPJ", "workspace_slug": "replay-ws",
                      "host_label": "rp-wdel", "name": "rp-wdel"},
                auth="api_key", expect_status=201,
                capture={"rdel_id": "runner_id", "rdel_mt": "machine_token"},
                body_blank_paths=["access_token_expires_at"],
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
            ),
            # -- black-box approval seeds (daemon endpoints) --
            Request(
                "POST", f"{DAEMON}/runs/{{w1}}/approvals/",
                route=f"{DAEMON}/runs/{{run_id}}/approvals/",
                json={"approval_id": "{aidw}", "kind": "command_execution",
                      "reason": "rm -rf /"},
                headers={**_MT, "Idempotency-Key": "{k1}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", f"{DAEMON}/chat/sessions/{{s4}}/approvals/",
                route=f"{DAEMON}/chat/sessions/{{session_id}}/approvals/",
                json={"local_approval_id": "la-w1", "kind": "shell",
                      "reason": "run tests"},
                headers=dict(_MT), auth="anonymous", expect_status=200,
                capture={"capid": "approval/id"},
                body_blank_paths=["approval/requested_at"],
            ),
            _prime("w0"),
            # -- pods (admin-only manage) --
            Request(
                "POST", f"{WEB}/pods/",
                route=f"{WEB}/pods/",
                json={"project": "{project}", "name": "web-contract"},
                auth="admin", expect_status=201,
                capture={"pod2": "id"},
                **_csrf("w0"),
            ),
            Request(
                "PATCH", f"{WEB}/pods/{{pod2}}/",
                route=f"{WEB}/pods/{{pod_id}}/",
                json={"description": "renamed by web"},
                auth="admin", expect_status=200,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/pods/",
                route=f"{WEB}/pods/",
                json={"project": "{project}", "name": "member-pod"},
                auth="member", expect_status=403,
                **_csrf("w0"),
            ),
            Request(
                "PATCH", f"{WEB}/pods/{{pod2}}/",
                route=f"{WEB}/pods/{{pod_id}}/",
                json={"description": "member rename"},
                auth="member", expect_status=403,
                **_csrf("w0"),
            ),
            Request(
                "DELETE", f"{WEB}/pods/{{pod2}}/",
                route=f"{WEB}/pods/{{pod_id}}/",
                auth="admin", expect_status=204,
                **_csrf("w0"),
            ),
            Request(
                "DELETE", f"{WEB}/pods/{{pod}}/",
                route=f"{WEB}/pods/{{pod_id}}/",
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "DELETE", f"{WEB}/pods/{{pod}}/",
                route=f"{WEB}/pods/{{pod_id}}/",
                auth="member", expect_status=403,
                **_csrf("w0"),
            ),
            # -- web chat --
            Request(
                "POST", f"{WEB}/chat/sessions/",
                route=f"{WEB}/chat/sessions/",
                json={}, auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/",
                route=f"{WEB}/chat/sessions/",
                json={"workspace": "{ws_id}", "runner": "{roff}"},
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/",
                route=f"{WEB}/chat/sessions/",
                json={"workspace": "{ws_id}", "runner": "{ron}"},
                auth="admin", expect_status=201,
                capture={"snew": "id"},
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s5}}/messages/",
                route=f"{WEB}/chat/sessions/{{session_id}}/messages/",
                json={}, auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s4}}/messages/",
                route=f"{WEB}/chat/sessions/{{session_id}}/messages/",
                json={"content": "hello?"},
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s5}}/messages/",
                route=f"{WEB}/chat/sessions/{{session_id}}/messages/",
                json={"content": "online hello"},
                auth="admin", expect_status=201,
                body_blank_paths=["id"],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s4}}/warm/",
                route=f"{WEB}/chat/sessions/{{session_id}}/warm/",
                json={}, auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s5}}/warm/",
                route=f"{WEB}/chat/sessions/{{session_id}}/warm/",
                json={}, auth="admin", expect_status=202,
                # PIDASHCONV-889: the warm offline-dispatch failure
                # event payload shape differs (Django {"code":
                # "dispatch_failed"} vs Rust {"message_id": null,
                # "reason": "warm_dispatch_failed"}). Exact per-side
                # pin; any change fails loudly.
                known_cell_diffs=[
                    ("agent_chat_event", "PIDASHCONV-889",
                     {"kind": "chat_warm_failed", "session_id": "{s5}"},
                     "payload",
                     {"code": "dispatch_failed"},
                     {"message_id": None, "reason": "warm_dispatch_failed"}),
                ],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s5}}/cancel/",
                route=f"{WEB}/chat/sessions/{{session_id}}/cancel/",
                json={}, auth="admin", expect_status=200,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/sessions/{{s3}}/close/",
                route=f"{WEB}/chat/sessions/{{session_id}}/close/",
                json={}, auth="admin", expect_status=500,
                skip_body=True,
                body_contains=[("<title>RunnerOfflineError", "Something went wrong")],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/approvals/{{capid}}/decide/",
                route=f"{WEB}/chat/approvals/{{approval_id}}/decide/",
                json={"decision": "maybe"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/chat/approvals/{{capid}}/decide/",
                route=f"{WEB}/chat/approvals/{{approval_id}}/decide/",
                json={"decision": "accept"},
                auth="admin", expect_status=500,
                skip_body=True,
                body_contains=[("<title>RunnerOfflineError", "Something went wrong")],
                **_csrf("w0"),
            ),
            # -- web run approvals --
            Request(
                "POST", f"{WEB}/approvals/{{aidw}}/decide/",
                route=f"{WEB}/approvals/{{approval_id}}/decide/",
                json={"decision": "maybe"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/approvals/{{aidw}}/decide/",
                route=f"{WEB}/approvals/{{approval_id}}/decide/",
                json={"decision": "accept"},
                auth="admin", expect_status=500,
                skip_body=True,
                body_contains=[("<title>NotSupportedError", "<title>NotSupportedError")],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/approvals/{{missing}}/decide/",
                route=f"{WEB}/approvals/{{approval_id}}/decide/",
                json={"decision": "accept"},
                auth="admin", expect_status=500,
                skip_body=True,
                body_contains=[("<title>NotSupportedError", "<title>NotSupportedError")],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/approvals/{{aidw}}/decide/",
                route=f"{WEB}/approvals/{{approval_id}}/decide/",
                json={"decision": "accept"},
                auth="anonymous", expect_status=401,
            ),
            # -- web runners (busy guard before the cancel below) --
            Request(
                "PATCH", f"{WEB}/{{rdel_id}}/",
                route=f"{WEB}/{{runner_id}}/",
                json={"name": "renamed-web"},
                auth="admin", expect_status=200,
                # The HTTP-created runner's dev machine id is minted
                # per backend; capturing it blanks both occurrences
                # in this response.
                capture={"rdel_machine": "dev_machine"},
                **_csrf("w0"),
            ),
            Request(
                "PATCH", f"{WEB}/{{rdel_id}}/",
                route=f"{WEB}/{{runner_id}}/",
                json={"name": "  "},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "PATCH", f"{WEB}/{{rdel_id}}/",
                route=f"{WEB}/{{runner_id}}/",
                json={"pod": "{missing}"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "PATCH", f"{WEB}/{{roff}}/",
                route=f"{WEB}/{{runner_id}}/",
                json={"pod": "{pod3}"},
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/{{rvok_id}}/revoke/",
                route=f"{WEB}/{{runner_id}}/revoke/",
                json={}, auth="admin", expect_status=200,
                # Per-backend minted machine id, both occurrences
                # (captures substitute within their own response).
                capture={"rvok_machine": "dev_machine"},
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/{{rvok_id}}/revoke/",
                route=f"{WEB}/{{runner_id}}/revoke/",
                json={}, auth="admin", expect_status=200,
                body_blank_paths=["dev_machine", "dev_machine_detail/id"],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/{{rdel_id}}/revive/",
                route=f"{WEB}/{{runner_id}}/revive/",
                json={}, auth="admin", expect_status=410,
                **_csrf("w0"),
            ),
            Request(
                "DELETE", f"{WEB}/{{rdel_id}}/",
                route=f"{WEB}/{{runner_id}}/",
                auth="admin", expect_status=204,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/invites/",
                route=f"{WEB}/invites/",
                json={}, auth="admin", expect_status=410,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/invites/",
                route=f"{WEB}/invites/",
                json={}, auth="anonymous", expect_status=401,
            ),
            # -- web runs --
            Request(
                "POST", f"{WEB}/runs/",
                route=f"{WEB}/runs/",
                json={}, auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/",
                route=f"{WEB}/runs/",
                json={"prompt": "hi", "workspace": "{ws_id}", "work_item": "nope"},
                auth="admin", expect_status=500,
                skip_body=True,
                body_contains=[("<title>ValidationError", "<title>ValidationError")],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/{{w5}}/cancel/",
                route=f"{WEB}/runs/{{run_id}}/cancel/",
                json={}, auth="admin", expect_status=200,
                body_blank_paths=["cancel_requested_at"],
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/{{w3}}/cancel/",
                route=f"{WEB}/runs/{{run_id}}/cancel/",
                json={}, auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/{{missing}}/cancel/",
                route=f"{WEB}/runs/{{run_id}}/cancel/",
                json={}, auth="admin", expect_status=404,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/{{w2}}/release-pin/",
                route=f"{WEB}/runs/{{run_id}}/release-pin/",
                json={}, auth="admin", expect_status=200,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/runs/{{w4}}/release-pin/",
                route=f"{WEB}/runs/{{run_id}}/release-pin/",
                json={}, auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/re-tick/",
                route=f"{WEB}/re-tick/",
                json={}, auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/re-tick/",
                route=f"{WEB}/re-tick/",
                json={"work_item": "nope"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/re-tick/",
                route=f"{WEB}/re-tick/",
                json={"work_item": "{nxuuid}"},
                auth="admin", expect_status=404,
                **_csrf("w0"),
            ),
            # -- web machines (revoke cascade last: R_off unused after) --
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/create-runner/",
                route=f"{WEB}/dev-machines/{{machine_id}}/create-runner/",
                json={"workspace": "{ws_id}"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/create-runner/",
                route=f"{WEB}/dev-machines/{{machine_id}}/create-runner/",
                json={"workspace": "{ws_id}", "project": "RPJ", "agent": "hal-9000"},
                auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/create-runner/",
                route=f"{WEB}/dev-machines/{{machine_id}}/create-runner/",
                json={"workspace": "{ws_id}", "project": "NOPE"},
                auth="admin", expect_status=404,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/create-runner/",
                route=f"{WEB}/dev-machines/{{machine_id}}/create-runner/",
                json={"workspace": "{ws_id}", "project": "RPJ"},
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{mspare}}/rotate/",
                route=f"{WEB}/dev-machines/{{machine_id}}/rotate/",
                json={"workspace": "{ws_id}"},
                auth="admin", expect_status=200,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/revoke/",
                route=f"{WEB}/dev-machines/{{machine_id}}/revoke/",
                json={"workspace": "{ws_id}"},
                auth="admin", expect_status=200,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{moff}}/rotate/",
                route=f"{WEB}/dev-machines/{{machine_id}}/rotate/",
                json={"workspace": "{ws_id}"},
                auth="admin", expect_status=409,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/dev-machines/{{mforeign}}/revoke/",
                route=f"{WEB}/dev-machines/{{machine_id}}/revoke/",
                json={"workspace": "{ws_id}"},
                auth="admin", expect_status=404,
                **_csrf("w0"),
            ),
            Request(
                "DELETE", f"{WEB}/dev-machines/{{mspare}}/",
                route=f"{WEB}/dev-machines/{{machine_id}}/",
                params={"workspace": "{ws_id}"},
                auth="admin", expect_status=204,
                **_csrf("w0"),
            ),
            # -- ticket mint -> redeem full circle --
            Request(
                "POST", f"{WEB}/machine-tokens/{{ws_id}}/tickets/",
                route=f"{WEB}/machine-tokens/{{workspace_id}}/tickets/",
                json={"host_label": "redeem-w"},
                auth="admin", expect_status=201,
                capture={"wticket": "ticket"},
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{DAEMON}/machine-tokens/",
                route=f"{DAEMON}/machine-tokens/",
                json={"ticket": "{wticket}"},
                auth="anonymous", expect_status=201,
                body_blank_paths=["machine_token"],
            ),
            Request(
                "POST", f"{DAEMON}/machine-tokens/",
                route=f"{DAEMON}/machine-tokens/",
                json={"ticket": "{wticket}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{WEB}/machine-tokens/{{ws_id}}/tickets/",
                route=f"{WEB}/machine-tokens/{{workspace_id}}/tickets/",
                json={}, auth="admin", expect_status=400,
                **_csrf("w0"),
            ),
            Request(
                "POST", f"{WEB}/machine-tokens/{{other_ws_id}}/tickets/",
                route=f"{WEB}/machine-tokens/{{workspace_id}}/tickets/",
                json={"host_label": "redeem-w"},
                auth="admin", expect_status=404,
                **_csrf("w0"),
            ),
        ),
    ),
)
