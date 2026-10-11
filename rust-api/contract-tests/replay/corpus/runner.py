# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: runner daemon write flows (PIDASHCONV-823).

Built from the ``runner`` contract suite's daemon surface:
test_daemon_enroll.py (enroll/single-use/runner-create/refresh rotation
+ replay detection/self-revoke/machine-token redeem), test_daemon_runs.py
(accept/queued/started/events/approvals/awaiting-reauth/complete/pause/
fail/refusal/cancelled/resumed/stream-upgrade, dedupe, 404/403/anon
gates), test_daemon_chat.py (started/message/events/approvals/failed/
closed, idempotency gates, foreign/anon gates incl. the 500 bug),
test_daemon_sessions.py (runner + machine session open/poll/delete,
protocol/project/cross/auth gates, command results), test_daemon_
projects.py (desktop-enroll 401/403 gates) and test_enroll_gating.py R2
(non-object ticket blob 500).

Seed strategy: runners cannot be created mid-session (the seed runs
before the requests), so the offline runner, its dev machine, and its
``mt_`` token are seeded by SQL, mirroring an HTTP-created row
(``enrolled_at`` set, status offline, machine bound — verified against
a live ``POST runners/`` row). The ``mt_`` raw is session-static, so
both backends hash the same bearer. ``POST runners/`` itself is
covered by a second create inside the flow. Enroll targets seed with
known enrollment raws (static context).

