"""tasks_mail oracle: user/project membership mail execution (PIDASHCONV-1104).

Executes the five membership mail tasks on the Django worker and pins
their effects: SMTP delivery (recipient, subject, body) plus the database
writes. The Rust replay twin is ``test_rust_replay_user_project.py``,
which runs the same assertions against the Rust worker; together they
prove the oracle replay matches Rust (rows plus captured mail).

The oracle worker must boot with these modules in ``-I``
(``_harness/oracle_worker_includes.txt``); the stock worker rejects
them as unregistered (PDASHOSS01-292).
"""

import re
import uuid
from email import message_from_bytes

from psycopg.rows import dict_row

from _harness import broker_probe, celery_wire
from _harness import seed as seed_helpers

M = "pi_dash.bgtasks"
SITE = "example.com"

ACTIVATION = f"{M}.user_activation_email_task.user_activation_email"
DEACTIVATION = f"{M}.user_deactivation_email_task.user_deactivation_email"
PROJECT_ADD_USER = f"{M}.project_add_user_email_task.project_add_user_email"
WORKSPACE_INVITATION = f"{M}.workspace_invitation_task.workspace_invitation"
PROJECT_INVITATION = f"{M}.project_invitation_task.project_invitation"


def _row(conn, table, row_id):
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(f"SELECT * FROM {table} WHERE id = %s", (str(row_id),))
        return cur.fetchone()


def _parse(raw):
    # The sink stores DATA decoded as UTF-8; our mails are 8bit/base64
    # UTF-8 throughout, so re-encoding restores the original bytes
    # exactly (parsing the str directly mangles 8bit non-ASCII on
    # get_payload(decode=True)).
    return message_from_bytes(raw.encode("utf-8"))


def _subject(delivered):
    # Long subjects arrive folded (CRLF + LWSP); unfold before comparing.
    return re.sub(r"\r\n(?=[ \t])", "", delivered["subject"])


def _text_part(raw):
    """Decoded text/plain payload of the delivered multipart mail."""
    parsed = _parse(raw)
    for part in parsed.walk():
        if part.get_content_type() == "text/plain":
            payload = part.get_payload(decode=True)
            # SMTP flattens to CRLF; the stored message keeps Django's LF.
            return payload.decode("utf-8").replace("\r\n", "\n")
    raise AssertionError("delivered mail has no text/plain part")


def _html_part(raw):
    """Decoded text/html payload of the delivered multipart mail."""
    parsed = _parse(raw)
    for part in parsed.walk():
        if part.get_content_type() == "text/html":
            payload = part.get_payload(decode=True)
            return payload.decode("utf-8").replace("\r\n", "\n")
    raise AssertionError("delivered mail has no text/html part")


def _part_ctes(raw):
    """Content-Transfer-Encoding per part of the delivered mail."""
    parsed = _parse(raw)
    ctes = {}
    for part in parsed.walk():
        ctype = part.get_content_type()
        if ctype in ("text/plain", "text/html"):
            ctes[ctype] = part.get("Content-Transfer-Encoding")
    return ctes


def _only_delivery_to(sink, rcpt, what):
    """Wait for our mail, drain the broker, and return the single delivery.

    Recipient-pinned (our seeds are unique per test), so background beat
    mail to other recipients cannot flake the wait; the drain after the
    first sighting means a duplicate send of our own mail fails the
    exact-count pin instead of racing it (same shape as the stack
    redelivery test).
    """
    from _harness.db import wait_for_condition as wait_for

    def _mine():
        mine = [m for m in sink.snapshot() if m["rcpt_tos"] == [rcpt]]
        return mine or None

    wait_for(_mine, what=what)
    broker_probe.wait_for_queue_drain(what=f"broker drained after {what}")
    mine = [m for m in sink.snapshot() if m["rcpt_tos"] == [rcpt]]
    assert len(mine) == 1, f"expected exactly one mail to {rcpt}, got {len(mine)}"
    return mine[0]


