"""Row factories mirroring Django-side creation semantics.

Everything is plain SQL (signals do not run on raw INSERT, so signal-created
rows such as ``user_notification_preferences`` are inserted explicitly with
the same defaults the ``post_save`` receiver uses: all five flags True).

Passwords use the Django ``pbkdf2_sha256`` storage format computed with
``hashlib`` so ``check_password`` verifies them; no Django import needed.
"""
import base64
import hashlib
import secrets
import uuid

from . import db
from .config import database_url

TEST_PASSWORD = "Contract-Test-Pw-123!"

# WorkspaceMember.role values (pi_dash.app.permissions.ROLE).
ROLE_ADMIN = 20
ROLE_MEMBER = 15
ROLE_GUEST = 5


def make_password(password: str, iterations: int = 600_000) -> str:
    salt = secrets.token_hex(12)
    dk = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), iterations)
    digest = base64.b64encode(dk).decode("ascii").strip()
    return f"pbkdf2_sha256${iterations}${salt}${digest}"


def create_user(email: str, password: str = TEST_PASSWORD, *, user_timezone: str = "UTC") -> dict:
    """Insert a users row plus its signal-created preference row. Returns the user."""
    uid = uuid.uuid4()
    username = f"ctn-{uid.hex[:12]}"
    with db.connect(database_url()) as conn, conn.cursor() as cur:
        cur.execute(
            "INSERT INTO users (id, password, username, email, first_name, last_name, avatar,"
            " date_joined, created_at, updated_at, last_location, created_location,"
            " is_superuser, is_managed, is_password_expired, is_active, is_staff,"
            " is_email_verified, is_password_autoset, token, user_timezone,"
            " last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,"
            " is_bot, display_name, is_email_valid, is_password_reset_required)"
            " VALUES (%s,%s,%s,%s,'','','', now(), now(), now(), '','',"
            " false,false,false,true,false,false,false,'',%s,"
            " '','','email','',false,'',false,false)",
            (uid, make_password(password), username, email, user_timezone),
        )
        # Mirror create_user_notification post_save receiver (non-bot users).
        cur.execute(
            "INSERT INTO user_notification_preferences (id, property_change, state_change,"
            " comment, mention, issue_completed, user_id, created_at, updated_at)"
            " VALUES (%s, true, true, true, true, true, %s, now(), now())",
            (uuid.uuid4(), uid),
        )
    return {"id": str(uid), "email": email, "password": password, "username": username}


def create_workspace(slug: str, owner_id: str, name: str = "Contract WS") -> dict:
    wid = uuid.uuid4()
    db.execute(database_url(), 
        "INSERT INTO workspaces (id, name, slug, owner_id, timezone, background_color,"
        " created_at, updated_at) VALUES (%s,%s,%s,%s,'UTC','#ffffff', now(), now())",
        (wid, name, slug, owner_id),
    )
    return {"id": str(wid), "slug": slug}


def add_member(workspace_id: str, user_id: str, role: int = ROLE_ADMIN) -> dict:
    mid = uuid.uuid4()
    db.execute(database_url(), 
        "INSERT INTO workspace_members (id, role, member_id, workspace_id, view_props,"
        " default_props, issue_props, is_active, explored_features,"
        " getting_started_checklist, tips, created_at, updated_at)"
        " VALUES (%s,%s,%s,%s,'{}','{}','{}', true,'{}','{}','{}', now(), now())",
        (mid, role, user_id, workspace_id),
    )
    return {"id": str(mid)}


def create_notification(
    workspace_id: str,
    receiver_id: str,
    *,
    entity_name: str = "issue",
    title: str = "Contract notification",
    sender: str = "issue.created",
    triggered_by_id=None,
    entity_identifier=None,
    read_at=None,
    snoozed_till=None,
    archived_at=None,
    data=None,
    message=None,
) -> dict:
    """Insert a notifications row. Timestamps accept ISO strings or None."""
    import json as _json

    nid = uuid.uuid4()
    db.execute(database_url(), 
        "INSERT INTO notifications (id, entity_name, title, message_html, sender,"
        " workspace_id, receiver_id, triggered_by_id, entity_identifier,"
        " data, message, read_at, snoozed_till, archived_at, created_at, updated_at)"
        " VALUES (%s,%s,%s,'<p></p>',%s,%s,%s,%s,%s,%s::jsonb,%s::jsonb,%s,%s,%s, now(), now())",
        (
            nid,
            entity_name,
            title,
            sender,
            workspace_id,
            receiver_id,
            triggered_by_id,
            entity_identifier,
            _json.dumps(data) if data is not None else None,
            _json.dumps(message) if message is not None else None,
            read_at,
            snoozed_till,
            archived_at,
        ),
    )
    return {"id": str(nid)}


def delete_notification(notification_id: str) -> None:
    db.execute(database_url(), "DELETE FROM notifications WHERE id = %s", (notification_id,))


def cleanup_run(tag: str) -> None:
    """Best-effort removal of every row seeded under a run tag.

    Slugs/emails carry the tag; children first. Sessions store user_id as text.
    """
    like = f"%{tag}%"
    user_ids = [r["id"] for r in db.fetchall(database_url(), "SELECT id FROM users WHERE email LIKE %s", (like,))]
    ws_ids = [r["id"] for r in db.fetchall(database_url(), "SELECT id FROM workspaces WHERE slug LIKE %s", (like,))]
    if user_ids or ws_ids:
        db.execute(database_url(), 
            "DELETE FROM notifications WHERE receiver_id = ANY(%s) OR workspace_id = ANY(%s)",
            (user_ids or [], ws_ids or []),
        )
    # Device rows reference sessions; drop them before the sessions themselves.
    db.execute(database_url(), 
        "DELETE FROM device_sessions WHERE session_id IN"
        " (SELECT session_key FROM sessions WHERE user_id = ANY(%s))",
        ([str(u) for u in user_ids] or [],),
    )
    for uid in user_ids:
        db.execute(database_url(), "DELETE FROM sessions WHERE user_id = %s", (str(uid),))
    # First login creates a Profile row for the user (post-auth workflow).
    # Swept twice: a late login side-effect must never block user removal.
    db.execute(database_url(), "DELETE FROM profiles WHERE user_id = ANY(%s)", (user_ids or [],))
    db.execute(database_url(), 
        "DELETE FROM user_notification_preferences WHERE user_id = ANY(%s)", (user_ids or [],)
    )
    db.execute(database_url(), "DELETE FROM workspace_members WHERE member_id = ANY(%s)", (user_ids or [],))
    db.execute(database_url(), "DELETE FROM workspaces WHERE id = ANY(%s)", (ws_ids or [],))
    db.execute(database_url(), "DELETE FROM profiles WHERE user_id = ANY(%s)", (user_ids or [],))
    db.execute(database_url(), "DELETE FROM users WHERE id = ANY(%s)", (user_ids or [],))
