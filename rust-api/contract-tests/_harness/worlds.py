# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""SQL factories mirroring the Django ``world`` fixture.

``build_world`` creates a workspace with users at every role (admin 20,
member 15, guest 5, plus a workspace-member-but-project-outsider and a
cross-tenant user in a second workspace), with random email/slug suffixes so
tests never collide. Passwords are Django-compatible PBKDF2 hashes computed
with ``hashlib`` alone — no Django import needed.
"""

from __future__ import annotations

import base64
import hashlib
import secrets
import uuid
from dataclasses import dataclass

from _harness.db import db_cursor

ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST = 20, 15, 5

# Keep in sync with the server's hasher (Django 4.x default). The iteration
# count travels inside the hash string, so this verifies against any server
# that still reads the ``pbkdf2_sha256`` format.
_PBKDF2_ITERATIONS = 720000
TEST_PASSWORD = "contract-test-pass-123"


def password_hash(password: str = TEST_PASSWORD) -> str:
    salt = secrets.token_hex(12)
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), _PBKDF2_ITERATIONS)
    return f"pbkdf2_sha256${_PBKDF2_ITERATIONS}${salt}${base64.b64encode(dk).decode().strip()}"


@dataclass
class SeededUser:
    id: str
    email: str
    password_hash: str = ""


@dataclass
class SeededWorkspace:
    id: str
    slug: str


@dataclass
class World:
    ws: SeededWorkspace
    other_ws: SeededWorkspace
    admin: SeededUser
    member: SeededUser
    guest: SeededUser
    outsider: SeededUser
    other_user: SeededUser


def _tag() -> str:
    return uuid.uuid4().hex[:8]


def create_user(email: str | None = None, password: str = TEST_PASSWORD) -> SeededUser:
    email = email or f"ct-{_tag()}@e.com"
    uid = str(uuid.uuid4())
    hashed = password_hash(password)
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO users (id, username, email, password, first_name, last_name,
                avatar, date_joined, created_at, updated_at, last_location,
                created_location, is_superuser, is_managed, is_password_expired,
                is_active, is_staff, is_email_verified, is_password_autoset, token,
                user_timezone, last_login_ip, last_logout_ip, last_login_medium,
                last_login_uagent, display_name, is_email_valid, is_bot,
                is_password_reset_required)
               VALUES (%s,%s,%s,%s,'Contract','User','',now(),now(),now(),'','',
                false,false,false,true,false,true,false,'','UTC','','','email','',
                'Contract',true,false,false)""",
            (uid, email, email, hashed),
        )
    return SeededUser(id=uid, email=email, password_hash=hashed)


def create_workspace(owner: SeededUser, slug: str | None = None) -> SeededWorkspace:
    slug = slug or f"ctws-{_tag()}"
    wid = str(uuid.uuid4())
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO workspaces (id, created_at, updated_at, name, slug,
                owner_id, timezone, background_color)
               VALUES (%s,now(),now(),%s,%s,%s,'UTC','#ffffff')""",
            (wid, slug, slug, owner.id),
        )
    return SeededWorkspace(id=wid, slug=slug)


def add_member(ws: SeededWorkspace, user: SeededUser, role: int) -> None:
    with db_cursor() as cur:
        cur.execute(
            """INSERT INTO workspace_members (id, created_at, updated_at, workspace_id,
                member_id, role, view_props, default_props, issue_props, is_active,
                explored_features, getting_started_checklist, tips)
               VALUES (%s,now(),now(),%s,%s,%s,'{}','{}','{}',true,'{}','{}','{}')""",
            (str(uuid.uuid4()), ws.id, user.id, role),
        )


def ensure_instance() -> None:
    """The sign-in flow refuses to authenticate until an Instance is set up."""
    with db_cursor() as cur:
        cur.execute("SELECT count(*) FROM instances")
        if cur.fetchone()[0] > 0:
            return
        cur.execute(
            """INSERT INTO instances (id, created_at, updated_at, instance_name,
                instance_id, current_version, last_checked_at, is_setup_done,
                is_telemetry_enabled, is_support_required, edition, domain,
                is_signup_screen_visited, is_verified, is_test,
                is_current_version_deprecated)
               VALUES (%s,now(),now(),'contract','contract-1','1.0',now(),true,
                false,false,'community','',false,false,false,false)""",
            (str(uuid.uuid4()),),
        )


def build_world() -> World:
    ensure_instance()
    admin = create_user()
    member = create_user()
    guest = create_user()
    outsider = create_user()
    other_user = create_user()

    ws = create_workspace(admin)
    add_member(ws, admin, ROLE_ADMIN)
    add_member(ws, member, ROLE_MEMBER)
    add_member(ws, guest, ROLE_GUEST)
    add_member(ws, outsider, ROLE_MEMBER)

    other_ws = create_workspace(other_user)
    add_member(other_ws, other_user, ROLE_ADMIN)
    return World(
        ws=ws,
        other_ws=other_ws,
        admin=admin,
        member=member,
        guest=guest,
        outsider=outsider,
        other_user=other_user,
    )
