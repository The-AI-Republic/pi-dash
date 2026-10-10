# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-37 data-repair commands oracle (PIDASHCONV-808).

Executable F37-05 (``repair_sql.sql`` + ``repair.rows.json``) and F37-06
(``version_sync.golden.json``): every test seeds a scratch ``DATABASE_URL``
with raw SQL (no Django imports), drives ``manage.py <command>`` (the
Django oracle) and ``pidash-api ops <command>`` (the Rust port) as
subprocesses, and asserts byte-identical stdout/stderr/exit plus
equivalent DB and broker effects.

Run from ``rust-api/contract-tests`` (the binary must be built)::

    export DATABASE_URL=postgresql://...  # your own scratch database
    export CELERY_BROKER_URL=amqp://...   # broker for the sync_* tests
    export DJANGO_SETTINGS_MODULE=pi_dash.settings.local
    cargo build --bin pidash-api  # in rust-api/
    pytest -p no:django ops/test_repair.py -v

``PIDASH_MANAGE_PY`` / ``PIDASH_RUST_BIN`` override the driver paths.
No worker may consume the broker ``celery`` queue while the sync tests
run (the CI job starts none).
"""

from __future__ import annotations

import os
import subprocess
import sys
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest
from psycopg.types.json import Json

from _harness import config, db
from _harness.broker_probe import collect_matching

REPO_ROOT = Path(__file__).resolve().parents[3]
MANAGE_PY = os.environ.get("PIDASH_MANAGE_PY", str(REPO_ROOT / "apps/api/manage.py"))
RUST_BIN = os.environ.get(
    "PIDASH_RUST_BIN", str(REPO_ROOT / "rust-api/target/debug/pidash-api")
)

UTC = timezone.utc
OLD1 = datetime(2020, 1, 1, 10, 0, 0, tzinfo=UTC)
OLD2 = datetime(2020, 6, 1, 10, 0, 0, tzinfo=UTC)
OLD3 = datetime(2021, 1, 1, 10, 0, 0, tzinfo=UTC)
STALE = datetime(2019, 1, 1, 10, 0, 0, tzinfo=UTC)

COPY_DONE = b"Successfully Copied IssueComment to Description\n"
FIX_DONE = b"Sequence IDs updated successfully\n"
SYNC_VERSION_DONE = b"Successfully created issue version task\n"
SYNC_DESCRIPTION_DONE = b"Successfully created issue description version task\n"
SLUG_PROMPT = b"Workspace slug: "
TASK_VERSION = "pi_dash.bgtasks.issue_version_sync.schedule_issue_version"
TASK_DESCRIPTION = (
    "pi_dash.bgtasks.issue_description_version_sync.schedule_issue_description_version"
)


def run_django(*args: str, stdin_text: str = "") -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, MANAGE_PY, *args],
        input=stdin_text.encode(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )


def run_rust(*args: str, stdin_text: str = "") -> subprocess.CompletedProcess:
    return subprocess.run(
        [RUST_BIN, "ops", *args],
        input=stdin_text.encode(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )


def database_url() -> str:
    return config.database_url()


def tag(prefix: str) -> str:
    return f"{prefix}-{uuid.uuid4().hex[:10]}"


def seed_user(cur) -> uuid.UUID:
    user_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO users (id, username, password, first_name, last_name,
            avatar, date_joined, created_at, updated_at, last_location,
            created_location, is_superuser, is_managed, is_password_expired,
            is_active, is_staff, is_email_verified, is_password_autoset,
            token, user_timezone, last_login_ip, last_logout_ip,
            last_login_medium, last_login_uagent, is_bot, display_name,
            is_email_valid, is_password_reset_required)
            VALUES (%s, %s, '', '', '', '', %s, %s, %s, '', '', false,
            false, false, true, false, false, false, '', 'UTC', '', '',
            'email', '', false, '', false, false)""",
        (user_id, tag("r808u"), OLD1, OLD1, OLD1),
    )
    return user_id


