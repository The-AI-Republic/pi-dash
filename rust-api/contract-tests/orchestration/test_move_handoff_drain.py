"""Project-move handoff must drain the target pod (PIDASHCONV-1219).

An issue with an *executing* local-runner run is moved to another project
whose pod has an online idle runner. The move parks the source run in
``cancel_requested`` with a project-move handoff marker and waits for the
source runner to ack (here: the runner is revoked). Completing that handoff
creates the replacement QUEUED run in the target project and dispatches it;
a non-cloud dispatch returns ``DispatchDecision::PodDrain { pod_id }`` and
the caller must drain that pod so the QUEUED replacement is matched to the
target pod's idle runner. Django drains inline
(``cloud_agent/dispatch.py:61-70``: ``drain_pod_by_id(run.pod_id)``); the Rust
driver dropped the decision, so the replacement stayed QUEUED.

Drives only the app move endpoint and the web revoke endpoint; asserts the
replacement row lands ``assigned`` on the target runner. Seeds its own world
straight into Postgres (unique slugs), no cleanup pass needed.
"""

from __future__ import annotations

import secrets
from datetime import datetime, timedelta, timezone

from _harness import db, env, world
from _harness.auth import authed_client, forge_session_key, get_secret_key
from _harness.db import get_database_url

MOVE_PATH = "/api/workspaces/{slug}/projects/{project}/work-items/{issue}/move/"
REVOKE_PATH = "/api/runners/{runner}/revoke/"
HANDOFF_KEY = "_project_move_handoff"


def _exec(sql, params=()):
    db.execute(get_database_url(), sql, params)


def _fetchone(sql, params=()):
    return db.fetchone(get_database_url(), sql, params)


def _make_runner(owner_id, workspace_id, pod_id, *, status, heartbeat_secs_ago=0):
    """A private manual runner row (``visibility=0`` is what the matcher wants)."""
    runner_id = world.uid()
    heartbeat = datetime.now(timezone.utc) - timedelta(seconds=heartbeat_secs_ago)
    _exec(
        """INSERT INTO runner
           (id, name, capabilities, status, os, arch, runner_version,
            protocol_version, last_heartbeat_at, created_at, updated_at,
            owner_id, workspace_id, pod_id, host_label,
            refresh_token_hash, refresh_token_fingerprint,
            refresh_token_generation, previous_refresh_token_hash,
            access_token_signing_key_version, enrollment_token_hash,
            enrollment_token_fingerprint, revoked_reason, visibility,
            dev_metadata, provisioning)
           VALUES (%s,%s,'[]',%s,'','','',1,%s,now(),now(),%s,%s,%s,'',
                   '','',0,'',1,'','','',0,'{}','manual')""",
        (runner_id, f"runner-{secrets.token_hex(4)}", status, heartbeat,
         owner_id, workspace_id, pod_id),
    )
    return runner_id


def _session_client(db_conn, w):
    password_field = _fetchone(
        "SELECT password FROM users WHERE id=%s", (w.user_id,)
    )["password"]
    key = forge_session_key(
        db_conn, user_id=w.user_id, password_field=password_field,
        secret=get_secret_key(),
    )
    return authed_client(env.BASE_URL, key)


def _seed_world():
    w = world.build("hd1219")
    tgt_proj, tgt_ident = world.make_project(
        w.workspace_id, w.user_id, "T" + secrets.token_hex(2).upper()
    )
    world.add_project_member(tgt_proj, w.workspace_id, w.user_id)
    tgt_state = world.make_state(
        tgt_proj, w.workspace_id, "Backlog", "backlog", default=True, sequence=1.0
    )
    _exec("UPDATE projects SET default_state_id=%s WHERE id=%s", (tgt_state, tgt_proj))
    src_pod = world.make_pod(w.workspace_id, w.project_id, is_default=True)
    tgt_pod = world.make_pod(w.workspace_id, tgt_proj, is_default=True)
    issue = world.make_issue(w, "hd", "Backlog")
    src_runner = _make_runner(
        w.user_id, w.workspace_id, src_pod, status="running", heartbeat_secs_ago=0
    )
    tgt_runner = _make_runner(
        w.user_id, w.workspace_id, tgt_pod, status="online", heartbeat_secs_ago=0
    )
    src_run = world.make_run(
        issue, w.workspace_id, w.user_id, src_pod,
        status="running", runner_id=src_runner,
    )
    return {
        "w": w, "tgt_proj": tgt_proj, "tgt_ident": tgt_ident,
        "src_pod": src_pod, "tgt_pod": tgt_pod, "issue": issue,
        "src_runner": src_runner, "tgt_runner": tgt_runner, "src_run": src_run,
    }


def test_move_handoff_drains_target_pod(db_conn):
    seeded = _seed_world()
    w = seeded["w"]
    client = _session_client(db_conn, w)
    try:
        moved = client.post(
            MOVE_PATH.format(slug=w.workspace_slug, project=w.project_id, issue=seeded["issue"]),
            json={"project": seeded["tgt_ident"]},
        )
        assert moved.status_code == 200, moved.text

        parked = _fetchone(
            "SELECT status, run_config FROM agent_run WHERE id=%s", (seeded["src_run"],)
        )
        assert parked["status"] == "cancel_requested", parked
        assert seeded["tgt_pod"] == str(
            (parked["run_config"] or {}).get(HANDOFF_KEY, {}).get("target_pod_id")
        ), parked["run_config"]

        revoked = client.post(REVOKE_PATH.format(runner=seeded["src_runner"]))
        assert revoked.status_code == 200, revoked.text

        replacement = _fetchone(
            "SELECT id, status, runner_id, pod_id FROM agent_run"
            " WHERE work_item_id=%s AND parent_run_id=%s",
            (seeded["issue"], seeded["src_run"]),
        )
        assert replacement is not None, "no replacement run created by the handoff"
        assert str(replacement["pod_id"]) == seeded["tgt_pod"], replacement
        assert replacement["status"] == "assigned", replacement
        assert str(replacement["runner_id"]) == seeded["tgt_runner"], replacement
    finally:
        client.close()
