# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""F37-03/F37-04 replay: users + membership ops commands (PIDASHCONV-807).

Drives the built ``pidash-api ops ...`` binary with piped stdio and
asserts byte-exact stdout/stderr/exit codes plus DB before/after
diffs. Goldens were recorded against Django 4.2.30
(``apps/api``, oracle venv) 2026-10-10; every divergence from the
F37 fixtures below favors the observed Python behavior:

* Uncaught ``CommandError`` renders ``CommandError: {msg}`` on
  stderr (exit 1) — not the ``Error: `` prefix the fixtures imply.
* ``create_project_member``'s ``except CommandError`` handler
  crashes (``style.ERROR`` is the identity when piped, so
  ``stdout.write`` raises ``AttributeError``): exit 1 with a chained
  traceback, not the fixtures' exit-0 stdout errors.
* Absent ``--role`` writes ``NULL`` and the not-null violation
  crashes (exit 1) — it is never "written as NULL".
* ``Instance.objects.last()`` is the OLDEST row (``ORDER BY
  created_at ASC``), not ``DESC``.
* Traceback skeletons keep the deterministic structure (headers,
  exception lines, chaining sentence, blank separators, server
  message + ``DETAIL`` shape) and drop only the frames (checkout /
  venv paths) and the ``GetPassWarning`` header (stdlib path).
  ``DETAIL: Failing row`` bodies embed fresh timestamps and UUIDs
  (nondeterministic even between two Python runs), so those
  segments assert by pattern, everything else byte-exact.
* argparse exit-2 usage texts belong to ``manage.py``; the ``ops``
  CLI is clap and renders clap errors (exit codes still match).