def test_activation_delivers_and_writes_no_rows(db_conn, broker_url, smtp_sink):
    """user_activation_email: subject or-chain, profile link, no DB writes."""
    user = seed_helpers.user(db_conn, "contract-activate", first_name="Ada")
    before = _row(db_conn, "users", user["id"])
    smtp_sink.clear()

    celery_wire.publish(ACTIVATION, args=[SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "activation mail")

    assert _subject(delivered) == "Ada has been activated on Pi Dash"
    assert f"{SITE}/profile" in _html_part(delivered["raw"])
    assert _row(db_conn, "users", user["id"]) == before


def test_deactivation_delivers_and_writes_no_rows(db_conn, broker_url, smtp_sink):
    """user_deactivation_email: subject or-chain, login link, no DB writes."""
    user = seed_helpers.user(db_conn, "contract-deactivate", first_name="Ada")
    before = _row(db_conn, "users", user["id"])
    smtp_sink.clear()

    celery_wire.publish(DEACTIVATION, args=[SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "deactivation mail")

    assert _subject(delivered) == "Ada has been deactivated on Pi Dash"
    assert f"{SITE}/login" in _html_part(delivered["raw"])
    assert _row(db_conn, "users", user["id"]) == before


def test_activation_falls_through_to_display_name(db_conn, broker_url, smtp_sink):
    """user_activation_email: empty first_name falls through to display_name."""
    user = seed_helpers.user(db_conn, "contract-activate-fallback")
    assert user["first_name"] == ""
    smtp_sink.clear()

    celery_wire.publish(ACTIVATION, args=[SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "activation fallback mail")

    assert _subject(delivered) == (
        f"{user['display_name']} has been activated on Pi Dash"
    )


def test_project_add_user_delivers_and_writes_no_rows(db_conn, broker_url, smtp_sink):
    """project_add_user_email: fixed subject, project link, no DB writes."""
    tag = uuid.uuid4().hex[:8]
    invitor = seed_helpers.user(db_conn, f"contract-adduser-invitor-{tag}")
    member_user = seed_helpers.user(db_conn, f"contract-adduser-member-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"adduser{tag}", invitor["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    member = seed_helpers.project_member(
        db_conn, project["id"], workspace["id"], member_user["id"]
    )
    before = (
        _row(db_conn, "project_members", member["id"]),
        _row(db_conn, "projects", project["id"]),
        _row(db_conn, "workspaces", workspace["id"]),
    )
    smtp_sink.clear()

    celery_wire.publish(
        PROJECT_ADD_USER, args=[SITE, str(member["id"]), str(invitor["id"])]
    )
    delivered = _only_delivery_to(smtp_sink, member_user["email"], "project_add_user mail")

    assert _subject(delivered) == "You have been invited to a Pi Dash project"
    expected_url = (
        f"{SITE}/{workspace['slug']}/projects/{project['id']}/issues"
    )
    assert expected_url in _text_part(delivered["raw"])
    after = (
        _row(db_conn, "project_members", member["id"]),
        _row(db_conn, "projects", project["id"]),
        _row(db_conn, "workspaces", workspace["id"]),
    )
    assert after == before


def test_workspace_invitation_stores_message_and_mails(db_conn, broker_url, smtp_sink):
    """workspace_invitation: message stored + audit wipe + mail delivery.

    The invite row keeps ``message`` (the plain-text body), and the
    worker's ``invite.save()`` NULLs both audit columns (BaseModel.save
    with no current user) — the invite is seeded with created_by so the
    wipe is observable.
    """
    tag = uuid.uuid4().hex[:8]
    inviter = seed_helpers.user(db_conn, f"contract-ws-inviter-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"wsinv{tag}", inviter["id"])
    invite = seed_helpers.workspace_invite(
        db_conn, workspace["id"], created_by_id=str(inviter["id"])
    )
    assert invite["created_by_id"] == inviter["id"]
    before = _row(db_conn, "workspace_member_invites", invite["id"])
    smtp_sink.clear()

    celery_wire.publish(
        WORKSPACE_INVITATION,
        args=[invite["email"], str(workspace["id"]), invite["token"], SITE, inviter["email"]],
    )
    delivered = _only_delivery_to(smtp_sink, invite["email"], "workspace invitation mail")

    assert _subject(delivered) == (
        f"{inviter['display_name']} has invited you to join them in "
        f"{workspace['name']} on Pi Dash"
    )
    after = _row(db_conn, "workspace_member_invites", invite["id"])
    assert after["message"] is not None
    assert after["message"] == _text_part(delivered["raw"])
    assert after["created_by_id"] is None
    assert after["updated_by_id"] is None
    assert after["updated_at"] > before["updated_at"]
    rest_before = {
        k: v for k, v in before.items()
        if k not in ("message", "created_by_id", "updated_by_id", "updated_at")
    }
    rest_after = {
        k: v for k, v in after.items()
        if k not in ("message", "created_by_id", "updated_by_id", "updated_at")
    }
    assert rest_after == rest_before
    for needle in (
        f"invitation_id={invite['id']}",
        f"slug={workspace['slug']}",
        f"token={invite['token']}",
    ):
        assert needle in _html_part(delivered["raw"])


def test_project_invitation_stores_message_and_mails(db_conn, broker_url, smtp_sink):
    """project_invitation: message stored + audit wipe + mail delivery.

    Twin of the workspace test: the invite row keeps ``message``, the
    worker's ``invite.save()`` NULLs both audit columns, and the mail
    carries the project link.
    """
    tag = uuid.uuid4().hex[:8]
    inviter = seed_helpers.user(db_conn, f"contract-pj-inviter-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"pjinv{tag}", inviter["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    invite = seed_helpers.project_invite(
        db_conn, project["id"], workspace["id"], created_by_id=str(inviter["id"])
    )
    assert invite["created_by_id"] == inviter["id"]
    before = _row(db_conn, "project_member_invites", invite["id"])
    smtp_sink.clear()

    celery_wire.publish(
        PROJECT_INVITATION,
        args=[invite["email"], str(project["id"]), invite["token"], SITE, inviter["email"]],
    )
    delivered = _only_delivery_to(smtp_sink, invite["email"], "project invitation mail")

    assert _subject(delivered) == (
        f"{inviter['display_name']} invited you to join "
        f"{project['name']} on Pi Dash"
    )
    assert _part_ctes(delivered["raw"]) == {
        "text/plain": "8bit",
        "text/html": "quoted-printable",
    }
    after = _row(db_conn, "project_member_invites", invite["id"])
    assert after["message"] is not None
    assert after["message"] == _text_part(delivered["raw"])
    assert after["created_by_id"] is None
    assert after["updated_by_id"] is None
    assert after["updated_at"] > before["updated_at"]
    rest_before = {
        k: v for k, v in before.items()
        if k not in ("message", "created_by_id", "updated_by_id", "updated_at")
    }
    rest_after = {
        k: v for k, v in after.items()
        if k not in ("message", "created_by_id", "updated_by_id", "updated_at")
    }
    assert rest_after == rest_before
    for needle in (
        f"invitation_id={invite['id']}",
        f"email={invite['email']}",
        f"slug={workspace['slug']}",
        f"project_id={project['id']}",
    ):
        assert needle in _html_part(delivered["raw"])
