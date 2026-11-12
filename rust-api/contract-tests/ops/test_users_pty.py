# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""PIDASHCONV-1267: interactive ``reset_password`` must not echo.

On a controlling terminal CPython's ``getpass.getpass`` opens
``/dev/tty``, writes the prompt there, turns echo off, reads the line,
and writes a newline. It prints ``Warning: Password input may be
echoed.`` only when there is no controlling terminal. This suite drives
both the Django oracle and the Rust binary under a real pty and pins the
interactive bytes and the stored hash.

The pty test needs two migrated scratch databases and a Django
interpreter:

- ``DATABASE_URL`` (Rust side), ``ORACLE_DATABASE_URL`` (Django side).
- ``DJANGO_PYTHON`` (default: this interpreter), ``MANAGE_PY``,
  ``DJANGO_SETTINGS_MODULE``, ``DJANGO_API_DIR``.

The piped-stdin fallback remains covered by ``test_users.py``.
"""

from __future__ import annotations

import errno
import os
import pty
import select
import sys
import time
import uuid
from pathlib import Path

import psycopg
import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from _harness.djangocrypto import verify_password  # noqa: E402
from _harness.seed import Seed  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
MANAGE_PY = Path(os.environ.get("MANAGE_PY", REPO_ROOT / "apps/api/manage.py"))
DJANGO_PYTHON = os.environ.get("DJANGO_PYTHON", sys.executable)
DJANGO_SETTINGS = os.environ.get("DJANGO_SETTINGS_MODULE", "pi_dash.settings.local")
DJANGO_API_DIR = os.environ.get("DJANGO_API_DIR", str(REPO_ROOT / "apps/api"))

STRONG_PASSWORD = "Tr0ub4dor&3"  # zxcvbn 4; accepted by both backends
ECHO_WARNING = b"Warning: Password input may be echoed."


def _required(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise pytest.UsageError(f"pty reset_password contract test requires {name}")
    return value


def _backend(backend: str, rust_bin: str) -> tuple[list[str], dict[str, str], str]:
    env = dict(os.environ)
    if backend == "rust":
        database_url = _required("DATABASE_URL")
        env["DATABASE_URL"] = database_url
        return [rust_bin, "ops", "reset_password"], env, database_url

    database_url = _required("ORACLE_DATABASE_URL")
    if not MANAGE_PY.is_file():
        raise pytest.UsageError(f"manage.py not found at {MANAGE_PY}")
    env.update(
        DATABASE_URL=database_url,
        DJANGO_SETTINGS_MODULE=DJANGO_SETTINGS,
        PYTHONPATH=DJANGO_API_DIR + os.pathsep + env.get("PYTHONPATH", ""),
        PYTHONDONTWRITEBYTECODE="1",
    )
    return [DJANGO_PYTHON, str(MANAGE_PY), "reset_password"], env, database_url


def _seed_user(database_url: str, email: str) -> None:
    conn = psycopg.connect(database_url, autocommit=True)
    try:
        with conn.cursor() as cur:
            cur.execute('DELETE FROM "users" WHERE "email" = %s', (email,))
        Seed(conn).user(email=email)
    finally:
        conn.close()


def _stored_hash(database_url: str, email: str) -> str:
    conn = psycopg.connect(database_url, autocommit=True)
    try:
        with conn.cursor() as cur:
            cur.execute('SELECT "password" FROM "users" WHERE "email" = %s', (email,))
            row = cur.fetchone()
    finally:
        conn.close()
    assert row is not None, f"user {email} disappeared"
    return row[0]


def _run_pty(argv: list[str], env: dict[str, str], timeout: float = 120.0):
    """Run ``argv`` with a controlling pty and type the password twice."""
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(argv[0], argv, env)

    output = bytearray()
    prompts = [
        (b"Password: ", STRONG_PASSWORD),
        (b"Password (again): ", STRONG_PASSWORD),
    ]
    sent = 0
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            readable, _, _ = select.select([fd], [], [], 0.2)
            if readable:
                try:
                    chunk = os.read(fd, 4096)
                except OSError as error:
                    if error.errno == errno.EIO:  # child closed the pty
                        break
                    raise
                if not chunk:
                    break
                output.extend(chunk)
            if sent < len(prompts) and prompts[sent][0] in output:
                # rpassword writes its prompt before it switches the
                # terminal to no-echo; a human always types after that
                # point. Give the child the same beat before sending.
                time.sleep(0.2)
                os.write(fd, prompts[sent][1].encode() + b"\n")
                sent += 1
        else:
            os.kill(pid, 9)
            raise AssertionError(f"pty child timed out: {argv!r}")
    finally:
        _, status = os.waitpid(pid, 0)
        os.close(fd)
    return bytes(output), os.waitstatus_to_exitcode(status)


@pytest.mark.parametrize("backend", ["django", "rust"])
def test_reset_password_pty_hides_input(backend, rust_bin):
    email = f"pty-{backend}-{uuid.uuid4().hex[:8]}@example.com"
    argv, env, database_url = _backend(backend, rust_bin)
    _seed_user(database_url, email)

    output, code = _run_pty([*argv, email], env)

    assert code == 0, output
    assert b"Password: " in output, output
    assert b"Password (again): " in output, output
    assert STRONG_PASSWORD.encode() not in output, output
    assert ECHO_WARNING not in output, output
    assert verify_password(STRONG_PASSWORD, _stored_hash(database_url, email))
