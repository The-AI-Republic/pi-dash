"""Fixtures: the dedicated-runner world seeded straight into Postgres.

World (slugs prefixed ``dr-`` so teardown only touches our rows):

- user ``admin`` (workspace ADMIN), workspace ``dr-ws``
- project ``DR`` with a Backlog (default) state
- one default pod, one online runner (admin-owned), one offline runner
  (admin-owned), one runner owned by a foreign user (E1 probe)
- issue ``I1`` dedicated to the online runner, issue ``I2`` plain
- a QUEUED dedicated-pinned run on I1 (release-pin probe)

Sessions are forged Django sessions (``_harness.auth``), keyed by the
server's SECRET_KEY. Runner rows go through ``Seeder.enroll_runner``
so server-side token hashes are always right.
"""

from __future__ import annotations

import base64
import hashlib
import uuid

import pytest

from _harness.auth import authed_client, forge_session_key, get_secret_key
from _harness.config import get_settings
from _harness.db import LazyDatabase
from _harness.seed import Seeder, SeedTracker


def _tag() -> str:
    return f"dr-{uuid.uuid4().hex[:6]}"

PW_SALT = "dedicatedrun"
_PW_DK = hashlib.pbkdf2_hmac("sha256", b"dedicated-runner-pass", PW_SALT.encode(), 10000)
PASSWORD_FIELD = "pbkdf2_sha256$10000$%s$%s" % (PW_SALT, base64.b64encode(_PW_DK).decode())


@pytest.fixture(scope="session")
def settings():
    return get_settings()


@pytest.fixture()
def seeder(settings):
    from _harness.db import LazyDatabase as Database

    tracker = SeedTracker(Database(settings.database_url))
    yield Seeder(tracker.db, tracker, _tag(), secret_key=get_secret_key())
    tracker.cleanup()


def _uid() -> str:
    return str(uuid.uuid4())


def make_user(seeder: Seeder, email: str, username: str) -> dict:
    user_id = _uid()
    seeder.db.execute(
        """INSERT INTO users
           (id, password, email, username, display_name, first_name, last_name,
            avatar, date_joined, created_at, updated_at, last_location,
            created_location, is_superuser, is_managed, is_password_expired,
            is_active, is_staff, is_email_verified, is_password_autoset,
            is_password_reset_required, token, last_active, last_login_ip,
            last_logout_ip, last_login_medium, last_login_uagent, is_bot,
            user_timezone, is_email_valid)
           VALUES (%s,%s,%s,%s,'','','','',now(),now(),now(),'','',
                   false,false,false,true,false,false,false,false,'',now(),'','',
                   'email','',false,'UTC',true)""",
        (user_id, PASSWORD_FIELD, email, username),
    )
    seeder.track.add("users", user_id)
    return {"id": user_id, "email": email}


@pytest.fixture()
def world(seeder):
    admin = make_user(seeder, f"dr-admin-{seeder.tag}@example.com", f"dr_admin_{seeder.tag}")
    foreign = make_user(seeder, f"dr-foreign-{seeder.tag}@example.com", f"dr_foreign_{seeder.tag}")
    ws = seeder.create_workspace(admin["id"], name="dr-ws")
    seeder.create_workspace_member(ws["id"], admin["id"], role=20)
    project = seeder.create_project(ws["id"], name="DR")
    seeder.create_project_member(ws["id"], project["id"], admin["id"], role=20)
    backlog = seeder.create_state(ws["id"], project["id"], name="Backlog", group="backlog")
    pod = seeder.create_pod(ws["id"], project["id"], name="dr-pod", is_default=True)
    issue1 = seeder.create_issue(ws["id"], project["id"], backlog["id"], name="dedicated")
    issue2 = seeder.create_issue(ws["id"], project["id"], backlog["id"], name="plain")
    # E7: the runner's owner must be on the issue (creator here).
    for issue in (issue1, issue2):
        seeder.db.execute(
            "UPDATE issues SET created_by_id=%s WHERE id=%s",
            (admin["id"], issue["id"]),
        )
    return {
        "admin": admin,
        "foreign": foreign,
        "ws": ws,
        "project": project,
        "backlog": backlog,
        "pod": pod,
        "issue1": issue1,
        "issue2": issue2,
    }


def _online(seeder: Seeder, runner_id: str) -> None:
    """Flip an enrolled (offline) runner online+fresh (Django transient)."""
    seeder.db.execute(
        "UPDATE runner SET status='online', last_heartbeat_at=now() WHERE id=%s",
        (runner_id,),
    )


@pytest.fixture()
def online_runner(seeder, world):
    runner = seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], world["pod"]["id"], "dr-online-token"
    )
    _online(seeder, runner["id"])
    return runner


@pytest.fixture()
def offline_runner(seeder, world):
    return seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], world["pod"]["id"], "dr-offline-token"
    )


@pytest.fixture()
def foreign_runner(seeder, world):
    return seeder.enroll_runner(
        world["foreign"]["id"], world["ws"]["id"], world["pod"]["id"], "dr-foreign-token"
    )


