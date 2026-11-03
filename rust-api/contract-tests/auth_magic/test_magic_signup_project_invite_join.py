# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Oracle: magic sign-up with an accepted project invite (PIDASHCONV-1686).

Pins the magic arm of ``process_workspace_project_invitations``: a fresh
email carrying an accepted project invite completes magic sign-up, joins
the invite workspace (role clamped to 5/15) *and* the invite project
with the invite's ``project_id``, and the invite soft-deletes. FAILS on
pre-fix Rust (500, partial writes); PB-1008 retired.
"""

import uuid

import pytest

from _harness import redis as redis_helper

from .conftest import (
    SESSION_COOKIE,
    csrf_client,
    fresh_email,
    generate,
    location_query,
    post_form,
    session_rows,
)

SIGN_UP = "/auth/magic-sign-up/"
GENERATE = "/auth/magic-generate/"


def _seed_token(client, rdb, email: str) -> str:
    redis_helper.clear_magic(rdb, email)
    assert generate(client, GENERATE, email).status_code == 200
    return str(redis_helper.read_magic(rdb, email)["token"])


@pytest.mark.contract
class TestMagicSignUpProjectInviteJoin:
    def test_success_joins_invite_workspace_and_project(self, db, seed, rdb):
        owner = seed.user(email=fresh_email("own1686"))
        workspace = seed.workspace(owner["id"])
        project_id = seed.project(workspace["id"])
        email = fresh_email("mg1686")
        iid = str(uuid.uuid4())
        with db.cursor() as cur:
            cur.execute(
                "INSERT INTO project_member_invites (id, created_at, updated_at,"
                " project_id, workspace_id, email, accepted, token, role)"
                " VALUES (%s, now(), now(), %s, %s, %s, true, %s, %s)",
                (iid, project_id, workspace["id"], email, f"tok-{iid[:8]}", 20),
            )
        try:
            client = csrf_client()
            token = _seed_token(client, rdb, email)
            resp = post_form(client, SIGN_UP, {"email": email, "code": token})

            assert resp.status_code == 302, resp.text
            assert "error_code" not in location_query(resp)
            assert SESSION_COOKIE in resp.cookies

            with db.cursor() as cur:
                cur.execute("SELECT id FROM users WHERE email = %s", (email,))
                row = cur.fetchone()
            assert row is not None, "sign-up created the user"
            user_id = str(row[0])
            seed.track("users", "id", user_id)
            assert session_rows(db, user_id) == 1

            with db.cursor() as cur:
                cur.execute(
                    "SELECT workspace_id::text, role FROM workspace_members"
                    " WHERE member_id = %s",
                    (user_id,),
                )
                members = cur.fetchall()
                cur.execute(
                    "SELECT project_id::text, workspace_id::text, role"
                    " FROM project_members WHERE member_id = %s",
                    (user_id,),
                )
                project_rows = cur.fetchall()
                cur.execute(
                    "SELECT accepted, deleted_at FROM project_member_invites"
                    " WHERE id = %s",
                    (iid,),
                )
                invite = cur.fetchone()
            assert [(w, r) for w, r in members] == [(workspace["id"], 15)], (
                f"invite workspace joined (role 20 clamps to 15): {members}"
            )
            assert [(p, w, r) for p, w, r in project_rows] == [
                (project_id, workspace["id"], 15)
            ], f"invite project joined with its project_id: {project_rows}"
            assert invite is not None and invite[0] is True, invite
            assert invite[1] is not None, "consumed invite soft-deletes"
        finally:
            with db.cursor() as cur:
                cur.execute(
                    "DELETE FROM project_member_invites WHERE id = %s", (iid,)
                )
                cur.execute(
                    "DELETE FROM project_members WHERE member_id IN"
                    " (SELECT id FROM users WHERE email = %s)",
                    (email,),
                )
                cur.execute(
                    "DELETE FROM workspace_members WHERE member_id IN"
                    " (SELECT id FROM users WHERE email = %s)",
                    (email,),
                )
