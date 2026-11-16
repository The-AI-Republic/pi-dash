"""tasks_mail Rust replay: user/project membership mail (PIDASHCONV-1104).

Rust-worker twin of ``test_user_project_mail.py``: the same six
assertions run against ``pidash-api worker`` serving ``rust_job_queue``
(the Django worker stays stopped in this job). Each task must execute
LOCALLY — the row settles, no message for the task lands on the AMQP
broker (the per-task pin from ``test_rust_replay.py``, reused here,
never forked), the SMTP sink gets the delivery, and the invite rows
change exactly as the oracle pins (message stored, audit columns
wiped).
"""

import re
import uuid
from email import message_from_bytes

from psycopg.rows import dict_row

from _harness import rust_queue
from _harness import seed as seed_helpers

# The broker pin lives in the stack/send replay module; reuse it so the
# per-task drain-and-restore semantics cannot drift between files.
from tasks_mail.test_rust_replay import _drain_matching

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


def _publish_and_observe_local_args(task, args):
    """Publish one positional-args job and prove it executes locally.

    Same contract as ``test_rust_replay._publish_and_observe_local``
    (settle + no broker message for the task), but with positional
    args: every membership mail task takes its inputs positionally.
    """
    celery_id = rust_queue.publish(task, args=args, kwargs={})
    rust_queue.wait_for_settled(
        celery_id, what=f"locally executed mail task settled ({task})"
    )
    arrived = _drain_matching(task)
    assert arrived == [], (
        f"locally owned {task} published to the broker ({len(arrived)} message(s))"
    )
    return celery_id


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
    """Wait for our mail, drain the Rust queue, return the single delivery.

    Recipient-pinned (our seeds are unique per test). The Rust-queue
    drain after the first sighting means a duplicate send of our own
    mail fails the exact-count pin instead of racing it.
    """
    from _harness.db import wait_for_condition as wait_for

    def _mine():
        mine = [m for m in sink.snapshot() if m["rcpt_tos"] == [rcpt]]
        return mine or None

    wait_for(_mine, what=what)
    rust_queue.wait_for_queue_drain(what=f"rust queue drained after {what}")
    mine = [m for m in sink.snapshot() if m["rcpt_tos"] == [rcpt]]
    assert len(mine) == 1, f"expected exactly one mail to {rcpt}, got {len(mine)}"
    return mine[0]


def _rest(row, excluded):
    return {k: v for k, v in row.items() if k not in excluded}


