"""Shared fixtures for the v1_schedulers contract suite (PIDASHCONV-1061).

Ports Django's ``apps/api/pi_dash/tests/contract/api/test_schedulers.py``
(PDASHOSS01-225, commit 04920ea3) to the dual-backend harness: one isolated
workspace + project per session run with a scheduler, its binding, a pod,
three runs, and issue/comment audit rows. Unique slugs per run, so reruns
never collide and no teardown is needed.

Workspaces are seeded with raw SQL, so the ``post_save(Workspace)`` builtin
scheduler signal never fires and the scheduler lists stay deterministic.

Every test module authenticates with its own API key (``keys[<module>]``)
so the backend's per-key rate throttle never couples modules together.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db  # noqa: E402

DTSTART = "2026-10-01T00:00:00+00:00"
NEXT_RUN_AT = "2026-10-20T00:00:00+00:00"
RUN_OLD_AT = "2026-10-10T00:00:00+00:00"
RUN_MID_AT = "2026-10-11T00:00:00+00:00"
RUN_NEW_AT = "2026-10-12T00:00:00+00:00"


def _seed_scheduler(conn, *, workspace_id, slug, name, created_by_id):
    return db.insert_row(
        conn,
        "schedulers",
        {
            "workspace_id": workspace_id,
            "slug": slug,
            "name": name,
            "description": "Reads the project and files issues.",
            "prompt": "Scan the project for stalled issues.",
            "source": "builtin",
            "is_enabled": True,
            "color": "#3b82f6",
            "created_by_id": created_by_id,
        },
    )


def _seed_binding(conn, *, workspace_id, project_id, scheduler_id, actor_id):
    return db.insert_row(
        conn,
        "scheduler_bindings",
        {
            "workspace_id": workspace_id,
            "project_id": project_id,
            "scheduler_id": scheduler_id,
            "dtstart": DTSTART,
            "tzid": "UTC",
            "rrule": "FREQ=HOURLY",
            "rdates": [],
            "exdates": [],
            "extra_context": "",
            "enabled": True,
            "outcome_mode": "create_issue",
            "next_run_at": NEXT_RUN_AT,
            "last_error": "",
            "actor_id": actor_id,
        },
    )


def _seed_run(
    conn,
    *,
    workspace_id,
    pod_id,
    binding_id,
    created_by_id,
    status,
    created_at,
    started_at=None,
    error_code="",
    phase_kind="",
):
    return db.insert_row(
        conn,
        "agent_run",
        {
            "status": status,
            "prompt": "Seeded run",
            "run_config": {},
            "required_capabilities": {},
            "thread_id": "seed-thread",
            "error": "",
            "workspace_id": workspace_id,
            "pod_id": pod_id,
            "created_by_id": created_by_id,
            "llm_model": "seed-model",
            "refusal_category": "",
            "failure_reason": "",
            "trigger": "scheduler",
            "executor_kind": "local_runner",
            "dispatch_attempts": 0,
            "cancel_reason": "",
            "error_code": error_code,
            "tool_plan": {},
            "phase_kind": phase_kind,
            "agent_metadata": {},
            "usage": {"input": 1, "output": 1, "total": 2},
            "started_at": started_at,
            "scheduler_binding_id": binding_id,
            "created_at": created_at,
        },
    )


def _seed_issue(conn, *, workspace_id, project_id, name, sequence_id):
    return db.insert_row(
        conn,
        "issues",
        {
            "workspace_id": workspace_id,
            "project_id": project_id,
            "state_id": None,
            "name": name,
            "description_json": {},
            "description_html": "<p></p>",
            "priority": "none",
            "complexity_score": 0,
            "sequence_id": sequence_id,
            "sort_order": 65535,
            "is_draft": False,
            "git_work_branch": "",
            "workpad": "",
        },
    )


def _seed_comment(
    conn, *, workspace_id, project_id, issue_id, actor_id, run_id, created_at=None
):
    values = {
        "workspace_id": workspace_id,
        "project_id": project_id,
        "comment_stripped": "test comment",
        "comment_json": {},
        "comment_html": "<p>test comment</p>",
        "attachments": "{}",
        "labels": "{}",
        "issue_id": issue_id,
        "actor_id": actor_id,
        "access": "EXTERNAL",
        "speaker_type": "agent",
        "speaker_label": "",
        "speaker_agent_run_id": run_id,
    }
    if created_at is not None:
        values["created_at"] = created_at
    return db.insert_row(conn, "issue_comments", values)


@pytest.fixture(scope="session")
def conn():
    connection = db.connect()
    yield connection
    connection.close()


@pytest.fixture(scope="session")
def seed(conn):
    tag = db.new_tag()
    owner = db.create_user(conn, tag + "o", first_name="CtOwner")
    member = db.create_user(conn, tag + "m", first_name="CtMember")
    nonmember = db.create_user(conn, tag + "n", first_name="CtNonmember")
    outsider = db.create_user(conn, tag + "x", first_name="CtOutsider")
    owner_b = db.create_user(conn, tag + "b", first_name="CtOwnerB")

    ws_a = db.create_workspace(conn, tag + "a", owner["id"])
    ws_b = db.create_workspace(conn, tag + "b", owner_b["id"])
    db.add_workspace_member(conn, ws_a["id"], owner["id"], db.ADMIN)
    db.add_workspace_member(conn, ws_a["id"], member["id"], db.MEMBER)
    db.add_workspace_member(conn, ws_a["id"], nonmember["id"], db.MEMBER)
    db.add_workspace_member(conn, ws_b["id"], owner_b["id"], db.ADMIN)

    project = db.create_project(
        conn, ws_a["id"], tag + "p", created_by_id=owner["id"]
    )
    db.add_project_member(conn, ws_a["id"], project["id"], owner["id"], db.ADMIN)
    db.add_project_member(conn, ws_a["id"], project["id"], member["id"], db.ADMIN)

    hidden_project = db.create_project(
        conn, ws_a["id"], tag + "h", created_by_id=owner["id"]
    )

    project_b = db.create_project(
        conn, ws_b["id"], tag + "q", created_by_id=owner_b["id"]
    )
    db.add_project_member(conn, ws_b["id"], project_b["id"], owner_b["id"], db.ADMIN)

    scheduler = _seed_scheduler(
        conn,
        workspace_id=ws_a["id"],
        slug=f"sched-{tag}",
        name=f"CT Scheduler {tag}",
        created_by_id=owner["id"],
    )
    bare_scheduler = _seed_scheduler(
        conn,
        workspace_id=ws_a["id"],
        slug=f"bare-{tag}",
        name=f"CT Bare {tag}",
        created_by_id=owner["id"],
    )
    other_scheduler = _seed_scheduler(
        conn,
        workspace_id=ws_b["id"],
        slug=f"other-{tag}",
        name=f"CT Other {tag}",
        created_by_id=owner_b["id"],
    )
    binding = _seed_binding(
        conn,
        workspace_id=ws_a["id"],
        project_id=project["id"],
        scheduler_id=scheduler["id"],
        actor_id=owner["id"],
    )
    pod = db.insert_row(
        conn,
        "pod",
        {
            "name": f"pod-{tag}",
            "description": "Seeded for contract tests",
            "is_default": True,
            "workspace_id": ws_a["id"],
            "project_id": project["id"],
        },
    )
    run_old = _seed_run(
        conn,
        workspace_id=ws_a["id"],
        pod_id=pod["id"],
        binding_id=binding["id"],
        created_by_id=owner["id"],
        status="failed",
        created_at=RUN_OLD_AT,
        started_at=RUN_OLD_AT,
        error_code="watchdog_timeout",
    )
    run_mid = _seed_run(
        conn,
        workspace_id=ws_a["id"],
        pod_id=pod["id"],
        binding_id=binding["id"],
        created_by_id=owner["id"],
        status="running",
        created_at=RUN_MID_AT,
        started_at=RUN_MID_AT,
    )
    run_new = _seed_run(
        conn,
        workspace_id=ws_a["id"],
        pod_id=pod["id"],
        binding_id=binding["id"],
        created_by_id=owner["id"],
        status="queued",
        created_at=RUN_NEW_AT,
    )
    issue_filed = _seed_issue(
        conn,
        workspace_id=ws_a["id"],
        project_id=project["id"],
        name="Filed by scheduler",
        sequence_id=11,
    )
    _seed_comment(
        conn,
        workspace_id=ws_a["id"],
        project_id=project["id"],
        issue_id=issue_filed["id"],
        actor_id=owner["id"],
        run_id=run_old["id"],
    )
    issue_visible = _seed_issue(
        conn,
        workspace_id=ws_a["id"],
        project_id=project["id"],
        name="Visible to caller",
        sequence_id=12,
    )
    _seed_comment(
        conn,
        workspace_id=ws_a["id"],
        project_id=project["id"],
        issue_id=issue_visible["id"],
        actor_id=member["id"],
        run_id=run_mid["id"],
    )
    issue_hidden = _seed_issue(
        conn,
        workspace_id=ws_a["id"],
        project_id=hidden_project["id"],
        name="Should not be visible",
        sequence_id=1,
    )
    _seed_comment(
        conn,
        workspace_id=ws_a["id"],
        project_id=hidden_project["id"],
        issue_id=issue_hidden["id"],
        actor_id=owner["id"],
        run_id=run_mid["id"],
    )

    keys = {}
    for module in ("ws", "pl", "dt", "rn"):
        keys[module] = db.create_api_token(conn, owner["id"], tag + module)["token"]
    keys["member"] = db.create_api_token(conn, member["id"], tag + "mm")["token"]
    keys["nonmember"] = db.create_api_token(conn, nonmember["id"], tag + "nn")["token"]
    keys["outsider"] = db.create_api_token(conn, outsider["id"], tag + "xx")["token"]
    keys["owner_b"] = db.create_api_token(conn, owner_b["id"], tag + "bb")["token"]

    return {
        "tag": tag,
        "owner": owner,
        "member": member,
        "nonmember": nonmember,
        "outsider": outsider,
        "owner_b": owner_b,
        "ws_a": ws_a,
        "ws_b": ws_b,
        "project": project,
        "hidden_project": hidden_project,
        "project_b": project_b,
        "scheduler": scheduler,
        "bare_scheduler": bare_scheduler,
        "other_scheduler": other_scheduler,
        "binding": binding,
        "pod": pod,
        "run_old": run_old,
        "run_mid": run_mid,
        "run_new": run_new,
        "issue_filed": issue_filed,
        "issue_visible": issue_visible,
        "issue_hidden": issue_hidden,
        "keys": keys,
    }


# PIDASHCONV-1204: a run that commented on the same issue more than once must
# list one entry per comment, newest comment first — Django's ``_issues_by_run``
# is ``.values_list(...).distinct()`` with ``IssueComment.Meta.ordering``
# (``-created_at``), and Django's compiler adds the ordering column to the
# ``SELECT DISTINCT`` list, so each comment is its own row.
REPEAT_T0 = "2026-10-13T00:00:00+00:00"
REPEAT_T1 = "2026-10-13T01:00:00+00:00"
REPEAT_T2 = "2026-10-13T02:00:00+00:00"
REPEAT_RUN_AT = "2026-10-12T00:00:00+00:00"


@pytest.fixture(scope="session")
def repeat_seed(conn):
    """A run R with comments on I1 (t0), I2 (t1), I1 (t2), t0 < t1 < t2."""
    tag = db.new_tag()
    owner = db.create_user(conn, tag + "o", first_name="CtRepeatOwner")
    ws = db.create_workspace(conn, tag + "w", owner["id"])
    db.add_workspace_member(conn, ws["id"], owner["id"], db.ADMIN)
    project = db.create_project(conn, ws["id"], tag + "p", created_by_id=owner["id"])
    db.add_project_member(conn, ws["id"], project["id"], owner["id"], db.ADMIN)

    scheduler = _seed_scheduler(
        conn,
        workspace_id=ws["id"],
        slug=f"repeat-{tag}",
        name=f"CT Repeat {tag}",
        created_by_id=owner["id"],
    )
    binding = _seed_binding(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        scheduler_id=scheduler["id"],
        actor_id=owner["id"],
    )
    pod = db.insert_row(
        conn,
        "pod",
        {
            "name": f"pod-repeat-{tag}",
            "description": "Seeded for contract tests",
            "is_default": True,
            "workspace_id": ws["id"],
            "project_id": project["id"],
        },
    )
    run = _seed_run(
        conn,
        workspace_id=ws["id"],
        pod_id=pod["id"],
        binding_id=binding["id"],
        created_by_id=owner["id"],
        status="completed",
        created_at=REPEAT_RUN_AT,
        started_at=REPEAT_RUN_AT,
    )
    issue_1 = _seed_issue(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        name="Repeated issue",
        sequence_id=21,
    )
    issue_2 = _seed_issue(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        name="Once issue",
        sequence_id=22,
    )
    _seed_comment(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        issue_id=issue_1["id"],
        actor_id=owner["id"],
        run_id=run["id"],
        created_at=REPEAT_T0,
    )
    _seed_comment(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        issue_id=issue_2["id"],
        actor_id=owner["id"],
        run_id=run["id"],
        created_at=REPEAT_T1,
    )
    _seed_comment(
        conn,
        workspace_id=ws["id"],
        project_id=project["id"],
        issue_id=issue_1["id"],
        actor_id=owner["id"],
        run_id=run["id"],
        created_at=REPEAT_T2,
    )

    keys = {"rn": db.create_api_token(conn, owner["id"], tag + "rn")["token"]}
    return {
        "tag": tag,
        "owner": owner,
        "ws_a": ws,
        "project": project,
        "scheduler": scheduler,
        "binding": binding,
        "pod": pod,
        "run": run,
        "issue_1": issue_1,
        "issue_2": issue_2,
        "keys": keys,
    }