"""

import base64
import hashlib
import os
import re
import subprocess
import sys
import uuid

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from _harness.seed import Seed  # noqa: E402

WEAK_PASSWORD = "password123"  # zxcvbn 0 on both estimators
BOUNDARY_PASSWORD = "Sup3rS3cur3!"  # zxcvbn 3: weakest accepted
STRONG_PASSWORD = "Tr0ub4dor&3"  # zxcvbn 4

GETPASS_PAIR = (
    b"Warning: Password input may be echoed.\n"
    b"Password: \n"
    b"Warning: Password input may be echoed.\n"
    b"Password (again): \n"
)


def run_ops(rust_bin, *argv, stdin=b""):
    """Run ``pidash-api ops ...`` with piped stdio; raw bytes out."""
    return subprocess.run(
        [rust_bin, "ops", *argv],
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=300,
    )


def check_django_hash(password, encoded):
    """Independent PBKDF2 check (stdlib only, no Django import)."""
    algo, iterations, salt, digest = encoded.split("$")
    assert algo == "pbkdf2_sha256", encoded
    assert iterations == "600000", encoded
    assert len(salt) == 22 and salt.isalnum(), encoded
    calc = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), 600000)
    return base64.b64encode(calc).decode() == digest


def fetch_user(db, email):
    with db.cursor() as cur:
        cur.execute(
            'SELECT "id", "email", "display_name", "is_staff", "is_superuser",'
            ' "is_active", "is_password_autoset", "password", "token",'
            ' "token_updated_at", "updated_at" FROM "users" WHERE "email" = %s',
            (email,),
        )
        return cur.fetchone()


def seed_edge_user(seed, *, email, display_name="", superuser=False,
                   staff=False, active=False, token="tok",
                   token_updated_at="2020-01-01T00:00:00+00:00"):
    """A user whose `save()` edges all fire (raw SQL skips `save()`)."""
    user = seed.user(email=email)
    with seed.conn.cursor() as cur:
        cur.execute(
            'UPDATE "users" SET "display_name" = %s, "is_superuser" = %s,'
            ' "is_staff" = %s, "is_active" = %s, "token" = %s,'
            ' "token_updated_at" = %s, "updated_at" = %s WHERE "id" = %s',
            (display_name, superuser, staff, active, token,
             token_updated_at, "2020-01-01T00:00:00+00:00", user["id"]),
        )
    return user


def seed_project_member(db, project_id, workspace_id, user_id, *, role=5,
                        active=True, sort_order=65535.0):
    with db.cursor() as cur:
        cur.execute(
            'INSERT INTO "project_members" ("id", "created_at", "updated_at",'
            ' "project_id", "workspace_id", "member_id", "role",'
            ' "view_props", "default_props", "preferences", "sort_order",'
            ' "is_active") VALUES (gen_random_uuid(), NOW(), NOW(), %s, %s,'
            ' %s, %s, %s, %s, %s, %s, %s) RETURNING "id"',
            (project_id, workspace_id, user_id, role, "{}", "{}", "{}",
             sort_order, active),
        )
        return cur.fetchone()[0]


def fetch_project_member(db, project_id, user_id):
    with db.cursor() as cur:
        cur.execute(
            'SELECT "id", "role", "is_active", "sort_order", "comment",'
            ' "view_props", "default_props", "preferences",'
            ' "created_by_id", "updated_at", "deleted_at"'
            ' FROM "project_members" WHERE "project_id" = %s'
            ' AND "member_id" = %s AND "deleted_at" IS NULL',
            (project_id, user_id),
        )
        return cur.fetchone()


def seed_property(db, workspace_id, project_id, user_id, *, sort_order=65535.0):
    with db.cursor() as cur:
        cur.execute(
            'INSERT INTO "project_user_properties" ("id", "created_at",'
            ' "updated_at", "workspace_id", "project_id", "user_id",'
            ' "filters", "display_filters", "display_properties",'
            ' "rich_filters", "preferences", "sort_order") VALUES'
            ' (gen_random_uuid(), NOW(), NOW(), %s, %s, %s, %s, %s, %s, %s,'
            ' %s, %s) RETURNING "id"',
            (workspace_id, project_id, user_id, "{}", "{}", "{}", "{}",
             "{}", sort_order),
        )
        return cur.fetchone()[0]


def fetch_property(db, project_id, user_id):
    with db.cursor() as cur:
        cur.execute(
            'SELECT "id", "sort_order", "filters", "display_filters",'
            ' "display_properties", "rich_filters", "preferences",'
            ' "workspace_id", "created_by_id" FROM "project_user_properties"'
            ' WHERE "project_id" = %s AND "user_id" = %s'
            ' AND "deleted_at" IS NULL',
            (project_id, user_id),
        )
        return cur.fetchone()


def count_properties(db, user_id):
    with db.cursor() as cur:
        cur.execute(
            'SELECT COUNT(*) FROM "project_user_properties"'
            ' WHERE "user_id" = %s AND "deleted_at" IS NULL',
            (user_id,),
        )
        return cur.fetchone()[0]


def reset_instances(db):
    """Empty the global instance tables; tests seed their own world."""
    with db.cursor() as cur:
        cur.execute('DELETE FROM "instance_admins"')
        cur.execute('DELETE FROM "instances"')


def seed_instance(db, *, name, iid, created_at):
    with db.cursor() as cur:
        cur.execute(
            'INSERT INTO "instances" ("id", "created_at", "updated_at",'
            ' "instance_name", "instance_id", "current_version", "edition",'
            ' "domain", "last_checked_at", "is_telemetry_enabled",'
            ' "is_support_required", "is_setup_done",'
            ' "is_signup_screen_visited", "is_verified", "is_test",'
            ' "is_current_version_deprecated") VALUES (gen_random_uuid(),'
            ' %s, NOW(), %s, %s, %s, %s, %s, NOW(), TRUE, TRUE, FALSE,'
            ' FALSE, FALSE, FALSE, FALSE) RETURNING "id"',
            (created_at, name, iid, "1.0-test", "PI_DASH_COMMUNITY", ""),
        )
        return str(cur.fetchone()[0])


def seed_instance_admin(db, user_id, instance_id, *, role=20):
    with db.cursor() as cur:
        cur.execute(
            'INSERT INTO "instance_admins" ("id", "created_at", "updated_at",'
            ' "user_id", "instance_id", "role", "is_verified")'
            ' VALUES (gen_random_uuid(), NOW(), NOW(), %s, %s, %s, FALSE)'
            ' RETURNING "id"',
            (user_id, instance_id, role),
        )
        return cur.fetchone()[0]


def member_crash_stderr(inner_message):
    """The BUG-3 chained skeleton (frames stripped, blanks kept)."""
    return (
        "Traceback (most recent call last):\n"
        f"django.core.management.base.CommandError: {inner_message}\n"
        "\n"
        "During handling of the above exception, another exception occurred:\n"
        "\n"
        "Traceback (most recent call last):\n"
        "AttributeError: 'CommandError' object has no attribute 'endswith'\n"
    ).encode()


def direct_cause_skeleton(psycopg_class, django_class, message, detail=None):
    """A DB-error chained skeleton; detail renders on both blocks."""
    rendered = message if detail is None else f"{message}\nDETAIL:  {detail}"
    return (
        "Traceback (most recent call last):\n"
        f"{psycopg_class}: {rendered}\n"
        "\n"
        "The above exception was the direct cause of the following exception:\n"
        "\n"
        "Traceback (most recent call last):\n"
        f"{django_class}: {rendered}\n"
    ).encode()


def tag(prefix):
    return f"{prefix}-{uuid.uuid4().hex[:8]}"


# ---------------------------------------------------------------------------
# activate_user
# ---------------------------------------------------------------------------

class TestActivateUser:
    def test_missing_user(self, rust_bin, db):
        email = f"nobody-{uuid.uuid4().hex[:8]}@example.com"
        proc = run_ops(rust_bin, "activate_user", email)
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == (
            f"CommandError: Error: User with {email} does not exists\n"
        ).encode()

    def test_empty_email(self, rust_bin, db):
        proc = run_ops(rust_bin, "activate_user", "")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == b"CommandError: Error: Email is required\n"

    def test_success_flips_active_and_stamps(self, rust_bin, db):
        seed = Seed(db)
        user = seed_edge_user(
            seed, email=f"inactive-{uuid.uuid4().hex[:8]}@example.com")
        proc = run_ops(rust_bin, "activate_user", user["email"])
        assert proc.returncode == 0
        assert proc.stdout == b"User activated successfully\n"
        assert proc.stderr == b""
        row = fetch_user(db, user["email"].lower())
        assert row[5] is True  # is_active
        assert str(row[10]) != "2020-01-01 00:00:00+00:00"  # updated_at

    def test_success_applies_save_edges(self, rust_bin, db):
        seed = Seed(db)
        email = f"EDGE-{uuid.uuid4().hex[:8]}@EXAMPLE.COM"
        user = seed_edge_user(seed, email=email, superuser=True)
        proc = run_ops(rust_bin, "activate_user", email)
        assert proc.returncode == 0
        assert proc.stdout == b"User activated successfully\n"
        row = fetch_user(db, email.lower())
        assert row[1] == email.lower()  # email normalized
        assert row[2] == email.lower().split("@")[0]  # display fill
        assert row[3] is True  # is_staff from superuser
        assert row[5] is True  # is_active
        assert row[8] != "tok" and len(row[8]) == 64  # token rotated
        assert row[9] is not None  # token_updated_at
        assert re.fullmatch(rb"[0-9a-f]{64}", row[8].encode())

    def test_missing_positional_is_clap_usage_error(self, rust_bin, db):
        proc = run_ops(rust_bin, "activate_user")
        assert proc.returncode == 2
        assert proc.stdout == b""
        assert proc.stderr == (
            b"error: the following required arguments were not provided:\n"
            b"  <EMAIL>\n"
            b"\n"
            b"Usage: pidash-api ops activate_user <EMAIL>\n"
            b"\n"
            b"For more information, try '--help'.\n"
        )


# ---------------------------------------------------------------------------
# reset_password
# ---------------------------------------------------------------------------

class TestResetPassword:
    def test_missing_user(self, rust_bin, db):
        email = f"nobody-{uuid.uuid4().hex[:8]}@example.com"
        proc = run_ops(rust_bin, "reset_password", email)
        assert proc.returncode == 0
        assert proc.stdout == b""
        assert proc.stderr == (
            f"Error: User with {email} does not exists\n"
        ).encode()

    def test_empty_email(self, rust_bin, db):
        proc = run_ops(rust_bin, "reset_password", "")
        assert proc.returncode == 0
        assert proc.stdout == b""
        assert proc.stderr == b"Error: Email is required\n"

    def test_mismatch(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        proc = run_ops(
            rust_bin, "reset_password", user["email"],
            stdin=b"foo12345\nbar12345\n",
        )
        assert proc.returncode == 0
        assert proc.stdout == b""
        # No GetPassWarning header: stdlib-path noise, not ported.
        assert proc.stderr == GETPASS_PAIR + b"Error: Your passwords didn't match.\n"

    def test_blank(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        proc = run_ops(
            rust_bin, "reset_password", user["email"], stdin=b"   \n   \n")
        assert proc.returncode == 0
        assert proc.stdout == b""
        assert proc.stderr == (
            GETPASS_PAIR + b"Error: Blank passwords aren't allowed.\n")

    def test_weak_password(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        before = fetch_user(db, user["email"])
        stdin = f"{WEAK_PASSWORD}\n{WEAK_PASSWORD}\n".encode()
        proc = run_ops(rust_bin, "reset_password", user["email"], stdin=stdin)
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == (
            GETPASS_PAIR
            + b"CommandError: Password is too common please set a complex password\n"
        )
        assert fetch_user(db, user["email"])[7] == before[7]  # hash kept

    def test_boundary_score3_accepted(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        stdin = f"{BOUNDARY_PASSWORD}\n{BOUNDARY_PASSWORD}\n".encode()
        proc = run_ops(rust_bin, "reset_password", user["email"], stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == b"User password updated successfully\n"
        assert proc.stderr == GETPASS_PAIR
        assert check_django_hash(
            BOUNDARY_PASSWORD, fetch_user(db, user["email"])[7])

    def test_success_rehashes_and_clears_autoset(self, rust_bin, db):
        seed = Seed(db)
        email = f"PW-{uuid.uuid4().hex[:8]}@EXAMPLE.COM"
        user = seed_edge_user(seed, email=email)
        with db.cursor() as cur:
            cur.execute(
                'UPDATE "users" SET "is_password_autoset" = TRUE'
                ' WHERE "id" = %s', (user["id"],))
        stdin = f"{STRONG_PASSWORD}\n{STRONG_PASSWORD}\n".encode()
        proc = run_ops(rust_bin, "reset_password", email, stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == b"User password updated successfully\n"
        assert proc.stderr == GETPASS_PAIR
        row = fetch_user(db, email.lower())
        assert row[6] is False  # is_password_autoset flipped
        assert check_django_hash(STRONG_PASSWORD, row[7])
        assert row[1] == email.lower()  # save() still normalizes

    def test_eof_before_first_password(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        proc = run_ops(rust_bin, "reset_password", user["email"], stdin=b"")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == (
            b"Warning: Password input may be echoed.\n"
            b"Password: Traceback (most recent call last):\n"
            b"EOFError\n"
        )

    def test_missing_positional_is_clap_usage_error(self, rust_bin, db):
        proc = run_ops(rust_bin, "reset_password")
        assert proc.returncode == 2
        assert proc.stdout == b""
        assert b"<EMAIL>" in proc.stderr


# ---------------------------------------------------------------------------
# create_instance_admin
# ---------------------------------------------------------------------------

class TestCreateInstanceAdmin:
    def test_empty_email(self, rust_bin, db):
        proc = run_ops(rust_bin, "create_instance_admin", "")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == (
            b"CommandError: Please provide the email of the admin.\n")

    def test_missing_user(self, rust_bin, db):
        email = f"nobody-{uuid.uuid4().hex[:8]}@example.com"
        proc = run_ops(rust_bin, "create_instance_admin", email)
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == (
            b"CommandError: User with the provided email does not exist.\n")

    def test_success_links_oldest_instance(self, rust_bin, db):
        reset_instances(db)
        seed = Seed(db)
        user = seed.user()
        old_id = seed_instance(
            db, name="old", iid=tag("inst-old"),
            created_at="2020-01-01T00:00:00+00:00")
        seed_instance(
            db, name="new", iid=tag("inst-new"),
            created_at="2021-01-01T00:00:00+00:00")
        proc = run_ops(rust_bin, "create_instance_admin", user["email"])
        assert proc.returncode == 0
        assert proc.stdout == b"Successfully created the admin\n"
        assert proc.stderr == b""
        with db.cursor() as cur:
            cur.execute(
                'SELECT "role", "is_verified", "instance_id",'
                ' "created_by_id", "updated_by_id", "deleted_at", "user_id"'
                ' FROM "instance_admins" WHERE "user_id" = %s',
                (user["id"],),
            )
            row = cur.fetchone()
        assert row[0] == 20 and row[1] is False
        assert str(row[2]) == old_id  # oldest, not newest
        assert row[3] is None and row[4] is None and row[5] is None

    def test_duplicate_prints_inner_then_outer(self, rust_bin, db):
        reset_instances(db)
        seed = Seed(db)
        user = seed.user()
        inst = seed_instance(
            db, name="solo", iid=tag("inst-solo"),
            created_at="2020-01-01T00:00:00+00:00")
        seed_instance_admin(db, user["id"], inst)
        proc = run_ops(rust_bin, "create_instance_admin", user["email"])
        assert proc.returncode == 1
        assert proc.stdout == (
            b"The provided email is already an instance admin.\n")
        assert proc.stderr == (
            b"CommandError: Failed to create the instance admin.\n")

    def test_no_instances_prints_db_error(self, rust_bin, db):
        reset_instances(db)
        seed = Seed(db)
        user = seed.user()
        proc = run_ops(rust_bin, "create_instance_admin", user["email"])
        assert proc.returncode == 1
        # `print(e)`: server message + DETAIL row (timestamps/UUIDs are
        # runtime noise, so the row body asserts by pattern).
        assert proc.stdout.startswith(
            b'null value in column "instance_id" of relation'
            b' "instance_admins" violates not-null constraint\n'
            b"DETAIL:  Failing row contains ("
        )
        assert proc.stdout.endswith(b").\n")
        assert proc.stderr == (
            b"CommandError: Failed to create the instance admin.\n")

    def test_role_mismatch_retries_get_then_fails(self, rust_bin, db):
        # Existing row with another role: the `get(role=20)` misses,
        # the `create(role=20)` hits the (instance, user) unique
        # constraint, the retry `get(role=20)` misses again.
        reset_instances(db)
        seed = Seed(db)
        user = seed.user()
        inst = seed_instance(
            db, name="solo", iid=tag("inst-mm"),
            created_at="2020-01-01T00:00:00+00:00")
        seed_instance_admin(db, user["id"], inst, role=15)
        proc = run_ops(rust_bin, "create_instance_admin", user["email"])
        assert proc.returncode == 1
        assert b"duplicate key value violates unique constraint" in proc.stdout
        assert proc.stderr == (
            b"CommandError: Failed to create the instance admin.\n")

    def test_missing_positional_is_clap_usage_error(self, rust_bin, db):
        proc = run_ops(rust_bin, "create_instance_admin")
        assert proc.returncode == 2
        assert proc.stdout == b""
        assert b"<ADMIN_EMAIL>" in proc.stderr


# ---------------------------------------------------------------------------
# create_project_member
# ---------------------------------------------------------------------------

class TestCreateProjectMember:
    def _world(self, db):
        """User + workspace + active ws-membership + project."""
        seed = Seed(db)
        user = seed.user()
        ws = seed.workspace(user["id"])
        seed.member(ws["id"], user["id"], role=15)
        project_id = seed.project(ws["id"])
        return seed, user, ws, project_id

    def test_missing_project_id(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        proc = run_ops(
            rust_bin, "create_project_member",
            "--user_email", user["email"], "--role", "5")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == member_crash_stderr("Project ID is required")

    def test_missing_user_email(self, rust_bin, db):
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", str(uuid.uuid4()), "--role", "5")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == member_crash_stderr("User Email is required")

    def test_empty_flags_crash_like_missing(self, rust_bin, db):
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", "", "--user_email", "u@e.com", "--role", "5")
        assert proc.returncode == 1
        assert proc.stdout == b""
        assert proc.stderr == member_crash_stderr("Project ID is required")

    def test_user_not_found(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        email = f"nobody-{uuid.uuid4().hex[:8]}@example.com"
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", email, "--role", "15")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 15\n"
        assert proc.stderr == member_crash_stderr("User not found")

    def test_project_not_found(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        missing = str(uuid.uuid4())
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", missing, "--user_email", user["email"],
            "--role", "15")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 15\n"
        assert proc.stderr == member_crash_stderr("Project not found")

    def test_not_member_in_workspace(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        outsider = seed.user()
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", outsider["email"],
            "--role", "15")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 15\n"
        assert proc.stderr == member_crash_stderr("User not member in workspace")

    def test_soft_deleted_project_not_found(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        with db.cursor() as cur:
            cur.execute(
                'UPDATE "projects" SET "deleted_at" = NOW() WHERE "id" = %s',
                (project_id,))
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 5\n"
        assert proc.stderr == member_crash_stderr("Project not found")

    def test_invalid_uuid(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", "xyz", "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 5\n"
        assert proc.stderr == (
            "Traceback (most recent call last):\n"
            "ValueError: badly formed hexadecimal UUID string\n"
            "\n"
            "During handling of the above exception, another exception occurred:\n"
            "\n"
            "Traceback (most recent call last):\n"
            "django.core.exceptions.ValidationError:"
            " ['\u201cxyz\u201d is not a valid UUID.']\n"
        ).encode()

    def test_braced_uuid_form_succeeds_and_echoes_raw(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        braced = "{" + project_id + "}"
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", braced, "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 0
        # The success line echoes the RAW flag text, braces included.
        assert proc.stdout == (
            f"Role: 5\nUser {user['email']} added to project {braced}\n"
        ).encode()
        assert proc.stderr == b""

    def test_update_path_sets_role_and_active_only(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        seed_project_member(
            db, project_id, ws["id"], user["id"], role=5, active=False)
        seed_property(db, ws["id"], project_id, user["id"])
        before = fetch_project_member(db, project_id, user["id"])
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "15")
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Role: 15\nUser {user['email']} added to project {project_id}\n"
        ).encode()
        assert proc.stderr == b""
        after = fetch_project_member(db, project_id, user["id"])
        assert after[1] == 15 and after[2] is True
        # queryset update(): no auto_now bump, no other columns.
        assert after[9] == before[9]
        assert count_properties(db, user["id"]) == 1  # get_or_create no-op

    def test_create_path_writes_member_and_hook_property(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Role: 5\nUser {user['email']} added to project {project_id}\n"
        ).encode()
        assert proc.stderr == b""
        member = fetch_project_member(db, project_id, user["id"])
        assert member[1] == 5 and member[2] is True  # role, is_active
        assert member[3] == 65535.0 and member[4] is None  # sort, comment
        assert member[5]["display_filters"]["order_by"] == "-created_at"
        assert "display_properties" not in member[5]  # project-flavored
        assert member[5] == member[6]  # view == default
        assert member[7]["pages"] == {"block_display": True}
        assert member[8] is None  # created_by
        prop = fetch_property(db, project_id, user["id"])
        assert prop[1] == 65535.0  # hook: no prior sorts
        assert prop[2]["subscriber"] is None
        assert prop[4]["sub_issue_count"] is True
        assert "sub_issue" not in prop[4]
        assert prop[5] == {}  # rich_filters
        assert str(prop[7]) == ws["id"]  # workspace carried
        assert prop[8] is None

    def test_soft_deleted_member_takes_create_path(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        seed_project_member(
            db, project_id, ws["id"], user["id"], role=15, active=True)
        with db.cursor() as cur:
            cur.execute(
                'UPDATE "project_members" SET "deleted_at" = NOW()'
                ' WHERE "project_id" = %s AND "member_id" = %s',
                (project_id, user["id"]))
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 0
        assert f"added to project {project_id}\n".encode() in proc.stdout
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "project_members"'
                ' WHERE "project_id" = %s AND "member_id" = %s'
                ' AND "deleted_at" IS NULL',
                (project_id, user["id"]))
            assert cur.fetchone()[0] == 1

    def test_hook_sorts_below_existing_minimum(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        seed_property(db, ws["id"], project_id, user["id"], sort_order=65535.0)
        second = seed.project(ws["id"])
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", second, "--user_email", user["email"],
            "--role", "5")
        assert proc.returncode == 0
        prop = fetch_property(db, second, user["id"])
        assert prop[1] == 55535.0  # min(65535) - 10000

    def test_absent_role_crashes_and_orphans_property(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"])
        assert proc.returncode == 1
        assert proc.stdout == b"Role: None\n"
        stderr = proc.stderr.decode()
        assert stderr.startswith(
            "Traceback (most recent call last):\n"
            "psycopg.errors.NotNullViolation: "
            'null value in column "role" of relation "project_members"'
            " violates not-null constraint\n"
            "DETAIL:  Failing row contains ("
        )
        assert (
            "\n\nThe above exception was the direct cause of the following"
            " exception:\n\nTraceback (most recent call last):\n"
            "django.db.utils.IntegrityError: "
            'null value in column "role" of relation "project_members"'
            " violates not-null constraint\n"
            "DETAIL:  Failing row contains (" in stderr
        )
        # The hook property row persists; the member row does not.
        assert count_properties(db, user["id"]) == 1
        assert fetch_project_member(db, project_id, user["id"]) is None

    def test_bare_role_flag_reads_none(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: None\n"
        assert b"NotNullViolation" in proc.stderr

    def test_underscore_role_parses(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "1_5")
        assert proc.returncode == 0
        assert proc.stdout.startswith(b"Role: 15\n")
        assert fetch_project_member(db, project_id, user["id"])[1] == 15

    def test_negative_role_hits_check_constraint(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "-5")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: -5\n"
        stderr = proc.stderr.decode()
        assert "psycopg.errors.CheckViolation" in stderr
        assert "project_member_role_check" in stderr
        assert "django.db.utils.IntegrityError" in stderr

    def test_huge_role_is_data_error(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "9999999999")
        assert proc.returncode == 1
        assert proc.stdout == b"Role: 9999999999\n"
        # No DETAIL on this server error: fully deterministic.
        assert proc.stderr == direct_cause_skeleton(
            "psycopg.errors.NumericValueOutOfRange",
            "django.db.utils.DataError",
            "smallint out of range",
        )
        assert count_properties(db, user["id"]) == 1  # hook orphan

    def test_orphaned_property_collides_on_retry(self, rust_bin, db):
        # The orphan from a crashed create makes the NEXT create fail
        # in the hook with a unique violation (verified on the oracle).
        seed, user, ws, project_id = self._world(db)
        first = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"])
        assert first.returncode == 1
        retry = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "5")
        assert retry.returncode == 1
        assert retry.stdout == b"Role: 5\n"
        stderr = retry.stderr.decode()
        assert "psycopg.errors.UniqueViolation" in stderr
        assert "project_user_property_unique_user_project_when_deleted_at_null" in stderr

    def test_invalid_role_is_clap_value_error(self, rust_bin, db):
        seed, user, ws, project_id = self._world(db)
        proc = run_ops(
            rust_bin, "create_project_member",
            "--project_id", project_id, "--user_email", user["email"],
            "--role", "abc")
        assert proc.returncode == 2
        assert proc.stdout == b""
        assert proc.stderr == (
            b"error: invalid value 'abc' for '--role <ROLE>': "
            b"invalid literal for int() with base 10: 'abc'\n"
        )


# ---------------------------------------------------------------------------
# create_dummy_data
# ---------------------------------------------------------------------------

PROMPTS5 = (
    b"Workspace Name: Workspace slug: Your email: "
    b"Enter Member emails (comma separated): "
    b"Number of projects to be created: "
)
COUNT_PROMPTS5 = (
    b"Number of issues to be created: "
    b"Number of cycles to be created: "
    b"Number of modules to be created: "
    b"Number of pages to be created: "
    b"Number of intake issues to be created: "
)


def fetch_workspace(db, slug):
    with db.cursor() as cur:
        cur.execute(
            'SELECT "id", "name", "slug", "background_color", "timezone",'
            ' "logo", "organization_size", "owner_id" FROM "workspaces"'
            ' WHERE "slug" = %s AND "deleted_at" IS NULL',
            (slug,),
        )
        return cur.fetchone()


def workspace_member_emails(db, workspace_id):
    with db.cursor() as cur:
        cur.execute(
            'SELECT u."email", wm."role", wm."is_active" FROM "workspace_members" wm'
            ' JOIN "users" u ON u."id" = wm."member_id"'
            ' WHERE wm."workspace_id" = %s AND wm."deleted_at" IS NULL'
            " ORDER BY 1",
            (workspace_id,),
        )
        return cur.fetchall()


def count_project_children(db, workspace_id, table):
    with db.cursor() as cur:
        cur.execute(
            f'SELECT COUNT(*) FROM "{table}" c'
            ' JOIN "projects" p ON p."id" = c."project_id"'
            ' WHERE p."workspace_id" = %s AND c."deleted_at" IS NULL'
            ' AND p."deleted_at" IS NULL',
            (workspace_id,),
        )
        return cur.fetchone()[0]


class TestCreateDummyData:
    def test_blank_slug(self, rust_bin, db):
        proc = run_ops(
            rust_bin, "create_dummy_data", stdin=b"My Workspace\n\n")
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Workspace slug: "
            b"Command errored out Workspace slug is required\n"
        )
        assert proc.stderr == b""

    def test_existing_slug(self, rust_bin, db):
        seed = Seed(db)
        user = seed.user()
        ws = seed.workspace(user["id"])
        stdin = f"My Workspace\n{ws['slug']}\n".encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Workspace slug: "
            b"Command errored out Workspace already exists\n"
        )
        assert proc.stderr == b""

    def test_blank_email(self, rust_bin, db):
        slug = tag("ws-blank")
        proc = run_ops(
            rust_bin, "create_dummy_data",
            stdin=f"W\n{slug}\n\n".encode())
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Workspace slug: Your email: "
            b"Command errored out User email is required and should have"
            b" signed in pi dash\n"
        )
        assert proc.stderr == b""
        assert fetch_workspace(db, slug) is None

    def test_unknown_email(self, rust_bin, db):
        slug = tag("ws-unknown")
        email = f"nobody-{uuid.uuid4().hex[:8]}@example.com"
        proc = run_ops(
            rust_bin, "create_dummy_data",
            stdin=f"W\n{slug}\n{email}\n".encode())
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Workspace slug: Your email: "
            b"Command errored out User email is required and should have"
            b" signed in pi dash\n"
        )
        assert proc.stderr == b""

    def test_eof_on_first_prompt(self, rust_bin, db):
        proc = run_ops(rust_bin, "create_dummy_data", stdin=b"")
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Command errored out EOF when reading a line\n")
        assert proc.stderr == b""

    def test_bad_project_count_keeps_scaffolding(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        member = seed.user()
        slug = tag("ws-badcount")
        stdin = (f"W\n{slug}\n{creator['email']}\n{member['email']}\n"
                 f"abc\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == (
            PROMPTS5 +
            b"Command errored out invalid literal for int() with base 10:"
            b" 'abc'\n"
        )
        assert proc.stderr == b""
        # No transaction: the workspace + members persist.
        ws = fetch_workspace(db, slug)
        assert ws is not None and ws[1] == "W"
        emails = [row[0] for row in workspace_member_emails(db, ws[0])]
        assert emails == sorted([creator["email"], member["email"]])

    def test_bad_intake_count_aborts_before_task(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        slug = tag("ws-badintake")
        stdin = (f"W\n{slug}\n{creator['email']}\n\n1\n6\n1\n5\n2\n"
                 f"abc\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        # The intake prompt prints before int() fails on the answer.
        assert proc.stdout == (
            PROMPTS5 +
            b"Please provide the following details for project 1:\n"
            + COUNT_PROMPTS5
            + b"Command errored out invalid literal for int() with base 10:"
            b" 'abc'\n"
        )
        assert proc.stderr == b""
        ws = fetch_workspace(db, slug)
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "projects" WHERE "workspace_id" = %s'
                ' AND "deleted_at" IS NULL', (ws[0],))
            assert cur.fetchone()[0] == 0

    def test_long_workspace_name_is_data_error(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        slug = tag("ws-longname")
        stdin = (f"{'N' * 81}\n{slug}\n{creator['email']}\n\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == (
            b"Workspace Name: Workspace slug: Your email: "
            b"Enter Member emails (comma separated): "
            b"Command errored out value too long for type character"
            b" varying(80)\n"
        )
        assert proc.stderr == b""
        assert fetch_workspace(db, slug) is None

    def test_zero_projects_resolves_exact_members(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        member = seed.user()
        slug = tag("ws-zero")
        # Unknown addresses never match; spaced addresses keep the
        # space and match nothing either.
        stdin = (f"WS Five\n{slug}\n{creator['email']}\n{member['email']},"
                 f" unknown-{uuid.uuid4().hex[:8]}@example.com\n0\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == PROMPTS5 + b"Data is pushed to the queue\n"
        assert proc.stderr == b""
        ws = fetch_workspace(db, slug)
        assert ws[1] == "WS Five" and ws[4] == "UTC"
        assert re.fullmatch(r"#[0-9a-fA-F]{6}", ws[3])
        assert ws[5] is None and ws[6] is None  # logo, org size
        assert str(ws[7]) == creator["id"]  # owner
        members = workspace_member_emails(db, ws[0])
        assert sorted(row[0] for row in members) == sorted(
            [creator["email"], member["email"]])
        assert [(row[1], row[2]) for row in members] == [(20, True), (20, True)]

    def test_one_project_runs_task_synchronously(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        first = seed.user()
        second = seed.user()
        slug = tag("ws-task")
        members = f"{first['email']},{second['email']}"
        stdin = (f"WS Seven\n{slug}\n{creator['email']}\n{members}\n1\n"
                 f"6\n1\n5\n2\n2\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == (
            PROMPTS5 +
            b"Please provide the following details for project 1:\n"
            + COUNT_PROMPTS5
            + b"Data is pushed to the queue\n"
        )
        assert proc.stderr == b""
        ws = fetch_workspace(db, slug)
        assert [row[0] for row in workspace_member_emails(db, ws[0])] == (
            sorted([creator["email"], first["email"], second["email"]]))
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "projects" WHERE "workspace_id" = %s'
                ' AND "deleted_at" IS NULL', (ws[0],))
            assert cur.fetchone()[0] == 1
        # issues = 6 + 2 intake; cycles = 1 + 1 off-by-one.
        assert count_project_children(db, ws[0], "issues") == 8
        assert count_project_children(db, ws[0], "cycles") == 2
        assert count_project_children(db, ws[0], "modules") == 5
        assert count_project_children(db, ws[0], "states") == 5
        # Labels: 50 attempts with conflicts ignored. Python's seeded
        # Faker yields 40 distinct (project, name) pairs; the Rust
        # task draws from a 20-name approximation pool
        # (`COLOR_NAMES` in jobs dummy_data.rs) over a seeded RNG, so
        # exactly 20 distinct rows persist, deterministically. The
        # count pins the seeded implementation against regressions —
        # value parity with Faker was never the port's design (see the
        # `seeded_fake` doc comment).
        assert count_project_children(db, ws[0], "labels") == 20
        assert count_project_children(db, ws[0], "intake_issues") == 2
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "project_pages" pp'
                ' JOIN "projects" p ON p."id" = pp."project_id"'
                ' WHERE p."workspace_id" = %s', (ws[0],))
            assert cur.fetchone()[0] == 2  # pages linked

    def test_two_projects_zero_counts(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        slug = tag("ws-two")
        stdin = (f"WS Ten\n{slug}\n{creator['email']}\n\n2\n0\n0\n0\n0\n"
                 f"0\n0\n0\n0\n0\n0\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout == (
            PROMPTS5 +
            b"Please provide the following details for project 1:\n"
            + COUNT_PROMPTS5
            + b"Please provide the following details for project 2:\n"
            + COUNT_PROMPTS5
            + b"Data is pushed to the queue\n"
        )
        assert proc.stderr == b""
        ws = fetch_workspace(db, slug)
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "projects" WHERE "workspace_id" = %s'
                ' AND "deleted_at" IS NULL', (ws[0],))
            assert cur.fetchone()[0] == 2

    def test_second_project_failure_keeps_first(self, rust_bin, db):
        seed = Seed(db)
        creator = seed.user()
        slug = tag("ws-partial")
        stdin = (f"W\n{slug}\n{creator['email']}\n\n2\n6\n1\n5\n2\n2\n"
                 f"6\n1\n5\n2\nabc\n").encode()
        proc = run_ops(rust_bin, "create_dummy_data", stdin=stdin)
        assert proc.returncode == 0
        assert proc.stdout.endswith(
            b"Command errored out invalid literal for int() with base 10:"
            b" 'abc'\n"
        )
        assert proc.stderr == b""
        ws = fetch_workspace(db, slug)
        with db.cursor() as cur:
            cur.execute(
                'SELECT COUNT(*) FROM "projects" WHERE "workspace_id" = %s'
                ' AND "deleted_at" IS NULL', (ws[0],))
            assert cur.fetchone()[0] == 1  # first task run persisted
