# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier notification write flows (PIDASHCONV-823).

Built from the ``app_notifications`` contract suite: test_preferences.py
(own-preferences get/patch), test_detail.py (retrieve/patch/delete),
test_state_actions.py (read/unread, archive/unarchive) and
test_unread.py (mark-all-read). Session-cookie auth (admin/member),
like the suite.

Notifications have no create endpoint — rows are seeded (the suite
seeds them too, via ``_harness.factory.create_notification``) and the
session walks the state machine: snooze -> read -> unread -> archive ->
unarchive -> delete -> mark-all-read.
"""

from psycopg.rows import dict_row

from ..session import Request, Session

# Fixed notification ids seeded by _seed_notifications (addressed via
# static_context).
N1_ID = "88888888-0000-4000-8000-000000000001"
N2_ID = "88888888-0000-4000-8000-000000000002"
N3_ID = "88888888-0000-4000-8000-000000000003"
N4_ID = "88888888-0000-4000-8000-000000000004"

MISSING_ID = "00000000-0000-0000-0000-000000000000"

_PREFS = "/api/users/me/notification-preferences"
_N = "/api/workspaces/{ws}/users/notifications"


def _seed_notifications(conn) -> None:
    """Two plain + one mentioned notification for admin, one for member,
    plus an all-true preferences row for admin.

    The INSERTs mirror ``_harness.factory.create_notification`` and the
    ``create_user_notification`` post_save mirror in ``factory.create_user``
    (same columns).
    """
    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute("SELECT id FROM workspaces WHERE slug = 'replay-ws'")
        ws_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-admin@example.com'")
        admin_id = str(cur.fetchone()["id"])
        cur.execute("SELECT id FROM users WHERE email = 'replay-member@example.com'")
        member_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        for nid, title, sender, receiver in (
            (N1_ID, "Plain unread", "issue.created", admin_id),
            (N2_ID, "Second unread", "issue.created", admin_id),
            (N3_ID, "Mentioned unread", "issue.mentioned", admin_id),
            (N4_ID, "Member note", "issue.created", member_id),
        ):
            cur.execute(
                """
                INSERT INTO notifications
                   (id, entity_name, title, message_html, sender,
                    workspace_id, receiver_id, data, read_at,
                    snoozed_till, archived_at, created_at, updated_at)
                VALUES (%s, 'issue', %s, '<p></p>', %s, %s, %s,
                        NULL, NULL, NULL, NULL, now(), now())
                """,
                (nid, title, sender, ws_id, receiver),
            )
        cur.execute(
            """
            INSERT INTO user_notification_preferences
               (id, property_change, state_change, comment, mention,
                issue_completed, user_id, created_at, updated_at)
            VALUES (gen_random_uuid(), true, true, true, true, true,
                    %s, now(), now())
            """,
            (admin_id,),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="app_notifications_lifecycle",
        domain="app_notifications",
        source="app_notifications/test_preferences.py, test_detail.py, test_state_actions.py, test_unread.py",
        auth_users=("admin", "member"),
        seed=_seed_notifications,
        static_context={
            "n1": N1_ID,
            "n2": N2_ID,
            "n3": N3_ID,
            "n4": N4_ID,
        },
        requests=(
            # Preferences: get -> patch off -> get persists ->
            # patch back on.
            Request(
                "GET", _PREFS + "/",
                route="/api/users/me/notification-preferences/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _PREFS + "/",
                route="/api/users/me/notification-preferences/",
                json={"mention": False, "comment": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _PREFS + "/",
                route="/api/users/me/notification-preferences/",
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _PREFS + "/",
                route="/api/users/me/notification-preferences/",
                json={"mention": True, "comment": True},
                auth="admin", expect_status=200,
            ),
            # Notifications: list (mentioned excluded by default) ->
            # unread counts -> detail -> foreign 404 -> unknown 404.
            Request(
                "GET", _N + "/",
                route="/api/workspaces/{slug}/users/notifications/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/unread/",
                route="/api/workspaces/{slug}/users/notifications/unread/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/{n1}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/{n1}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="member", expect_status=404,
            ),
            Request(
                "GET", _N + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Detail PATCH: snooze -> clear snooze (title writes
            # dropped) -> foreign 404.
            Request(
                "PATCH", _N + "/{n1}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                json={"snoozed_till": "2027-05-05T00:00:00Z"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _N + "/{n1}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                json={"snoozed_till": None, "title": "Hacked title"},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _N + "/{n1}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                json={"snoozed_till": None},
                auth="member", expect_status=404,
            ),
            # Read cycle: mark -> unmark -> foreign 404s.
            Request(
                "POST", _N + "/{n1}/read/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/read/",
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _N + "/{n1}/read/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/read/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _N + "/{n1}/read/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/read/",
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", _N + "/{n1}/read/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/read/",
                auth="member", expect_status=404,
            ),
            # Archive cycle: mark -> default list drops it ->
            # archived list keeps it -> unmark -> foreign 404.
            Request(
                "POST", _N + "/{n1}/archive/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/",
                route="/api/workspaces/{slug}/users/notifications/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/",
                route="/api/workspaces/{slug}/users/notifications/",
                params={"archived": "true"},
                auth="admin", expect_status=200,
            ),
            Request(
                "DELETE", _N + "/{n1}/archive/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/archive/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _N + "/{n1}/archive/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/archive/",
                auth="member", expect_status=404,
            ),
            # Deletes: foreign 404 first, then the soft 204, then
            # the row is gone from the API.
            Request(
                "DELETE", _N + "/{n3}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="member", expect_status=404,
            ),
            Request(
                "DELETE", _N + "/{n3}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _N + "/{n3}/",
                route="/api/workspaces/{slug}/users/notifications/{pk}/",
                auth="admin", expect_status=404,
            ),
            # Mark-all-read: the two surviving admin notifications
            # flip to read; the unread counts zero out.
            Request(
                "POST", _N + "/mark-all-read/",
                route="/api/workspaces/{slug}/users/notifications/mark-all-read/",
                json={},
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/unread/",
                route="/api/workspaces/{slug}/users/notifications/unread/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _N + "/",
                route="/api/workspaces/{slug}/users/notifications/",
                auth="admin", expect_status=200,
            ),
        ),
    ),
)