def seed_workspace(
    cur,
    slug: str,
    owner_id: uuid.UUID,
    name: str = "Repair WS",
    deleted_at: datetime | None = None,
    updated_at: datetime = OLD3,
) -> uuid.UUID:
    ws_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO workspaces (id, name, slug, owner_id, created_at,
            updated_at, timezone, background_color, deleted_at)
            VALUES (%s, %s, %s, %s, %s, %s, 'UTC', '#ffffff', %s)""",
        (ws_id, name, slug, owner_id, OLD1, updated_at, deleted_at),
    )
    return ws_id


def seed_project(cur, ws_id: uuid.UUID, identifier: str) -> uuid.UUID:
    pr_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO projects (id, name, description, network, identifier,
            workspace_id, cycle_view, module_view, issue_views_view,
            page_view, intake_view, archive_in, close_in, logo_props,
            is_time_tracking_enabled, is_issue_type_enabled,
            guest_view_all_features, timezone, members_can_edit_states,
            repo_url, base_branch, agent_default_interval_seconds,
            agent_default_max_ticks, agent_ticking_enabled, is_default,
            agent_review_default_interval_seconds, default_agent_executor,
            agent_test_default_interval_seconds, created_at, updated_at)
            VALUES (%s, %s, '', 2, %s, %s, false, false, false, true,
            false, 0, 0, '{}', false, false, false, 'UTC', true, '', 'main',
            10800, 10, true, false, 10800, 'local_runner', 10800, %s, %s)""",
        (pr_id, f"P-{identifier}", identifier, ws_id, OLD1, OLD1),
    )
    return pr_id


def seed_issue(
    cur,
    ws_id: uuid.UUID,
    pr_id: uuid.UUID,
    name: str,
    sequence_id: int,
    created_at: datetime = OLD1,
) -> uuid.UUID:
    issue_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO issues (id, name, description_json, priority,
            sequence_id, project_id, workspace_id, description_html,
            sort_order, is_draft, git_work_branch, workpad,
            complexity_score, created_at, updated_at)
            VALUES (%s, %s, %s, 'none', %s, %s, %s, '<p></p>', 65535,
            false, '', '', 0, %s, %s)""",
        (issue_id, name, Json({}), sequence_id, pr_id, ws_id, created_at, created_at),
    )
    return issue_id


def seed_comment(
    cur,
    ws_id: uuid.UUID,
    pr_id: uuid.UUID,
    issue_id: uuid.UUID,
    comment_json: dict | None = None,
    comment_html: str = "<p>c</p>",
    comment_stripped: str = "c",
    created_at: datetime = OLD2,
    deleted_at: datetime | None = None,
    description_id: uuid.UUID | None = None,
) -> uuid.UUID:
    comment_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO issue_comments (id, comment_stripped, attachments,
            issue_id, project_id, workspace_id, comment_html, comment_json,
            access, speaker_type, speaker_label, labels, created_at,
            updated_at, deleted_at, description_id)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, 'INTERNAL', 'human',
            '', %s, %s, %s, %s, %s)""",
        (
            comment_id,
            comment_stripped,
            [],
            issue_id,
            pr_id,
            ws_id,
            comment_html,
            Json(comment_json if comment_json is not None else {}),
            [],
            created_at,
            created_at,
            deleted_at,
            description_id,
        ),
    )
    return comment_id


