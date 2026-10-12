# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: instance-console (license) write flows (PIDASHCONV-823).

Built from the ``license`` contract suite: test_instance.py (PATCH
name/read-only/overlong), test_configurations.py (PATCH plain, padded,
unknown-key, encrypted-at-rest, disable-email, email-check),
test_admins.py (create/unknown/missing-email, soft-delete, idempotent
delete, sign-up-screen-visited), test_auth_forms.py (token-less form
POSTs hit the CSRF_FAILURE_VIEW: HTTP 200 + csrf_failure template),
test_workspaces.py (create + validation) and test_permissions.py (anon
401 / non-admin 403 shapes).

The world seed already carries the license instance + admin; the seed
extras add the configuration rows, one newcomer user for the admin
invite, and nothing else. Console requests authenticate as
``admin:<user>`` (the forged sessions row presented as the
admin-session-id cookie the /api/instances/* console reads).
"""

from ..session import Request, Session

NEWCOMER_EMAIL = "replay-newcomer@example.com"

_EMAIL_SEED = (
    ("EMAIL_HOST", "smtp.example.com"),
    ("EMAIL_HOST_USER", "mailer"),
    ("EMAIL_HOST_PASSWORD", "s3cret"),
    ("ENABLE_SMTP", "1"),
    ("EMAIL_PORT", "587"),
    ("EMAIL_FROM", "noreply@example.com"),
)


def _seed_extras(conn) -> None:
    """Configuration rows + newcomer user (cloned into the Rust copy)."""
    from _harness import db as db_helpers

    db_helpers.create_user(conn, "newcomer", email=NEWCOMER_EMAIL)
    with conn.cursor() as cur:
        rows = [
            ("CONTRACT_PLAIN", "old", "general", False),
            ("CONTRACT_PADDED", "old", "general", False),
            # Plaintext seed under a true flag, exactly like the
            # contract test: the PATCH overwrites it with real
            # ciphertext; the comparer decrypts only valid Fernet.
            ("CONTRACT_SECRET", "old", "general", True),
        ]
        rows += [(key, value, "email", False) for key, value in _EMAIL_SEED]
        for key, value, category, encrypted in rows:
            cur.execute(
                "INSERT INTO instance_configurations "
                "(created_at, updated_at, id, key, value, category, is_encrypted) "
                "VALUES (now(), now(), gen_random_uuid(), %s, %s, %s, %s)",
                (key, value, category, encrypted),
            )
    conn.commit()


SESSIONS = (
    Session(
        name="license_lifecycle",
        domain="license",
        source="license/test_instance.py, test_configurations.py, "
        "test_admins.py, test_auth_forms.py, test_workspaces.py, "
        "test_permissions.py",
        auth_users=("admin:admin", "admin:member"),
        seed=_seed_extras,
        # Fernet-randomised ciphertext (license/utils/encryption.py):
        # rows flagged is_encrypted compare by decrypted plaintext.
        encrypted_columns={"instance_configurations": ["value"]},
        # Workspace.background_color default=get_random_color
        # (db/models/workspace.py:139): minted per backend at create.
        generated_columns={"workspaces": ["background_color"]},
        # NOTE (rebase): PIDASHCONV-886 landed; the builtin
        # scheduler pins tripped stale and were removed.
        requests=(
            # -- instance: rename, read-only fields ignored, overlong --
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "Renamed"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "Kept", "is_setup_done": False,
                      "email": "x@y.zz"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "n" * 300},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "x"},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "PATCH", "/api/instances/",
                route="/api/instances/",
                json={"instance_name": "x"},
                auth="anonymous", expect_status=401,
            ),
            # -- configurations: plain, padded, unknown, encrypted --
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PLAIN": "new"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PADDED": "  padded  "},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"NO_SUCH_KEY": "x"},
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_SECRET": "fresh"},
                auth="admin:admin", expect_status=200,
            ),
            # GET re-reads the stored rows through the decrypting
            # serializer: proves both backends' ciphertext decrypts to
            # the same plaintext (the DB compare already asserts it via
            # encrypted_columns; this asserts the read path too).
            Request(
                "GET", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "DELETE", "/api/instances/configurations/disable-email-feature/",
                route="/api/instances/configurations/disable-email-feature/",
                auth="admin:admin", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/email-credentials-check/",
                route="/api/instances/email-credentials-check/",
                json={},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "PATCH", "/api/instances/configurations/",
                route="/api/instances/configurations/",
                json={"CONTRACT_PLAIN": "z"},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "DELETE", "/api/instances/configurations/disable-email-feature/",
                route="/api/instances/configurations/disable-email-feature/",
                auth="anonymous", expect_status=401,
            ),
            # -- admins: invite, unknown, missing, delete, idempotent --
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": NEWCOMER_EMAIL, "role": 20},
                auth="admin:admin", capture={"newadmin": "id"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": "ghost@example.com"},
                auth="admin:admin", expect_status=404,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/admins/",
                route="/api/instances/admins/",
                json={"email": NEWCOMER_EMAIL},
                auth="admin:member", expect_status=403,
            ),
            Request(
                "DELETE", "/api/instances/admins/{newadmin}/",
                route="/api/instances/admins/{}/",
                auth="admin:admin", expect_status=204,
            ),
            Request(
                "DELETE", "/api/instances/admins/00000000-0000-0000-0000-000000000000/",
                route="/api/instances/admins/{}/",
                auth="admin:admin", expect_status=204,
            ),
            # -- signup-screen-visited: admin then anonymous (AllowAny,
            # idempotent re-mark) --
            Request(
                "POST", "/api/instances/admins/sign-up-screen-visited/",
                route="/api/instances/admins/sign-up-screen-visited/",
                auth="admin:admin", expect_status=204,
            ),
            Request(
                "POST", "/api/instances/admins/sign-up-screen-visited/",
                route="/api/instances/admins/sign-up-screen-visited/",
                auth="anonymous", expect_status=204,
            ),
            # -- token-less form POSTs: the custom CSRF_FAILURE_VIEW
            # (HTTP 200 + csrf_failure template) is the contract --
            Request(
                "POST", "/api/instances/admins/sign-in/",
                route="/api/instances/admins/sign-in/",
                form={"email": "replay-admin@example.com",
                      "password": "ContractPass123!"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/admins/sign-up/",
                route="/api/instances/admins/sign-up/",
                form={"email": "fresh@example.com",
                      "password": "SomeStrongPass123!",
                      "first_name": "Fresh"},
                auth="anonymous", expect_status=200,
            ),
            Request(
                "POST", "/api/instances/admins/sign-out/",
                route="/api/instances/admins/sign-out/",
                auth="admin:admin", expect_status=200,
            ),
            # -- workspaces: create + validation --
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "New Space", "slug": "new-space"},
                auth="admin:admin", capture={"ws_new": "id"},
                # background_color is minted random per backend (see
                # generated_columns); presence-asserted here.
                body_blank_paths=["background_color"],
                expect_status=201,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Only Name"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "n" * 81, "slug": "s" * 49},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Api", "slug": "api"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Acme 2", "slug": "REPLAY-WS"},
                auth="admin:admin", expect_status=400,
            ),
            Request(
                "POST", "/api/instances/workspaces/",
                route="/api/instances/workspaces/",
                json={"name": "Nope", "slug": "nope"},
                auth="admin:member", expect_status=403,
            ),
        ),
    ),
)