@pytest.fixture()
def dedicated_issue(seeder, world, online_runner):
    seeder.db.execute(
        "UPDATE issues SET assigned_runner_id=%s, assigned_pod_id=%s WHERE id=%s",
        (online_runner["id"], world["pod"]["id"], world["issue1"]["id"]),
    )
    return world["issue1"]


@pytest.fixture()
def project2(seeder, world):
    project = seeder.create_project(world["ws"]["id"], name="DR2")
    seeder.create_project_member(world["ws"]["id"], project["id"], world["admin"]["id"], role=20)
    return project


@pytest.fixture()
def other_project_runner(seeder, world, project2):
    pod2 = seeder.create_pod(world["ws"]["id"], project2["id"], name="dr-pod2")
    return seeder.enroll_runner(
        world["admin"]["id"], world["ws"]["id"], pod2["id"], "dr-other-token"
    )


@pytest.fixture()
def dedicated_run(seeder, world, online_runner, dedicated_issue):
    run_id = _uid()
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, status, prompt, run_config, required_capabilities, thread_id,
            error_details, workspace_id, work_item_id, pod_id, created_by_id,
            llm_model, trigger, executor_kind, dispatch_attempts, cancel_reason,
            tool_plan, phase_kind, agent_metadata, usage, pinned_runner_id,
            pin_kind, created_at)
           VALUES (%s,'queued','dedicated prompt','{}','{}','',
                   '{}',%s,%s,%s,%s,'model','human','local_runner',0,'',
                   '{}','work','{}','{"input": 0, "output": 0, "total": 0}',
                   %s,'dedicated',now())""",
        (
            run_id,
            world["ws"]["id"],
            world["issue1"]["id"],
            world["pod"]["id"],
            world["admin"]["id"],
            online_runner["id"],
        ),
    )
    seeder.track.add("agent_run", run_id)
    return {"id": run_id}


@pytest.fixture()
def merge_world(seeder, world):
    """Two machines (source/target), each with an engine runner for the
    same project (§23.4): the source's engine in the default pod, the
    target's engine in a second pod. Issue I1 is dedicated to the
    source's engine. `runner`/`dev_machine` return ids (strings)."""
    admin_id = world["admin"]["id"]
    ws_id = world["ws"]["id"]
    project_id = world["project"]["id"]
    source_id = seeder.create_dev_machine(admin_id, host_label=f"dr-src-{seeder.tag}")["id"]
    target_id = seeder.create_dev_machine(admin_id, host_label=f"dr-tgt-{seeder.tag}")["id"]
    pod_b = seeder.create_pod(ws_id, project_id, name="dr-pod-b")
    loser = seeder.enroll_runner(
        admin_id, ws_id, world["pod"]["id"], "dr-loser-token", name=f"dr-loser-{seeder.tag}"
    )
    seeder.db.execute(
        "UPDATE runner SET provisioning='desktop_bundled', dev_machine_id=%s WHERE id=%s",
        (source_id, loser["id"]),
    )
    loser_id = loser["id"]
    survivor = seeder.enroll_runner(
        admin_id, ws_id, pod_b["id"], "dr-survivor-token", name=f"dr-survivor-{seeder.tag}"
    )
    seeder.db.execute(
        "UPDATE runner SET provisioning='desktop_bundled', dev_machine_id=%s WHERE id=%s",
        (target_id, survivor["id"]),
    )
    survivor_id = survivor["id"]
    seeder.db.execute(
        "UPDATE issues SET assigned_runner_id=%s, assigned_pod_id=%s WHERE id=%s",
        (loser_id, world["pod"]["id"], world["issue1"]["id"]),
    )
    return {
        "source_id": source_id,
        "target_id": target_id,
        "loser_id": loser_id,
        "survivor_id": survivor_id,
        "pod_b_id": pod_b["id"],
    }


@pytest.fixture()
def merge_run(seeder, world, merge_world):
    """A QUEUED dedicated run on the merge issue, in the survivor's pod
    (the §23.4 same-pod re-point case)."""
    run_id = _uid()
    seeder.db.execute(
        """INSERT INTO agent_run
           (id, status, prompt, run_config, required_capabilities, thread_id,
            error_details, workspace_id, work_item_id, pod_id, created_by_id,
            llm_model, trigger, executor_kind, dispatch_attempts, cancel_reason,
            tool_plan, phase_kind, agent_metadata, usage, pinned_runner_id,
            pin_kind, created_at)
           VALUES (%s,'queued','merge prompt','{}','{}','',
                   '{}',%s,%s,%s,%s,'model','human','local_runner',0,'',
                   '{}','work','{}','{"input": 0, "output": 0, "total": 0}',
                   %s,'dedicated',now())""",
        (
            run_id,
            world["ws"]["id"],
            world["issue1"]["id"],
            merge_world["pod_b_id"],
            world["admin"]["id"],
            merge_world["loser_id"],
        ),
    )
    seeder.track.add("agent_run", run_id)
    return {"id": run_id}


@pytest.fixture()
def admin_client(settings, world):
    db = LazyDatabase(settings.database_url)
    try:
        key = forge_session_key(
            db.connect(),
            user_id=world["admin"]["id"],
            password_field=PASSWORD_FIELD,
            secret=get_secret_key(),
        )
    finally:
        db.close()
    client = authed_client(settings.base_url, key)
    yield client
    client.close()