def seed_sequence(
    cur,
    ws_id: uuid.UUID,
    pr_id: uuid.UUID,
    issue_id: uuid.UUID | None,
    sequence: int,
    created_at: datetime = OLD1,
) -> uuid.UUID:
    seq_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO issue_sequences (id, sequence, deleted, project_id,
            workspace_id, created_at, updated_at, issue_id)
            VALUES (%s, %s, false, %s, %s, %s, %s, %s)""",
        (seq_id, sequence, pr_id, ws_id, created_at, created_at, issue_id),
    )
    return seq_id


def seed_description(
    cur,
    ws_id: uuid.UUID,
    pr_id: uuid.UUID | None = None,
    created_at: datetime = OLD1,
) -> uuid.UUID:
    desc_id = uuid.uuid4()
    cur.execute(
        """INSERT INTO descriptions (id, description_json, description_html,
            workspace_id, project_id, created_at, updated_at)
            VALUES (%s, %s, '<p>d</p>', %s, %s, %s, %s)""",
        (desc_id, Json({}), ws_id, pr_id, created_at, created_at),
    )
    return desc_id


def fetch_descriptions(ws_id: uuid.UUID) -> list[dict]:
    return db.fetchall(
        database_url(),
        """SELECT id, created_at, updated_at, description_json,
            description_html, description_stripped, description_binary,
            project_id, created_by_id, updated_by_id, workspace_id,
            deleted_at FROM descriptions WHERE workspace_id = %s""",
        (ws_id,),
    )


def fetch_comments(ws_id: uuid.UUID) -> list[dict]:
    return db.fetchall(
        database_url(),
        """SELECT id, created_at, updated_at, comment_json, comment_html,
            comment_stripped, project_id, created_by_id, updated_by_id,
            workspace_id, description_id, deleted_at
            FROM issue_comments WHERE workspace_id = %s ORDER BY created_at""",
        (ws_id,),
    )


def seed_copy_world(comment_count: int = 3) -> dict:
    """One workspace/project/issues plus NULL-description comments.

    Includes a soft-deleted NULL comment (must be ignored) and a
    pre-linked comment (must be skipped) alongside ``comment_count``
    live NULL comments with distinct payloads.
    """
    with db.connect(database_url()) as conn, conn.cursor() as cur:
        owner = seed_user(cur)
        slug = tag("r808copy")
        ws_id = seed_workspace(cur, slug, owner)
        pr_id = seed_project(cur, ws_id, tag("CP")[:12])
        issue_a = seed_issue(cur, ws_id, pr_id, "copy-a", 1)
        issue_b = seed_issue(cur, ws_id, pr_id, "copy-b", 2)
        comment_ids = []
        for i in range(comment_count):
            issue = issue_a if i % 2 == 0 else issue_b
            comment_ids.append(
                seed_comment(
                    cur,
                    ws_id,
                    pr_id,
                    issue,
                    comment_json={"n": i},
                    comment_html=f"<p>c{i}</p>",
                    comment_stripped=f"c{i}",
                    created_at=OLD2 + timedelta(seconds=i),
                )
            )
        deleted_id = seed_comment(
            cur, ws_id, pr_id, issue_a, comment_html="<p>del</p>",
            comment_stripped="del", deleted_at=OLD3,
        )
        linked_desc = seed_description(cur, ws_id, pr_id)
        linked_id = seed_comment(
            cur, ws_id, pr_id, issue_a, description_id=linked_desc
        )
    return {
        "ws_id": ws_id,
        "comment_ids": comment_ids,
        "deleted_id": deleted_id,
        "linked_id": linked_id,
        "linked_desc": linked_desc,
    }


def assert_copy_effects(world: dict, before: datetime, new_count: int) -> None:
    ws_id = world["ws_id"]
    descriptions = {
        str(d["id"]): d for d in fetch_descriptions(ws_id)
    }
    # One pre-linked description plus one per live NULL comment.
    assert len(descriptions) == new_count + 1
    comments = {str(c["id"]): c for c in fetch_comments(ws_id)}
    for position, comment_id in enumerate(world["comment_ids"]):
        comment = comments[str(comment_id)]
        assert comment["description_id"] is not None
        desc = descriptions[str(comment["description_id"])]
        assert desc["description_json"] == comment["comment_json"]
        assert desc["description_html"] == comment["comment_html"]
        assert desc["description_stripped"] == comment["comment_stripped"]
        assert desc["project_id"] == comment["project_id"]
        assert desc["created_by_id"] == comment["created_by_id"]
        assert desc["updated_by_id"] == comment["updated_by_id"]
        assert desc["workspace_id"] == comment["workspace_id"]
        assert desc["description_binary"] is None
        assert desc["deleted_at"] is None
        # auto_now_add/auto_now overwrite: batch now, not the comment's.
        assert desc["created_at"] >= before
        assert desc["updated_at"] >= before
        # bulk_update writes description_id only: no updated_at bump.
        assert comment["updated_at"] == OLD2 + timedelta(seconds=position)
    assert comments[str(world["deleted_id"])]["description_id"] is None
    assert comments[str(world["linked_id"])]["description_id"] == world["linked_desc"]


def test_copy_small_batch() -> None:
    dj_world = seed_copy_world()
    before = datetime.now(UTC)
    dj = run_django("copy_issue_comment_to_description")
    assert dj.returncode == 0
    assert dj.stdout == COPY_DONE
    assert dj.stderr == b""
    assert_copy_effects(dj_world, before, 3)

    rs_world = seed_copy_world()
    before = datetime.now(UTC)
    rs = run_rust("copy_issue_comment_to_description")
    assert rs.returncode == dj.returncode
    assert rs.stdout == dj.stdout == COPY_DONE
    assert rs.stderr == dj.stderr == b""
    assert_copy_effects(rs_world, before, 3)


def test_copy_spans_batches() -> None:
    before = datetime.now(UTC)
    dj_world = seed_copy_world(comment_count=501)
    dj = run_django("copy_issue_comment_to_description")
    assert dj.returncode == 0
    assert dj.stdout == COPY_DONE
    assert dj.stderr == b""
    assert_copy_effects(dj_world, before, 501)

    before = datetime.now(UTC)
    rs_world = seed_copy_world(comment_count=501)
    rs = run_rust("copy_issue_comment_to_description")
    assert rs.returncode == dj.returncode
    assert rs.stdout == dj.stdout == COPY_DONE
    assert rs.stderr == dj.stderr == b""
    assert_copy_effects(rs_world, before, 501)


def test_copy_rerun_is_noop() -> None:
    for run in (run_django, run_rust):
        world = seed_copy_world()
        first = run("copy_issue_comment_to_description")
        assert first.returncode == 0
        count_before = len(fetch_descriptions(world["ws_id"]))
        second = run("copy_issue_comment_to_description")
        assert second.returncode == 0
        assert second.stdout == COPY_DONE
        assert second.stderr == b""
        assert len(fetch_descriptions(world["ws_id"])) == count_before


def seed_fix_world(
    identifier: str = "FX",
    duplicate_sequence: int = 7,
    duplicate_count: int = 3,
    max_sequence: int = 50,
    with_stale: bool = True,
    with_sequences: bool = True,
) -> dict:
    """Workspace/project with duplicate issues and sequence rows.

    Duplicates get distinct ``created_at`` (oldest first); the newest
    keeps its id. ``with_stale`` adds an older second sequence row for
    the middle duplicate (it must win the id-map). ``max_sequence``
    sets ``MAX(sequence)`` via an unrelated issue.
    """
    with db.connect(database_url()) as conn, conn.cursor() as cur:
        owner = seed_user(cur)
        slug = tag("r808fix")
        ws_id = seed_workspace(cur, slug, owner)
        ident = f"{identifier}{uuid.uuid4().hex[:6]}".upper()[:12]
        pr_id = seed_project(cur, ws_id, ident)
        created = [OLD1, OLD2, OLD3, OLD3 + timedelta(seconds=1)]
        dup_ids = [
            seed_issue(
                cur, ws_id, pr_id, f"dup-{i}", duplicate_sequence,
                created_at=created[i],
            )
            for i in range(duplicate_count)
        ]
        seq_ids: dict[str, uuid.UUID] = {}
        stale_id = None
        if with_sequences:
            for issue_id in dup_ids:
                seq_ids[str(issue_id)] = seed_sequence(
                    cur, ws_id, pr_id, issue_id, duplicate_sequence
                )
            if with_stale and duplicate_count >= 2:
                stale_id = seed_sequence(
                    cur, ws_id, pr_id, dup_ids[1], duplicate_sequence,
                    created_at=STALE,
                )
            other = seed_issue(cur, ws_id, pr_id, "other", max_sequence)
            seed_sequence(cur, ws_id, pr_id, other, max_sequence)
    return {
        "ws_id": ws_id,
        "slug": slug,
        "ident": ident,
        "dup_ids": dup_ids,
        "seq_ids": seq_ids,
        "stale_id": stale_id,
        "max_sequence": max_sequence,
    }


def fetch_issue_sequences(ws_id: uuid.UUID) -> list[dict]:
    return db.fetchall(
        database_url(),
        "SELECT id, issue_id, sequence, updated_at FROM issue_sequences"
        " WHERE workspace_id = %s",
        (ws_id,),
    )


def fetch_issues(ws_id: uuid.UUID) -> list[dict]:
    return db.fetchall(
        database_url(),
        "SELECT id, name, sequence_id, created_at, updated_at FROM issues"
        " WHERE workspace_id = %s",
        (ws_id,),
    )


def test_fix_renumbers_under_lock() -> None:
    for run in (run_django, run_rust):
        world = seed_fix_world()
        slug = world["slug"]
        ident = world["ident"]
        proc = run(
            "fix_duplicate_sequences", f"{ident}-7", stdin_text=f"{slug}\n"
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Workspace slug: 3 issues found with identifier {ident}-7\n"
            "Sequence IDs updated successfully\n"
        ).encode()
        assert proc.stderr == b""

        issues = {str(i["id"]): i for i in fetch_issues(world["ws_id"])}
        dup_ids = [str(i) for i in world["dup_ids"]]
        # Newest (dup_ids[2]) keeps 7; the rest take last+N in
        # -created_at order: dup_ids[1] -> 51, dup_ids[0] -> 52.
        assert issues[dup_ids[2]]["sequence_id"] == 7
        assert issues[dup_ids[1]]["sequence_id"] == 51
        assert issues[dup_ids[0]]["sequence_id"] == 52
        # bulk_update writes sequence_id only: no updated_at bump.
        assert issues[dup_ids[0]]["updated_at"] == OLD1
        assert issues[dup_ids[1]]["updated_at"] == OLD2
        assert issues[dup_ids[2]]["updated_at"] == OLD3

        seqs = {str(s["id"]): s for s in fetch_issue_sequences(world["ws_id"])}
        stale_id = str(world["stale_id"])
        assert seqs[stale_id]["sequence"] == 51
        normal_middle = str(world["seq_ids"][dup_ids[1]])
        assert seqs[normal_middle]["sequence"] == 7
        assert seqs[str(world["seq_ids"][dup_ids[0]])]["sequence"] == 52
        assert seqs[str(world["seq_ids"][dup_ids[2]])]["sequence"] == 7
        for row in seqs.values():
            assert row["updated_at"] in (OLD1, STALE)


def test_fix_matches_identifier_case_insensitively() -> None:
    for run in (run_django, run_rust):
        world = seed_fix_world(duplicate_count=2, with_stale=False)
        slug = world["slug"]
        ident = world["ident"]
        proc = run(
            "fix_duplicate_sequences", f"{ident.lower()}-7",
            stdin_text=f"{slug}\n",
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Workspace slug: 2 issues found with identifier {ident.lower()}-7\n"
            "Sequence IDs updated successfully\n"
        ).encode()
        assert proc.stderr == b""
        issues = {str(i["id"]): i for i in fetch_issues(world["ws_id"])}
        dup_ids = [str(i) for i in world["dup_ids"]]
        assert issues[dup_ids[1]]["sequence_id"] == 7
        assert issues[dup_ids[0]]["sequence_id"] == 51


def test_fix_without_sequences_reraises_type_error() -> None:
    for run in (run_django, run_rust):
        world = seed_fix_world(
            duplicate_count=2, with_stale=False, with_sequences=False
        )
        slug = world["slug"]
        ident = world["ident"]
        before = {
            str(i["id"]): i["sequence_id"]
            for i in fetch_issues(world["ws_id"])
        }
        proc = run(
            "fix_duplicate_sequences", f"{ident}-7", stdin_text=f"{slug}\n"
        )
        assert proc.returncode == 1
        # The count line prints before the TypeError; the transaction
        # rolls back, so no writes land.
        assert proc.stdout == (
            f"Workspace slug: 2 issues found with identifier {ident}-7\n"
        ).encode()
        assert proc.stderr == (
            b"CommandError: unsupported operand type(s) for +:"
            b" 'NoneType' and 'int'\n"
        )
        after = {
            str(i["id"]): i["sequence_id"]
            for i in fetch_issues(world["ws_id"])
        }
        assert after == before


@pytest.mark.parametrize(
    ("identifier", "stdin_slug", "stderr_line"),
    [
        ("FX-7", "", b"CommandError: Workspace slug is required\n"),
        ("FX", "SLUG", b"CommandError: Invalid issue identifier format\n"),
        ("FX-7-X", "SLUG", b"CommandError: Invalid issue identifier format\n"),
        ("FX-abc", "SLUG", b"CommandError: Invalid integer string\n"),
        ("FX-", "SLUG", b"CommandError: Invalid integer string\n"),
        ("ZZ-7", "SLUG", b"CommandError: Project matching query does not exist.\n"),
        ("FX-9", "SLUG", b"CommandError: No duplicate issues found with the given identifier\n"),
    ],
)
def test_fix_rejections(identifier: str, stdin_slug: str, stderr_line: bytes) -> None:
    for run in (run_django, run_rust):
        world = seed_fix_world()
        slug = world["slug"]
        ident = world["ident"]
        use_ident = identifier.replace("FX", ident).replace("ZZ", "ZZ")
        use_slug = stdin_slug.replace("SLUG", slug)
        proc = run(
            "fix_duplicate_sequences", use_ident, stdin_text=f"{use_slug}\n"
        )
        assert proc.returncode == 1
        assert proc.stdout == SLUG_PROMPT
        assert proc.stderr == stderr_line, (run, proc.stderr)


DELETED_AT = datetime(2024, 5, 6, 7, 8, 9, tzinfo=UTC)
DELETED_EPOCH = 1714979289


def seed_slug_world(kind: str) -> dict:
    """Workspace for one slug branch: live, deleted, stamped, colliding."""
    with db.connect(database_url()) as conn, conn.cursor() as cur:
        owner = seed_user(cur)
        slug = tag("r808slug")
        if kind == "live":
            ws_id = seed_workspace(cur, slug, owner, name="Live WS")
        elif kind == "stamped":
            ws_id = seed_workspace(
                cur, f"{slug}__1714976889", owner, name="Stamped WS",
                deleted_at=OLD3,
            )
            slug = f"{slug}__1714976889"
        elif kind == "colliding":
            ws_id = seed_workspace(
                cur, slug, owner, name="Del WS", deleted_at=DELETED_AT
            )
            seed_workspace(
                cur, f"{slug}__{DELETED_EPOCH}", owner, name="Blocker WS"
            )
        else:  # deleted
            ws_id = seed_workspace(
                cur, slug, owner, name="Del WS", deleted_at=DELETED_AT
            )
    return {"ws_id": ws_id, "slug": slug}


def fetch_workspace_by_id(ws_id: uuid.UUID) -> dict | None:
    return db.fetchone(
        database_url(),
        "SELECT id, name, slug, updated_at, deleted_at FROM workspaces"
        " WHERE id = %s",
        (ws_id,),
    )


def test_slug_not_found() -> None:
    for run in (run_django, run_rust):
        missing = tag("r808missing")
        proc = run("update_deleted_workspace_slug", missing)
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Workspace with slug '{missing}' not found.\n"
        ).encode()
        assert proc.stderr == b""


def test_slug_not_deleted() -> None:
    for run in (run_django, run_rust):
        world = seed_slug_world("live")
        proc = run("update_deleted_workspace_slug", world["slug"])
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Workspace 'Live WS' (slug: {world['slug']}) is not deleted.\n"
        ).encode()
        assert proc.stderr == b""


def test_slug_already_stamped() -> None:
    for run in (run_django, run_rust):
        world = seed_slug_world("stamped")
        proc = run("update_deleted_workspace_slug", world["slug"])
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Workspace 'Stamped WS' (slug: {world['slug']})"
            " already has a timestamp appended.\n"
        ).encode()
        assert proc.stderr == b""


def test_slug_dry_run_writes_nothing() -> None:
    for run in (run_django, run_rust):
        world = seed_slug_world("deleted")
        proc = run(
            "update_deleted_workspace_slug", world["slug"], "--dry-run"
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Would update workspace 'Del WS' slug from '{world['slug']}'"
            f" to '{world['slug']}__{DELETED_EPOCH}'\n"
        ).encode()
        assert proc.stderr == b""
        row = fetch_workspace_by_id(world["ws_id"])
        assert row is not None
        assert row["slug"] == world["slug"]
        assert row["updated_at"] == OLD3


def test_slug_write_stamps_twice() -> None:
    for run in (run_django, run_rust):
        world = seed_slug_world("deleted")
        proc = run("update_deleted_workspace_slug", world["slug"])
        assert proc.returncode == 0
        # Ported bug: both slots show the NEW slug.
        assert proc.stdout == (
            f"Updated workspace 'Del WS' slug from"
            f" '{world['slug']}__{DELETED_EPOCH}'"
            f" to '{world['slug']}__{DELETED_EPOCH}'\n"
        ).encode()
        assert proc.stderr == b""
        row = fetch_workspace_by_id(world["ws_id"])
        assert row is not None
        assert row["slug"] == f"{world['slug']}__{DELETED_EPOCH}"
        # update_fields=["slug"]: no updated_at bump.
        assert row["updated_at"] == OLD3


def test_slug_save_failure_stays_on_stdout() -> None:
    # The target slug is taken: the write collides. The `{error}` tail
    # is driver-specific (psycopg vs sqlx), so only the shape is pinned.
    for run in (run_django, run_rust):
        world = seed_slug_world("colliding")
        proc = run("update_deleted_workspace_slug", world["slug"])
        assert proc.returncode == 0, proc.stderr
        assert proc.stderr == b""
        assert proc.stdout.startswith(
            b"Error updating workspace 'Del WS': "
        )
        row = fetch_workspace_by_id(world["ws_id"])
        assert row is not None
        assert row["slug"] == world["slug"]


def drain_marker(marker: str, timeout: float = 30.0) -> list[tuple]:
    def _matches(headers: dict, payload: list) -> bool:
        try:
            return payload[1].get("batch_size") == marker
        except (IndexError, AttributeError, TypeError):
            return False

    return collect_matching(_matches, timeout=timeout)


@pytest.mark.parametrize(
    ("command", "task", "done"),
    [
        ("sync_issue_version", TASK_VERSION, SYNC_VERSION_DONE),
        (
            "sync_issue_description_version",
            TASK_DESCRIPTION,
            SYNC_DESCRIPTION_DONE,
        ),
    ],
)
def test_sync_publishes_delay_payload(
    command: str, task: str, done: bytes
) -> None:
    for run in (run_django, run_rust):
        marker = f"ops-{uuid.uuid4().hex[:12]}"
        proc = run(command, stdin_text=f"{marker}\n300\n")
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Enter the batch size: Enter the batch countdown: " + done
        )
        assert proc.stderr == b""
        found = drain_marker(marker)
        assert len(found) == 1
        headers, payload = found[0]
        assert headers["task"] == task
        assert payload[0] == []
        # Ported bug: batch_size stays the raw input string.
        assert payload[1] == {"batch_size": marker, "countdown": 300}
        assert list(payload[1].keys()) == ["batch_size", "countdown"]
        assert isinstance(payload[1]["batch_size"], str)
        assert isinstance(payload[1]["countdown"], int)
        assert headers["eta"] is None
        assert headers["retries"] == 0
        assert headers["argsrepr"] == "()"
        assert headers["kwargsrepr"] == (
            "{'batch_size': '%s', 'countdown': 300}" % marker
        )


@pytest.mark.parametrize(
    ("command", "task"),
    [
        ("sync_issue_version", TASK_VERSION),
        ("sync_issue_description_version", TASK_DESCRIPTION),
    ],
)
@pytest.mark.parametrize(
    ("raw", "expected"),
    [("300", 300), (" 45 ", 45), ("+7", 7), ("1_0", 10)],
)
def test_sync_countdown_keeps_py_int(
    command: str, task: str, raw: str, expected: int
) -> None:
    for run in (run_django, run_rust):
        marker = f"ops-{uuid.uuid4().hex[:12]}"
        proc = run(command, stdin_text=f"{marker}\n{raw}\n")
        assert proc.returncode == 0, proc.stderr
        assert proc.stderr == b""
        found = drain_marker(marker)
        assert len(found) == 1
        _headers, payload = found[0]
        assert payload[1]["batch_size"] == marker
        assert payload[1]["countdown"] == expected


@pytest.mark.parametrize("batch_size", ["", "007", "  padded  "])
def test_sync_batch_size_passes_through_verbatim(batch_size: str) -> None:
    for run in (run_django, run_rust):
        proc = run("sync_issue_version", stdin_text=f"{batch_size}\n5\n")
        assert proc.returncode == 0, proc.stderr
        assert proc.stderr == b""
        found = drain_marker(batch_size)
        assert len(found) == 1
        _headers, payload = found[0]
        assert payload[1]["batch_size"] == batch_size
        assert isinstance(payload[1]["batch_size"], str)
        assert payload[1]["countdown"] == 5