def test_rust_activation_delivers_and_writes_no_rows(db_conn, smtp_sink):
    """Rust: user_activation_email delivers; the user row is untouched."""
    user = seed_helpers.user(db_conn, "contract-rust-activate", first_name="Ada")
    before = _row(db_conn, "users", user["id"])
    smtp_sink.clear()

    _publish_and_observe_local_args(ACTIVATION, [SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "rust activation mail")

    assert _subject(delivered) == "Ada has been activated on Pi Dash"
    assert f"{SITE}/profile" in _html_part(delivered["raw"])
    assert _row(db_conn, "users", user["id"]) == before


def test_rust_deactivation_delivers_and_writes_no_rows(db_conn, smtp_sink):
    """Rust: user_deactivation_email delivers; the user row is untouched."""
    user = seed_helpers.user(db_conn, "contract-rust-deactivate", first_name="Ada")
    before = _row(db_conn, "users", user["id"])
    smtp_sink.clear()

    _publish_and_observe_local_args(DEACTIVATION, [SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "rust deactivation mail")

    assert _subject(delivered) == "Ada has been deactivated on Pi Dash"
    assert f"{SITE}/login" in _html_part(delivered["raw"])
    assert _row(db_conn, "users", user["id"]) == before


def test_rust_activation_falls_through_to_display_name(db_conn, smtp_sink):
    """Rust: empty first_name falls through to display_name in the subject."""
    user = seed_helpers.user(db_conn, "contract-rust-activate-fallback")
    assert user["first_name"] == ""
    smtp_sink.clear()

    _publish_and_observe_local_args(ACTIVATION, [SITE, str(user["id"])])
    delivered = _only_delivery_to(smtp_sink, user["email"], "rust activation fallback")

    assert _subject(delivered) == (
        f"{user['display_name']} has been activated on Pi Dash"
    )


def test_rust_project_add_user_delivers_and_writes_no_rows(db_conn, smtp_sink):
    """Rust: project_add_user_email delivers; no rows change."""
    tag = uuid.uuid4().hex[:8]
    invitor = seed_helpers.user(db_conn, f"contract-rust-adduser-invitor-{tag}")
    member_user = seed_helpers.user(db_conn, f"contract-rust-adduser-member-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"rustadduser{tag}", invitor["id"])
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

    _publish_and_observe_local_args(
        PROJECT_ADD_USER, [SITE, str(member["id"]), str(invitor["id"])]
    )
    delivered = _only_delivery_to(
        smtp_sink, member_user["email"], "rust project_add_user mail"
    )

    assert _subject(delivered) == "You have been invited to a Pi Dash project"
    expected_url = f"{SITE}/{workspace['slug']}/projects/{project['id']}/issues"
    assert expected_url in _text_part(delivered["raw"])
    after = (
        _row(db_conn, "project_members", member["id"]),
        _row(db_conn, "projects", project["id"]),
        _row(db_conn, "workspaces", workspace["id"]),
    )
    assert after == before


def test_rust_workspace_invitation_stores_message_and_mails(db_conn, smtp_sink):
    """Rust: workspace_invitation stores message, wipes audit cols, mails."""
    tag = uuid.uuid4().hex[:8]
    inviter = seed_helpers.user(db_conn, f"contract-rust-ws-inviter-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"rustwsinv{tag}", inviter["id"])
    invite = seed_helpers.workspace_invite(
        db_conn, workspace["id"], created_by_id=str(inviter["id"])
    )
    assert invite["created_by_id"] == inviter["id"]
    before = _row(db_conn, "workspace_member_invites", invite["id"])
    smtp_sink.clear()

    _publish_and_observe_local_args(
        WORKSPACE_INVITATION,
        [invite["email"], str(workspace["id"]), invite["token"], SITE, inviter["email"]],
    )
    delivered = _only_delivery_to(
        smtp_sink, invite["email"], "rust workspace invitation mail"
    )

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
    excluded = ("message", "created_by_id", "updated_by_id", "updated_at")
    assert _rest(after, excluded) == _rest(before, excluded)
    for needle in (
        f"invitation_id={invite['id']}",
        f"slug={workspace['slug']}",
        f"token={invite['token']}",
    ):
        assert needle in _html_part(delivered["raw"])


def test_rust_project_invitation_stores_message_and_mails(db_conn, smtp_sink):
    """Rust: project_invitation stores message, wipes audit cols, mails."""
    tag = uuid.uuid4().hex[:8]
    inviter = seed_helpers.user(db_conn, f"contract-rust-pj-inviter-{tag}")
    workspace = seed_helpers.workspace(db_conn, f"rustpjinv{tag}", inviter["id"])
    project = seed_helpers.project(db_conn, workspace["id"])
    invite = seed_helpers.project_invite(
        db_conn, project["id"], workspace["id"], created_by_id=str(inviter["id"])
    )
    assert invite["created_by_id"] == inviter["id"]
    before = _row(db_conn, "project_member_invites", invite["id"])
    smtp_sink.clear()

    _publish_and_observe_local_args(
        PROJECT_INVITATION,
        [invite["email"], str(project["id"]), invite["token"], SITE, inviter["email"]],
    )
    delivered = _only_delivery_to(
        smtp_sink, invite["email"], "rust project invitation mail"
    )

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
    excluded = ("message", "created_by_id", "updated_by_id", "updated_at")
    assert _rest(after, excluded) == _rest(before, excluded)
    for needle in (
        f"invitation_id={invite['id']}",
        f"email={invite['email']}",
        f"slug={workspace['slug']}",
        f"project_id={project['id']}",
    ):
        assert needle in _html_part(delivered["raw"])


# PIDASHCONV-1286: Rust-worker twin of
# ``test_user_project_mail.test_workspace_invitation_non_ascii_headers_encoded``:
# the same RFC 2047 Subject/From + punycoded IDN To wire pins, executed
# locally by ``pidash-api worker``.
NONASCII_FROM = "Équipe Pi Dash <team@x.com>"


def _override_email_from(conn, value):
    """Point EMAIL_FROM at ``value`` via its InstanceConfiguration row.

    EMAIL_* resolve from ``instance_configurations`` with env fallback on
    both backends; returns the previous live value (None when no row is
    set) so the caller can restore it.
    """
    with conn.cursor() as cur:
        cur.execute(
            "SELECT value FROM instance_configurations "
            "WHERE key = 'EMAIL_FROM' AND deleted_at IS NULL"
        )
        row = cur.fetchone()
        if row is None:
            seed_helpers.insert_row(
                conn,
                "instance_configurations",
                {
                    "key": "EMAIL_FROM",
                    "value": value,
                    "category": "EMAIL",
                    "is_encrypted": False,
                },
            )
        else:
            cur.execute(
                "UPDATE instance_configurations SET value = %s, updated_at = now() "
                "WHERE key = 'EMAIL_FROM' AND deleted_at IS NULL",
                (value,),
            )
    return row[0] if row else None


def _restore_email_from(conn, prev):
    with conn.cursor() as cur:
        if prev is None:
            cur.execute("DELETE FROM instance_configurations WHERE key = 'EMAIL_FROM'")
        else:
            cur.execute(
                "UPDATE instance_configurations SET value = %s, updated_at = now() "
                "WHERE key = 'EMAIL_FROM' AND deleted_at IS NULL",
                (prev,),
            )


def _only_delivery_matching(sink, needle, what):
    """_only_delivery_to for an address whose wire form is asserted later.

    The recipient pin must not assume the punycoded form: pre-fix, RCPT
    TO carries the raw IDN domain, and the test should fail on the byte
    asserts below (showing both forms) instead of timing out this wait.
    """
    from _harness.db import wait_for_condition as wait_for

    def _mine():
        mine = [m for m in sink.snapshot() if any(needle in r for r in m["rcpt_tos"])]
        return mine or None

    wait_for(_mine, what=what)
    rust_queue.wait_for_queue_drain(what=f"rust queue drained after {what}")
    mine = [m for m in sink.snapshot() if any(needle in r for r in m["rcpt_tos"])]
    assert len(mine) == 1, f"expected exactly one mail matching {needle}, got {len(mine)}"
    return mine[0]


def test_rust_workspace_invitation_non_ascii_headers_encoded(db_conn, smtp_sink):
    """workspace_invitation on the Rust worker: encoded wire headers + punycode."""
    tag = uuid.uuid4().hex[:8]
    inviter = seed_helpers.user(db_conn, f"contract-rust-hdr-{tag}", first_name="José")
    workspace = seed_helpers.workspace(db_conn, f"rswshdr{tag}", inviter["id"], name="Café")
    rcpt = f"invite-{tag}@münchen.de"
    puny_rcpt = f"invite-{tag}@xn--mnchen-3ya.de"
    invite = seed_helpers.workspace_invite(db_conn, workspace["id"], email=rcpt)
    smtp_sink.clear()
    prev_from = _override_email_from(db_conn, NONASCII_FROM)
    try:
        _publish_and_observe_local_args(
            WORKSPACE_INVITATION,
            [rcpt, str(workspace["id"]), invite["token"], SITE, inviter["email"]],
        )
        delivered = _only_delivery_matching(
            smtp_sink, f"invite-{tag}@", "rust non-ascii invitation mail"
        )
    finally:
        _restore_email_from(db_conn, prev_from)

    head = delivered["raw"].split("\r\n\r\n")[0]
    assert (
        "Subject: \r\n =?utf-8?q?Jos=C3=A9_has_invited_you_to_join_them_in_Caf=C3=A9_on_Pi_Dash?="
        in head
    ), f"Subject not RFC 2047-encoded on the wire:\n{head}"
    assert (
        "From: =?utf-8?q?=C3=89quipe_Pi_Dash?= <team@x.com>" in head
    ), f"From display name not RFC 2047-encoded on the wire:\n{head}"
    assert f"To: {puny_rcpt}" in head, f"To domain not punycoded on the wire:\n{head}"
    assert delivered["rcpt_tos"] == [puny_rcpt], (
        f"envelope RCPT TO not punycoded: {delivered['rcpt_tos']!r}"
    )
    assert delivered["mail_from"] == "team@x.com", (
        f"envelope MAIL FROM wrong: {delivered['mail_from']!r}"
    )
