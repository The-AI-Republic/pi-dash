"""Shared fixtures for the v1_projects contract suite (domain D-19).

Two isolated workspaces per session run (unique slugs, so reruns never
collide and no teardown is needed):

- ``ws_a`` — owned by ``owner`` (workspace ADMIN); ``member`` is a workspace
  MEMBER + project ADMIN on ``project``; ``guest`` is a workspace MEMBER with
  no project membership; ``outsider`` belongs to no workspace.
- ``ws_b`` — owned by ``owner_b``; used only for tenant-isolation cases.

Every test module authenticates with its own API key (``keys[<module>]``) so
the backend's per-key rate throttle never couples modules together.
"""

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402


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
    guest = db.create_user(conn, tag + "g", first_name="CtGuest")
    outsider = db.create_user(conn, tag + "x", first_name="CtOutsider")
    owner_b = db.create_user(conn, tag + "b", first_name="CtOwnerB")

    ws_a = db.create_workspace(conn, tag + "a", owner["id"])
    ws_b = db.create_workspace(conn, tag + "b", owner_b["id"])
    db.add_workspace_member(conn, ws_a["id"], owner["id"], db.ADMIN)
    db.add_workspace_member(conn, ws_a["id"], member["id"], db.MEMBER)
    db.add_workspace_member(conn, ws_a["id"], guest["id"], db.MEMBER)
    db.add_workspace_member(conn, ws_b["id"], owner_b["id"], db.ADMIN)

    project = db.create_project(conn, ws_a["id"], tag + "p", created_by_id=owner["id"])
    db.add_project_member(conn, ws_a["id"], project["id"], owner["id"], db.ADMIN)
    db.add_project_member(conn, ws_a["id"], project["id"], member["id"], db.ADMIN)

    project_b = db.create_project(conn, ws_b["id"], tag + "q", created_by_id=owner_b["id"])
    db.add_project_member(conn, ws_b["id"], project_b["id"], owner_b["id"], db.ADMIN)

    keys = {}
    for module in ("projects", "members", "invites", "user", "states", "estimates"):
        keys[module] = db.create_api_token(conn, owner["id"], tag + module[0])["token"]
    keys["member_self"] = db.create_api_token(conn, member["id"], tag + "ms")["token"]
    keys["guest"] = db.create_api_token(conn, guest["id"], tag + "g")["token"]
    keys["outsider"] = db.create_api_token(conn, outsider["id"], tag + "x")["token"]
    keys["owner_b"] = db.create_api_token(conn, owner_b["id"], tag + "b")["token"]

    return {
        "tag": tag,
        "owner": owner,
        "member": member,
        "guest": guest,
        "outsider": outsider,
        "owner_b": owner_b,
        "ws_a": ws_a,
        "ws_b": ws_b,
        "project": project,
        "project_b": project_b,
        "keys": keys,
    }