Framework 500s (anon chat ``AttributeError``, non-object ticket
``TypeError``) use ``skip_body``: Django renders its DEBUG HTML page
(host/ports/settings/memory addresses — environment, not behaviour)
while Rust answers its generic JSON 500; status, exception-kind
presence pins, DB, jobs, and sinks still compare. The contract suite
pins status-only on exactly these endpoints.
"""

from __future__ import annotations

import os

from ..session import Request, Session
from ._csrf import csrf as _csrf
from ._csrf import prime as _prime

DAEMON = "/api/v1/runner"
_MISSING = "00000000-0000-0000-0000-000000000000"

MT_RAW = "mt_rp823daemonoff0000000000000000000001"
ENROLL1 = "apd_en_rp823daemon1000000000000001"
ENROLL2 = "apd_en_rp823daemon2000000000000002"
AID1 = "11111111-2222-3333-4444-555555555555"
R2_TICKET = "rp823r2tick00000000000000000000001"

# Distinct static idempotency keys (same bytes both sides, so the
# dedupe rows match; K_DUP is sent twice to prove the duplicate leg).
def _keys():
    out = {}
    for n in range(1, 21):
        out[f"k{n}"] = f"{n:02d}" * 16
    out["kdup"] = "dd" * 16
    return out

_STATIC = {
    "mt": MT_RAW,
    "enroll1": ENROLL1,
    "enroll2": ENROLL2,
    "aid1": AID1,
    "r2ticket": R2_TICKET,
    "missing": _MISSING,
    **_keys(),
}

_RUN_TAGS = ("ra", "rb", "rc", "rd", "re", "rf", "rg", "rh", "ri", "rj", "rk", "rl")

_SETUP_KEYS = (
    "pod", "roff", "moff", "mspare", "e1", "e2",
    *_RUN_TAGS, "runf", "s0", "s1", "s2", "m1", "chatf", "r2primed",
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
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-other'")
        other_ws = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM projects WHERE identifier = 'RPX'")
        other_project = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()[0])
        cur.execute("SELECT id FROM users WHERE email = 'replay-outsider@example.com'")
        outsider_id = str(cur.fetchone()[0])

    pod = seeder.create_pod(ws_id, project_id, name="rp-daemon", is_default=True)
    pod_f = seeder.create_pod(other_ws, other_project, name="rp-foreign", is_default=True)

    # Offline runner mirroring a POST-runners/ row (enrolled, machine
    # bound, offline); the known mt_ raw authenticates every daemon
    # request in the flow.
    roff = seeder.enroll_runner(
        admin_id, ws_id, pod["id"], "apd_en_rp823roff0000000000000000", name="rp-roff",
    )
    moff = seeder.create_dev_machine(admin_id, host_label="rp-moff")
    seeder.create_machine_token(
        admin_id, ws_id, MT_RAW, host_label="rp-moff", dev_machine_id=moff["id"],
    )
    with conn.cursor() as cur:
        cur.execute(
            "UPDATE runner SET enrolled_at = now(), status = 'offline',"
            " host_label = 'rp-roff', dev_machine_id = %s WHERE id = %s",
            (moff["id"], roff["id"]),
        )
    seeder.create_dev_machine(admin_id, host_label="rp-mspare")
    e1 = seeder.enroll_runner(admin_id, ws_id, pod["id"], ENROLL1, name="rp-e1")
    e2 = seeder.enroll_runner(admin_id, ws_id, pod["id"], ENROLL2, name="rp-e2")
    rfor = seeder.enroll_runner(
        outsider_id, other_ws, pod_f["id"],
        "apd_en_rp823rfor0000000000000000", name="rp-rfor",
    )

    with conn.cursor() as cur:
        for tag in _RUN_TAGS:
            run = seeder.create_daemon_run(admin_id, ws_id, pod["id"], roff["id"])
            cur.execute(
                "UPDATE agent_run SET prompt = %s WHERE id = %s",
                (f"rp-{tag}", run["id"]),
            )
        runf = seeder.create_daemon_run(
            outsider_id, other_ws, pod_f["id"], rfor["id"]
        )
        cur.execute(
            "UPDATE agent_run SET prompt = 'rp-runf' WHERE id = %s", (runf["id"],)
        )
        chats = {}
        for stag in ("s0", "s1", "s2"):
            sess = seeder.create_chat_session(ws_id, roff["id"], pod["id"], admin_id)
            cur.execute(
                "UPDATE agent_chat_session SET local_thread_id = %s WHERE id = %s",
                (f"rp-{stag}", sess["id"]),
            )
            chats[stag] = sess["id"]
        seeder.create_chat_message(chats["s1"], role="user", content="hello")
        chatf = seeder.create_chat_session(
            other_ws, rfor["id"], pod_f["id"], outsider_id
        )
        cur.execute(
            "UPDATE agent_chat_session SET local_thread_id = 'rp-chatf' WHERE id = %s",
            (chatf["id"],),
        )


def _setup(world, config) -> dict:
    import psycopg
    import redis as redis_lib

    out = {}
    with psycopg.connect(config.django_db) as conn, conn.cursor() as cur:
        cur.execute("SELECT id FROM pod WHERE name = 'rp-daemon'")
        out["pod"] = str(cur.fetchone()[0])
        for name, key in (("rp-roff", "roff"), ("rp-e1", "e1"), ("rp-e2", "e2")):
            cur.execute("SELECT id FROM runner WHERE name = %s", (name,))
            out[key] = str(cur.fetchone()[0])
        for label, key in (("rp-moff", "moff"), ("rp-mspare", "mspare")):
            cur.execute("SELECT id FROM dev_machine WHERE host_label = %s", (label,))
            out[key] = str(cur.fetchone()[0])
        for tag in (*_RUN_TAGS, "runf"):
            cur.execute("SELECT id FROM agent_run WHERE prompt = %s", (f"rp-{tag}",))
            out[tag] = str(cur.fetchone()[0])
        for stag in ("s0", "s1", "s2", "chatf"):
            cur.execute(
                "SELECT id FROM agent_chat_session WHERE local_thread_id = %s",
                (f"rp-{stag}",),
            )
            out[stag] = str(cur.fetchone()[0])
        cur.execute(
            "SELECT id FROM agent_chat_message WHERE session_id = %s AND seq = 1",
            (out["s1"],),
        )
        out["m1"] = str(cur.fetchone()[0])
    # R2: the non-object ticket blob both backends must 500 on.
    for redis_url in (config.django_redis_url, config.rust_redis_url):
        client = redis_lib.Redis.from_url(redis_url, decode_responses=True)
        try:
            client.setex(f"machine_token_ticket:{R2_TICKET}", 120, "[1,2]")
        finally:
            client.close()
    out["r2primed"] = "yes"
    return out


# Bearer mt_ + X-Runner-Id (installed-daemon wire format).
_MT = {"Authorization": "Bearer {mt}", "X-Runner-Id": "{roff}"}
# Bearer mt_ alone (machine URLs carry their own identity).
_BMT = {"Authorization": "Bearer {mt}"}


def _run(run_id: str, leaf: str) -> str:
    return f"{DAEMON}/runs/{run_id}/{leaf}/"


def _chat(sid: str, leaf: str) -> str:
    return f"{DAEMON}/chat/sessions/{sid}/{leaf}/"


SESSIONS = (
    Session(
        name="runner_daemon_lifecycle",
        domain="runner",
        source=(
            "runner/test_daemon_enroll.py (enroll, runner-create, refresh, "
            "self-revoke, redeem), test_daemon_runs.py, test_daemon_chat.py, "
            "test_daemon_sessions.py, test_daemon_projects.py "
            "(desktop-enroll gates), test_enroll_gating.py R2"
        ),
        auth_users=("admin", "api_key"),
        static_context=dict(_STATIC),
        seed=_seed_extras,
        setup=_setup,
        setup_keys=_SETUP_KEYS,
        generated_columns={
            # Enroll/refresh/create mint random tokens per backend.
            "runner": [
                "refresh_token_hash", "refresh_token_fingerprint",
                "previous_refresh_token_hash",
            ],
            "machine_token": ["token_hash", "token_fingerprint"],
        },
        timestamp_columns={
            # Evidence: enroll stamps enrolled_at/last_seen_at with
            # now() per backend (request 2); replayed-refresh revoke
            # stamps revoked_at the same way (request 11); session
            # open stamps the runner heartbeat the same way (52).
            "runner": ["enrolled_at", "revoked_at", "last_heartbeat_at"],
            "dev_machine": ["last_seen_at"],
            # Evidence: session open stamps the session row with
            # now() per backend (request 52); the evicting second
            # open stamps the first row's revoked_at (request 53).
            "runner_session": ["last_seen_at", "revoked_at"],
            # Evidence: machine session open stamps the session row
            # with now() per backend (request 62); delete stamps
            # revoked_at the same way (request 63).
            "machine_session": ["last_seen_at", "revoked_at"],
            # Evidence: mt_ auth stamps last_used_at on every
            # authenticated request (request 16).
            "machine_token": ["last_used_at"],
            # Evidence: the complete transition stamps ended_at +
            # terminal_*_at with now() per backend (request 19).
            "agent_run": [
                "ended_at", "terminal_capacity_released_at",
                "terminal_hooks_applied_at",
                # Evidence: the started transition stamps started_at
                # with now() per backend (request 29).
                "started_at",
            ],
            # Evidence: approval creation stamps requested_at with
            # now() per backend (request 31).
            "agent_run_approval": ["requested_at"],
            # Evidence: message-complete stamps last_message_at with
            # now() per backend (request 42); close stamps closed_at
            # the same way (request 48).
            "agent_chat_session": ["last_message_at", "closed_at"],
            # Evidence: chat-approval creation stamps requested_at
            # with now() per backend (request 45).
            "agent_chat_approval": ["requested_at"],
        },
        db_cell_refs=[
            # Enroll/create mint a dev machine per backend; both sides'
            # FKs must exist as dev_machine ids in their own database.
            ("runner", "dev_machine_id", "dev_machine", ""),
            ("machine_token", "dev_machine_id", "dev_machine", ""),
            # Evidence: message-complete mints an assistant message
            # per backend; the event's FK must exist as an
            # agent_chat_message id in its own database (request 40).
            ("agent_chat_event", "message_id", "agent_chat_message", ""),
            # Evidence: the approval event's payload nests the minted
            # approval id (request 45).
            ("agent_chat_event", "payload", "agent_chat_approval", "approval_id"),
        ],
        requests=(
            # -- enroll / create / redeem --
            Request(
                "POST", f"{DAEMON}/runners/enroll/",
                route=f"{DAEMON}/runners/enroll/",
                json={}, auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", f"{DAEMON}/runners/enroll/",
                route=f"{DAEMON}/runners/enroll/",
                json={"enrollment_token": "apd_en_doesnotexist000000", "host_label": "h"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{DAEMON}/runners/enroll/",
                route=f"{DAEMON}/runners/enroll/",
                json={"enrollment_token": "{enroll1}", "host_label": "rp-h1"},
                auth="anonymous", expect_status=201,
                capture={"e1_access": "access_token", "e1_refresh": "refresh_token"},
                body_blank_paths=["access_token_expires_at", "machine_token"],
            ),
            Request(
                "POST", f"{DAEMON}/runners/enroll/",
                route=f"{DAEMON}/runners/enroll/",
                json={"enrollment_token": "{enroll1}", "host_label": "rp-h1b"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{DAEMON}/runners/enroll/",
                route=f"{DAEMON}/runners/enroll/",
                json={"enrollment_token": "{enroll2}", "host_label": "rp-h2"},
                auth="anonymous", expect_status=201,
                capture={"e2_refresh": "refresh_token"},
                body_blank_paths=[
                    "access_token", "access_token_expires_at", "machine_token",
                ],
            ),
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"project": "RPJ", "workspace_slug": "replay-ws",
                      "host_label": "rp-rcov", "name": "rp-rcov"},
                auth="api_key", expect_status=201,
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
                capture={"rcov_id": "runner_id", "rcov_mt": "machine_token"},
                body_blank_paths=["access_token_expires_at"],
            ),
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"workspace_slug": "replay-ws"},
                auth="api_key", expect_status=400,
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
            ),
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"project": "NOPE", "workspace_slug": "replay-ws"},
                auth="api_key", expect_status=404,
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
            ),
            Request(
                "POST", f"{DAEMON}/runners/",
                route=f"{DAEMON}/runners/",
                json={"project": "RPJ", "workspace_slug": "replay-ws",
                      "name": "not a valid name!"},
                auth="api_key", expect_status=400,
                known_absent_jobs={"pi_dash.bgtasks.logger_task.process_logs": "PIDASHCONV-868"},
            ),
            # -- refresh rotation (E1) + replay detection (E2) --
            Request(
                "POST", f"{DAEMON}/runners/{{e1}}/refresh/",
                route=f"{DAEMON}/runners/{{runner_id}}/refresh/",
                headers={"Authorization": "Bearer {e1_refresh}"},
                auth="anonymous", expect_status=200,
                capture={"e1_access2": "access_token", "e1_refresh2": "refresh_token"},
                body_blank_paths=["access_token_expires_at"],
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{e2}}/refresh/",
                route=f"{DAEMON}/runners/{{runner_id}}/refresh/",
                headers={"Authorization": "Bearer {e2_refresh}"},
                auth="anonymous", expect_status=200,
                body_blank_paths=[
                    "access_token", "refresh_token", "access_token_expires_at",
                ],
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{e2}}/refresh/",
                route=f"{DAEMON}/runners/{{runner_id}}/refresh/",
                headers={"Authorization": "Bearer {e2_refresh}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{e1}}/refresh/",
                route=f"{DAEMON}/runners/{{runner_id}}/refresh/",
                headers={"Authorization": "Bearer rt_bogus000000000000000000"},
                auth="anonymous", expect_status=401,
            ),
            # -- machine-token redeem gates --
            Request(
                "POST", f"{DAEMON}/machine-tokens/",
                route=f"{DAEMON}/machine-tokens/",
                json={}, auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", f"{DAEMON}/machine-tokens/",
                route=f"{DAEMON}/machine-tokens/",
                json={"ticket": "nope"}, auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{DAEMON}/machine-tokens/",
                route=f"{DAEMON}/machine-tokens/",
                json={"ticket": "{r2ticket}"}, auth="anonymous", expect_status=500,
                skip_body=True,
                body_contains=[("<title>TypeError", "Something went wrong")],
            ),
            # -- run terminals --
            Request(
                "POST", _run("{ra}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{ra}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers={**_MT, "Idempotency-Key": "{kdup}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{ra}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers={**_MT, "Idempotency-Key": "{kdup}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{ra}", "complete"),
                route=f"{DAEMON}/runs/{{run_id}}/complete/",
                json={"done_payload": {"summary": "done"}},
                headers={**_MT, "Idempotency-Key": "{k1}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rb}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rb}", "pause"),
                route=f"{DAEMON}/runs/{{run_id}}/pause/",
                json={"payload": {"question": "proceed?"}},
                headers={**_MT, "Idempotency-Key": "{k2}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rc}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rc}", "fail"),
                route=f"{DAEMON}/runs/{{run_id}}/fail/",
                json={"detail": "boom"},
                headers={**_MT, "Idempotency-Key": "{k3}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rd}", "fail"),
                route=f"{DAEMON}/runs/{{run_id}}/fail/",
                json={"reason": "refusal", "category": "cyber", "detail": "declined"},
                headers={**_MT, "Idempotency-Key": "{k4}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{re}", "cancelled"),
                route=f"{DAEMON}/runs/{{run_id}}/cancelled/",
                json={}, headers={**_MT, "Idempotency-Key": "{k5}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rf}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rf}", "resumed"),
                route=f"{DAEMON}/runs/{{run_id}}/resumed/",
                json={}, headers={**_MT, "Idempotency-Key": "{k6}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rg}", "queued"),
                route=f"{DAEMON}/runs/{{run_id}}/queued/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rh}", "started"),
                route=f"{DAEMON}/runs/{{run_id}}/started/",
                json={"thread_id": "thr-1", "model": "test-model"},
                headers={**_MT, "Idempotency-Key": "{k7}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{ri}", "events"),
                route=f"{DAEMON}/runs/{{run_id}}/events/",
                json={"events": [{"seq": 1, "kind": "log", "payload": {"text": "hi"}}]},
                headers={**_MT, "Idempotency-Key": "{k8}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rj}", "approvals"),
                route=f"{DAEMON}/runs/{{run_id}}/approvals/",
                json={"approval_id": "{aid1}", "kind": "command_execution",
                      "reason": "rm -rf /"},
                headers={**_MT, "Idempotency-Key": "{k9}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rk}", "awaiting-reauth"),
                route=f"{DAEMON}/runs/{{run_id}}/awaiting-reauth/",
                json={}, headers={**_MT, "Idempotency-Key": "{k10}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _run("{rl}", "stream/upgrade"),
                route=f"{DAEMON}/runs/{{run_id}}/stream/upgrade/",
                json={"stream": "events"}, headers=dict(_MT),
                auth="anonymous", expect_status=200,
                body_blank_paths=["ticket"],
            ),
            Request(
                "POST", f"{DAEMON}/runs/{{rl}}/stream/upgrade/",
                route=f"{DAEMON}/runs/{{run_id}}/stream/upgrade/",
                json={"stream": "video"}, headers=dict(_MT),
                auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", _run("{missing}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=404,
            ),
            Request(
                "POST", _run("{runf}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=403,
            ),
            Request(
                "POST", _run("{ra}", "accept"),
                route=f"{DAEMON}/runs/{{run_id}}/accept/",
                json={}, auth="anonymous", expect_status=403,
            ),
            # -- daemon chat --
            Request(
                "POST", _chat("{s1}", "started"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/started/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", _chat("{s1}", "started"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/started/",
                json={"local_thread_id": "lt-1", "agent_kind": "test"},
                headers={**_MT, "Idempotency-Key": "{k11}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", f"{DAEMON}/chat/sessions/{{s1}}/messages/{{m1}}/started/",
                route=f"{DAEMON}/chat/sessions/{{session_id}}/messages/{{message_id}}/started/",
                json={"turn_id": "turn-1"}, headers=dict(_MT),
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", f"{DAEMON}/chat/sessions/{{s1}}/messages/{{m1}}/started/",
                route=f"{DAEMON}/chat/sessions/{{session_id}}/messages/{{message_id}}/started/",
                json={"turn_id": "turn-1"}, headers=dict(_MT),
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", f"{DAEMON}/chat/sessions/{{s1}}/messages/{{m1}}/complete/",
                route=f"{DAEMON}/chat/sessions/{{session_id}}/messages/{{message_id}}/complete/",
                json={"assistant_message": "done", "status": "completed"},
                headers=dict(_MT), auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _chat("{s1}", "events"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/events/",
                json={"kind": "raw", "payload": {"text": "ping"}},
                headers={**_MT, "Idempotency-Key": "{k12}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _chat("{s1}", "events"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/events/",
                json={"kind": "raw", "payload": {}},
                headers=dict(_MT), auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", _chat("{s1}", "approvals"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/approvals/",
                json={"local_approval_id": "la-1", "kind": "shell",
                      "reason": "run tests"},
                headers=dict(_MT), auth="anonymous", expect_status=200,
                # The approval id is minted and requested_at stamped
                # per backend (both compare in the DB half instead).
                body_blank_paths=["approval/id", "approval/requested_at"],
            ),
            Request(
                "POST", _chat("{s1}", "approvals"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/approvals/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", _chat("{s1}", "failed"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/failed/",
                json={"code": "boom", "detail": "agent crashed"},
                headers={**_MT, "Idempotency-Key": "{k13}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _chat("{s2}", "closed"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/closed/",
                json={"reason": "user_done"},
                headers={**_MT, "Idempotency-Key": "{k14}"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", _chat("{missing}", "started"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/started/",
                json={}, headers={**_MT, "Idempotency-Key": "{k15}"},
                auth="anonymous", expect_status=404,
            ),
            Request(
                "POST", _chat("{chatf}", "started"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/started/",
                json={}, headers={**_MT, "Idempotency-Key": "{k16}"},
                auth="anonymous", expect_status=403,
            ),
            Request(
                "POST", _chat("{s0}", "started"),
                route=f"{DAEMON}/chat/sessions/{{session_id}}/started/",
                json={}, headers={"Idempotency-Key": "{k17}"},
                auth="anonymous", expect_status=500,
                skip_body=True,
                body_contains=[("<title>AttributeError", "Something went wrong")],
            ),
            # -- runner sessions --
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=201,
                capture={"sess1": "session_id"},
                # server_time is minted per open (the redelivered
                # entries above it still compare exactly).
                body_blank_paths=["welcome/server_time"],
            ),
            # NOTE: no first-poll 200 leg. Django's poll views share
            # one module-global async Redis client; under the dev
            # server each request runs on a fresh event loop, so a
            # poll nondeterministically answers 500 RuntimeError
            # ("Event loop is closed") instead of 200 — the suite's
            # _post_poll retries around it, but replay cannot retry
            # without masking real 500 divergences. The 200 envelope
            # stays suite-pinned (test_daemon_runner_session_poll_shape
            # and the machine lifecycle); replay covers the poll pair
            # through its deterministic gating legs below.
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=201,
                capture={"sess2": "session_id"},
                body_blank_paths=["welcome/server_time"],
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/{{sess1}}/poll",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/{{session_id}}/poll",
                json={}, headers=dict(_MT), auth="anonymous", expect_status=409,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/{{sess2}}/poll",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/{{session_id}}/poll",
                raw_body=b"{not json",
                headers={**_MT, "Content-Type": "application/json"},
                auth="anonymous", expect_status=400,
            ),
            Request(
                "DELETE", f"{DAEMON}/runners/{{roff}}/sessions/{{sess2}}/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/{{session_id}}/",
                headers=dict(_MT), auth="anonymous", expect_status=204,
            ),
            Request(
                "DELETE", f"{DAEMON}/runners/{{roff}}/sessions/{{missing}}/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/{{session_id}}/",
                headers=dict(_MT), auth="anonymous", expect_status=204,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={}, headers={**_MT, "X-Runner-Protocol-Version": "1"},
                auth="anonymous", expect_status=426,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={"project_slug": "some-other-project"},
                headers=dict(_MT), auth="anonymous", expect_status=409,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={}, headers={"Authorization": "Bearer {e1_access2}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", f"{DAEMON}/runners/{{roff}}/sessions/",
                route=f"{DAEMON}/runners/{{runner_id}}/sessions/",
                json={}, auth="anonymous", expect_status=403,
            ),
            # -- machine sessions --
            Request(
                "POST", f"{DAEMON}/dev-machines/{{moff}}/sessions/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/",
                json={}, headers=dict(_BMT), auth="anonymous", expect_status=201,
                capture={"msess1": "session_id"},
                body_blank_paths=["welcome/server_time"],
            ),
            # NOTE: no machine first-poll 200 leg (same async-Redis
            # flake as the runner poll: the suite's
            # test_daemon_machine_session_lifecycle pins the envelope
            # with retries; replay covers the pair through the
            # deterministic evicted-409 leg below).
            Request(
                "POST", f"{DAEMON}/dev-machines/{{moff}}/sessions/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/",
                json={}, headers=dict(_BMT), auth="anonymous", expect_status=201,
                capture={"msess2": "session_id"},
                body_blank_paths=["welcome/server_time"],
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/{{moff}}/sessions/{{msess1}}/poll",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/{{session_id}}/poll",
                json={}, headers=dict(_BMT), auth="anonymous", expect_status=409,
            ),
            Request(
                "DELETE", f"{DAEMON}/dev-machines/{{moff}}/sessions/{{msess1}}/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/{{session_id}}/",
                headers=dict(_BMT), auth="anonymous", expect_status=204,
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/{{mspare}}/sessions/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/",
                json={}, headers=dict(_BMT), auth="anonymous", expect_status=403,
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/{{moff}}/sessions/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/sessions/",
                json={}, auth="anonymous", expect_status=403,
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/{{moff}}/commands/{{missing}}/result/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/commands/{{request_id}}/result/",
                json={"status": "ok"}, headers=dict(_BMT),
                auth="anonymous", expect_status=404,
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/{{mspare}}/commands/{{missing}}/result/",
                route=f"{DAEMON}/dev-machines/{{machine_id}}/commands/{{request_id}}/result/",
                json={"status": "ok"}, headers=dict(_BMT),
                auth="anonymous", expect_status=403,
            ),
            # -- self-revoke + desktop-enroll gates --
            Request(
                "DELETE", f"{DAEMON}/runners/{{e2}}/",
                route=f"{DAEMON}/runners/{{runner_id}}/",
                headers={"Authorization": "Bearer {e1_access2}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "DELETE", f"{DAEMON}/runners/{{e1}}/",
                route=f"{DAEMON}/runners/{{runner_id}}/",
                headers={"Authorization": "Bearer {e1_access2}"},
                auth="anonymous", expect_status=204,
            ),
            Request(
                "POST", f"{DAEMON}/dev-machines/desktop-enroll/",
                route=f"{DAEMON}/dev-machines/desktop-enroll/",
                json={"workspace_slug": "ws", "host_label": "h"},
                auth="anonymous", expect_status=401,
            ),
            _prime("rd0"),
            Request(
                "POST", f"{DAEMON}/dev-machines/desktop-enroll/",
                route=f"{DAEMON}/dev-machines/desktop-enroll/",
                json={"workspace_slug": "replay-ws", "host_label": "my-macbook",
                      "app_version": "9.9.9"},
                auth="admin", expect_status=403,
                **_csrf("rd0"),
            ),
            Request(
                "DELETE", f"{DAEMON}/dev-machines/desktop-enroll/",
                route=f"{DAEMON}/dev-machines/desktop-enroll/",
                auth="anonymous", expect_status=401,
            ),
        ),
    ),
)
