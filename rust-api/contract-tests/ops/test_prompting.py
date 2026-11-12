# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""F37-09 contract: prompting ops commands (PIDASHCONV-810, D-37).

``pidash-api ops <command>`` must behave byte-for-byte like the Django
management commands it ports
(``apps/api/pi_dash/prompting/management/commands/``): same argv, same
stdout/stderr bytes, same exit codes, same database writes. Every test
below seeds two identically-migrated scratch databases with the same
rows (same UUIDs — flagged lines print workspace ids), runs the Django
oracle against one and the Rust binary against the other, and compares
the process results plus the resulting table snapshots.

This suite has no HTTP surface, so ``BASE_URL`` does not apply; it is
driven by database URLs plus the two binaries under test:

- ``DATABASE_URL`` (required) — psycopg URL of the Rust-side scratch DB.
- ``ORACLE_DATABASE_URL`` (required) — psycopg URL of the Django-side
  scratch DB. Both DBs must be migrated (``manage.py migrate``); each
  test truncates ``prompt_template`` / ``prompt_section_override`` and
  reseeds, so the two DBs stay mutually independent.
- ``PIDASH_BIN`` — the Rust binary (default:
  ``<repo>/rust-api/target/debug/pidash-api``; build it with
  ``cargo build -p pidash-api-bin`` first).
- ``DJANGO_PYTHON`` — interpreter with the Django deps installed
  (default: the pytest interpreter itself).
- ``MANAGE_PY`` (default: ``<repo>/apps/api/manage.py``),
  ``DJANGO_SETTINGS_MODULE`` (default: ``pi_dash.settings.test``),
  ``DJANGO_API_DIR`` (default: ``<repo>/apps/api``, added to
  ``PYTHONPATH`` for the oracle runs).

Two deliberate comparison rules, both inherited from the sources:

- The revalidate scan has no ``ORDER BY`` (Django adds none), so the
  multi-row test compares stdout as a sorted line multiset plus the
  (always last) summary line — the single-row tests compare raw bytes.
- A render-failure detail comes from the template engine, and the
  prompting domain knowingly ports minijinja prose where Python has
  Jinja2 prose (see the engine-message caveat in
  ``crates/services/src/prompting/composer.rs``) — read-only kernels
  this issue must not fork. That one test compares the attribution
  wrapper prefix plus identical flag/DB outcomes; every other message
  (unknown-key, locked-section, length-cap, summaries) is static text
  and is compared byte-for-byte.
"""

from __future__ import annotations

import os
import subprocess
import sys
import uuid
from pathlib import Path

import psycopg
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

REPO_ROOT = Path(__file__).resolve().parents[3]


def _required(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"contract-tests/ops requires {name} to be set")
    return value


# NOTE: env is resolved at fixture/run time, never at import time, so a bare
# `pytest --collect-only` (CI `collect` job) succeeds with no env set. Every
# test uses the `pair` fixture (directly or via `owner`), which requires the
# URLs and both binaries before touching either database.
BIN = Path(os.environ.get("PIDASH_BIN", REPO_ROOT / "rust-api/target/debug/pidash-api"))
DJANGO_PYTHON = os.environ.get("DJANGO_PYTHON", sys.executable)
MANAGE_PY = Path(os.environ.get("MANAGE_PY", REPO_ROOT / "apps/api/manage.py"))
DJANGO_SETTINGS = os.environ.get("DJANGO_SETTINGS_MODULE", "pi_dash.settings.test")
DJANGO_API_DIR = os.environ.get("DJANGO_API_DIR", str(REPO_ROOT / "apps/api"))

# (stdout label, PromptTemplate.name row, Django command == ops subcommand)
KINDS = [
    ("default", "coding-task", "reseed_default_template"),
    ("review", "review", "reseed_review_template"),
    ("test", "test", "reseed_test_template"),
]

KNOWN_KEY = "implementation"  # overridable section both backends carry
LOCKED_KEY = "pidash-cli"  # locked section: deterministic validation error
UNKNOWN_KEY = "no-such-section-810"  # absent from the registry, deterministically
CLEAN_BODY = "Custom workspace guidance for the ops suite."
BROKEN_RENDER_BODY = "{{ missing.nope }}"
MAX_BODY_LENGTH = 100_000  # prompting/registry.py: MAX_SECTION_BODY_LENGTH
OLD_TS = "2020-01-01T00:00:00+00:00"  # backdated stamp: any write clearly moves it


def _check_prereqs() -> None:
    _required("DATABASE_URL")
    _required("ORACLE_DATABASE_URL")
    if not BIN.is_file():
        raise RuntimeError(f"Rust binary not found at {BIN} (cargo build -p pidash-api-bin)")
    if not MANAGE_PY.is_file():
        raise RuntimeError(f"manage.py not found at {MANAGE_PY}")


# --------------------------------------------------------------------------
# Process runners
# --------------------------------------------------------------------------


def run_oracle(command: str, *args: str) -> subprocess.CompletedProcess[bytes]:
    """Run the Django management command against the oracle DB."""
    env = dict(os.environ)
    env["DATABASE_URL"] = _required("ORACLE_DATABASE_URL")
    env["DJANGO_SETTINGS_MODULE"] = DJANGO_SETTINGS
    env["PYTHONPATH"] = DJANGO_API_DIR + os.pathsep + env.get("PYTHONPATH", "")
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    return subprocess.run(
        [DJANGO_PYTHON, str(MANAGE_PY), command, *args],
        capture_output=True,
        env=env,
        timeout=300,
    )


def run_rust(command: str, *args: str) -> subprocess.CompletedProcess[bytes]:
    """Run the Rust port (``pidash-api ops <command>``) against the Rust DB."""
    env = dict(os.environ)
    env["DATABASE_URL"] = _required("DATABASE_URL")
    return subprocess.run(
        [str(BIN), "ops", command, *args],
        capture_output=True,
        env=env,
        timeout=300,
    )


def assert_cli_parity(
    rust: subprocess.CompletedProcess[bytes],
    oracle: subprocess.CompletedProcess[bytes],
) -> None:
    """stdout/stderr/exit code must match byte for byte (F37-09)."""
    assert rust.returncode == oracle.returncode == 0, (rust, oracle)
    assert rust.stdout == oracle.stdout, (rust.stdout, oracle.stdout)
    assert rust.stderr == oracle.stderr, (rust.stderr, oracle.stderr)


# --------------------------------------------------------------------------
# Dual-database fixtures (same rows, same UUIDs, both sides)
# --------------------------------------------------------------------------


@pytest.fixture()
def pair():
    """Autocommit connections to both scratch DBs with clean prompt tables."""
    _check_prereqs()
    rust = psycopg.connect(_required("DATABASE_URL"), autocommit=True)
    oracle = psycopg.connect(_required("ORACLE_DATABASE_URL"), autocommit=True)
    try:
        for conn in (rust, oracle):
            with conn.cursor() as cur:
                cur.execute("TRUNCATE prompt_section_override, prompt_template")
        yield (rust, oracle)
    finally:
        rust.close()
        oracle.close()


@pytest.fixture()
def owner(pair) -> str:
    """One user id present on both DBs (workspace owner)."""
    uid = str(uuid.uuid4())
    email = f"ops810-{uid[:8]}@example.com"
    for conn in pair:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO users (id, password, username, email, first_name,
                    last_name, avatar, date_joined, created_at, updated_at,
                    last_location, created_location, is_superuser, is_managed,
                    is_password_expired, is_active, is_staff, is_email_verified,
                    is_password_autoset, token, user_timezone, last_login_ip,
                    last_logout_ip, last_login_medium, last_login_uagent, is_bot,
                    display_name, is_email_valid, is_password_reset_required)
                VALUES (%s, '!', %s, %s, 'Ops', 'Suite',
                    '', now(), now(), now(), '', '', false, false, false,
                    true, false, true, false, '', 'UTC', '', '', '', '',
                    false, 'Ops Suite', true, false)""",
                (uid, email, email),
            )
    return uid


def make_workspace(pair, owner_id: str) -> str:
    """One workspace id present on both DBs (one per override row: the
    partial unique index on (workspace, section_key) forbids two active
    workspace-level rows for the same key)."""
    wid = str(uuid.uuid4())
    for conn in pair:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO workspaces (id, name, slug, owner_id, created_by_id,
                    updated_by_id, timezone, background_color, created_at, updated_at)
                VALUES (%s, 'Ops WS', %s, %s, %s, %s, 'UTC', '', now(), now())""",
                (wid, f"ops810-{wid[:8]}", owner_id, owner_id, owner_id),
            )
    return wid


def add_template(
    pair,
    *,
    name: str,
    body: str,
    workspace_id: str | None = None,
    version: int = 1,
    is_active: bool = True,
    updated_at: str = OLD_TS,
) -> str:
    """Insert the same template row (same id) on both DBs; return its id."""
    row_id = str(uuid.uuid4())
    for conn in pair:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO prompt_template (id, workspace_id, name, body,
                    is_active, version, updated_by_id, created_at, updated_at)
                VALUES (%s, %s, %s, %s, %s, %s, NULL, %s::timestamptz,
                    %s::timestamptz)""",
                (row_id, workspace_id, name, body, is_active, version, updated_at, updated_at),
            )
    return row_id


def add_override(
    pair,
    workspace_id: str,
    section_key: str,
    body: str,
    *,
    user_id: str | None = None,
    is_active: bool = True,
    version: int = 1,
    needs_attention: bool = False,
    updated_at: str = OLD_TS,
) -> str:
    """Insert the same override row (same id) on both DBs; return its id."""
    row_id = str(uuid.uuid4())
    for conn in pair:
        with conn.cursor() as cur:
            cur.execute(
                """INSERT INTO prompt_section_override (id, workspace_id, user_id,
                    section_key, body, is_active, version, needs_attention,
                    updated_by_id, created_at, updated_at)
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, NULL,
                    %s::timestamptz, %s::timestamptz)""",
                (
                    row_id,
                    workspace_id,
                    user_id,
                    section_key,
                    body,
                    is_active,
                    version,
                    needs_attention,
                    updated_at,
                    updated_at,
                ),
            )
    return row_id


def template_snapshot(conn) -> list[tuple]:
    """Logical template state: everything but the id (each backend mints
    its own on create) and the volatile timestamps."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT workspace_id, name, body, is_active, version, updated_by_id"
            " FROM prompt_template"
        )
        return sorted(cur.fetchall(), key=lambda r: (str(r[0]), r[1]))


def override_snapshot(conn) -> list[tuple]:
    """Logical override state: ids included (tests seed the same ids on
    both sides), volatile timestamps excluded."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, workspace_id, user_id, section_key, body, is_active,"
            " version, needs_attention, updated_by_id"
            " FROM prompt_section_override"
        )
        return sorted(cur.fetchall(), key=lambda r: (str(r[1]), str(r[2]), r[3], str(r[0])))


def template_ids(conn) -> list[str]:
    with conn.cursor() as cur:
        cur.execute("SELECT id FROM prompt_template")
        return sorted(str(r[0]) for r in cur.fetchall())


def template_rows_by_id(conn) -> dict:
    """Full template rows keyed by id (for same-seeded multi-row tests)."""
    with conn.cursor() as cur:
        cur.execute(
            "SELECT id, workspace_id, name, body, is_active, version,"
            " updated_by_id FROM prompt_template"
        )
        return {str(r[0]): r for r in cur.fetchall()}


def updated_at_of(conn, table: str, row_id: str):
    with conn.cursor() as cur:
        cur.execute(f"SELECT updated_at FROM {table} WHERE id = %s", (row_id,))
        row = cur.fetchone()
        return row[0] if row else None


def assert_db_parity(pair) -> None:
    """Both backends must leave the same logical rows behind."""
    rust, oracle = pair
    assert template_snapshot(rust) == template_snapshot(oracle)
    assert override_snapshot(rust) == override_snapshot(oracle)


# --------------------------------------------------------------------------
# reseed_{default,review,test}_template [--force]
# --------------------------------------------------------------------------


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_missing_row_creates(pair, label, name, command):
    rust_proc, oracle_proc = run_rust(command), run_oracle(command)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: created\n".encode()
    rust, oracle = pair
    assert template_snapshot(rust) == template_snapshot(oracle)
    (row,) = template_snapshot(rust)
    workspace_id, row_name, body, is_active, version, updated_by = row
    assert workspace_id is None  # global row
    assert row_name == name
    assert body  # the fresh body; equality across DBs pins fragment/const parity
    assert is_active is True
    assert version == 1
    assert updated_by is None


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_identical_body_skips_without_write(pair, label, name, command):
    # Each backend first creates from its own fresh body; the bodies must
    # agree or nothing below is comparable.
    run_rust(command)
    run_oracle(command)
    rust, oracle = pair
    assert template_snapshot(rust) == template_snapshot(oracle)
    (row,) = template_snapshot(rust)
    (rust_id,) = template_ids(rust)
    (oracle_id,) = template_ids(oracle)
    before = (
        updated_at_of(rust, "prompt_template", rust_id),
        updated_at_of(oracle, "prompt_template", oracle_id),
    )

    rust_proc, oracle_proc = run_rust(command), run_oracle(command)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: skipped\n".encode()
    assert template_snapshot(rust) == template_snapshot(oracle) == [row]
    assert updated_at_of(rust, "prompt_template", rust_id) == before[0]
    assert updated_at_of(oracle, "prompt_template", oracle_id) == before[1]


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_force_with_identical_body_still_skips(pair, label, name, command):
    # The quirk: `force and existing.body != body` — force alone never writes.
    run_rust(command)
    run_oracle(command)
    rust, oracle = pair
    (row,) = template_snapshot(rust)
    (rust_id,) = template_ids(rust)
    (oracle_id,) = template_ids(oracle)
    before = (
        updated_at_of(rust, "prompt_template", rust_id),
        updated_at_of(oracle, "prompt_template", oracle_id),
    )

    rust_proc, oracle_proc = run_rust(command, "--force"), run_oracle(command, "--force")
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: skipped\n".encode()
    assert template_snapshot(rust) == template_snapshot(oracle) == [row]
    assert updated_at_of(rust, "prompt_template", rust_id) == before[0]
    assert updated_at_of(oracle, "prompt_template", oracle_id) == before[1]


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_stale_body_skips_without_force(pair, label, name, command):
    row_id = add_template(pair, name=name, body="stale body", version=3)
    rust, oracle = pair
    before = (
        updated_at_of(rust, "prompt_template", row_id),
        updated_at_of(oracle, "prompt_template", row_id),
    )
    rust_proc, oracle_proc = run_rust(command), run_oracle(command)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: skipped\n".encode()
    assert template_snapshot(rust) == template_snapshot(oracle)
    (row,) = template_snapshot(rust)
    assert row[2] == "stale body" and row[4] == 3
    assert updated_at_of(rust, "prompt_template", row_id) == before[0]
    assert updated_at_of(oracle, "prompt_template", row_id) == before[1]


@pytest.mark.parametrize("label,name,command", KINDS)
@pytest.mark.parametrize("old_version,new_version", [(0, 1), (3, 4)])
def test_reseed_force_refreshes_stale_body(pair, label, name, command, old_version, new_version):
    # `(version or 0) + 1`, re-activation included: the seed row starts cold.
    row_id = add_template(
        pair, name=name, body="stale body", version=old_version, is_active=False
    )
    rust, oracle = pair
    before = (
        updated_at_of(rust, "prompt_template", row_id),
        updated_at_of(oracle, "prompt_template", row_id),
    )
    rust_proc, oracle_proc = run_rust(command, "--force"), run_oracle(command, "--force")
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: refreshed\n".encode()
    assert template_snapshot(rust) == template_snapshot(oracle)
    (row,) = template_snapshot(rust)
    assert row[2] != "stale body"
    assert row[3] is True
    assert row[4] == new_version
    assert row[5] is None  # refresh never touches updated_by
    assert updated_at_of(rust, "prompt_template", row_id) != before[0]
    assert updated_at_of(oracle, "prompt_template", row_id) != before[1]


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_refresh_targets_newest_global_row(pair, label, name, command):
    old_id = add_template(
        pair, name=name, body="older body", version=7, updated_at="2019-01-01T00:00:00+00:00"
    )
    new_id = add_template(
        pair, name=name, body="newer body", version=2, updated_at="2021-06-01T00:00:00+00:00"
    )
    rust_proc, oracle_proc = run_rust(command, "--force"), run_oracle(command, "--force")
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == f"{label} template: refreshed\n".encode()
    assert_db_parity(pair)
    rust, oracle = pair
    for conn in (rust, oracle):
        rows = template_rows_by_id(conn)
        assert rows[old_id][3] == "older body" and rows[old_id][5] == 7  # untouched
        assert rows[new_id][3] != "newer body" and rows[new_id][5] == 3  # refreshed


@pytest.mark.parametrize("label,name,command", KINDS)
def test_reseed_leaves_workspace_row_alone(pair, owner, label, name, command):
    wid = make_workspace(pair, owner)
    ws_id = add_template(pair, name=name, body="workspace body", workspace_id=wid, version=5)
    rust, oracle = pair
    before = (
        updated_at_of(rust, "prompt_template", ws_id),
        updated_at_of(oracle, "prompt_template", ws_id),
    )
    rust_proc, oracle_proc = run_rust(command), run_oracle(command)
    assert_cli_parity(rust_proc, oracle_proc)
    # No global row exists, so a global row is created; the workspace row
    # is invisible to the lookup and stays exactly as seeded.
    assert rust_proc.stdout == f"{label} template: created\n".encode()
    assert_db_parity(pair)
    for conn in (rust, oracle):
        rows = template_rows_by_id(conn)
        assert rows[ws_id][3] == "workspace body" and rows[ws_id][5] == 5
    assert updated_at_of(rust, "prompt_template", ws_id) == before[0]
    assert updated_at_of(oracle, "prompt_template", ws_id) == before[1]


# --------------------------------------------------------------------------
# revalidate_section_overrides [--clear]
# --------------------------------------------------------------------------

REVALIDATE = "revalidate_section_overrides"
ZERO_SUMMARY = b"checked 0 active override(s): 0 newly flagged, 0 cleared.\n"


def test_revalidate_empty_scan(pair):
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == ZERO_SUMMARY
    assert_db_parity(pair)


def test_revalidate_unknown_key_flags_without_rendering(pair, owner):
    wid = make_workspace(pair, owner)
    row_id = add_override(pair, wid, UNKNOWN_KEY, "anything {{ here }}")
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        f"flagged {wid}/{UNKNOWN_KEY} (user=None): section key no longer exists\n"
        "checked 1 active override(s): 1 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)
    rust, _oracle = pair
    (row,) = override_snapshot(rust)
    assert str(row[0]) == row_id and row[7] is True  # needs_attention set


def test_revalidate_already_flagged_broken_row_stays_silent(pair, owner):
    wid = make_workspace(pair, owner)
    row_id = add_override(pair, wid, UNKNOWN_KEY, "body", needs_attention=True)
    rust, oracle = pair
    before = (
        updated_at_of(rust, "prompt_section_override", row_id),
        updated_at_of(oracle, "prompt_section_override", row_id),
    )
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        "checked 1 active override(s): 0 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)
    assert updated_at_of(rust, "prompt_section_override", row_id) == before[0]
    assert updated_at_of(oracle, "prompt_section_override", row_id) == before[1]


def test_revalidate_clean_row_is_kept_silently(pair, owner):
    wid = make_workspace(pair, owner)
    row_id = add_override(pair, wid, KNOWN_KEY, CLEAN_BODY)
    rust, oracle = pair
    before = (
        updated_at_of(rust, "prompt_section_override", row_id),
        updated_at_of(oracle, "prompt_section_override", row_id),
    )
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        "checked 1 active override(s): 0 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)
    assert updated_at_of(rust, "prompt_section_override", row_id) == before[0]
    assert updated_at_of(oracle, "prompt_section_override", row_id) == before[1]


def test_revalidate_render_failure_flags_with_engine_detail(pair, owner):
    # The attribution wrapper is byte-exact on both sides; only the
    # trailing engine prose differs (minijinja vs Jinja2 StrictUndefined —
    # the prompting domain's documented caveat, not forked here).
    wid = make_workspace(pair, owner)
    add_override(pair, wid, KNOWN_KEY, BROKEN_RENDER_BODY)
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert rust_proc.returncode == oracle_proc.returncode == 0
    assert rust_proc.stderr == oracle_proc.stderr == b""
    prefix = (
        f"flagged {wid}/{KNOWN_KEY} (user=None): override for section"
        f" {KNOWN_KEY!r} fails to render as part of the 'coding-task' prompt: "
    ).encode()
    for proc in (rust_proc, oracle_proc):
        lines = proc.stdout.split(b"\n")
        assert lines[0].startswith(prefix), proc.stdout
        assert len(lines[0]) > len(prefix), proc.stdout  # non-empty engine detail
        assert lines[1] == b"checked 1 active override(s): 1 newly flagged, 0 cleared."
        assert lines[2] == b""
    assert_db_parity(pair)
    rust, _oracle = pair
    (row,) = override_snapshot(rust)
    assert row[7] is True


def test_revalidate_locked_section_flags_byte_identical(pair, owner):
    wid = make_workspace(pair, owner)
    add_override(pair, wid, LOCKED_KEY, "nobody may override this")
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        f"flagged {wid}/{LOCKED_KEY} (user=None): section '{LOCKED_KEY}'"
        " is locked and cannot be overridden\n"
        "checked 1 active override(s): 1 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)


def test_revalidate_overlong_body_flags_byte_identical(pair, owner):
    wid = make_workspace(pair, owner)
    body = "x" * (MAX_BODY_LENGTH + 1)
    add_override(pair, wid, KNOWN_KEY, body)
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        f"flagged {wid}/{KNOWN_KEY} (user=None): override body exceeds"
        f" {MAX_BODY_LENGTH}-character limit (got {MAX_BODY_LENGTH + 1} characters)\n"
        "checked 1 active override(s): 1 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)


@pytest.mark.parametrize("clear", [False, True])
def test_revalidate_flag_clear_matrix(pair, owner, clear):
    # broken x flagged x clear, over unknown-key (deterministic) rows.
    specs = [
        (UNKNOWN_KEY, "broken", False),  # flags (maybe), never clears
        (UNKNOWN_KEY, "broken", True),  # stays flagged silently either way
        (KNOWN_KEY, CLEAN_BODY, False),  # untouched either way
        (KNOWN_KEY, CLEAN_BODY, True),  # clears only under --clear
    ]
    for key, body, flagged in specs:
        wid = make_workspace(pair, owner)
        add_override(pair, wid, key, body, needs_attention=flagged)
    args = ("--clear",) if clear else ()
    rust_proc, oracle_proc = run_rust(REVALIDATE, *args), run_oracle(REVALIDATE, *args)
    assert rust_proc.returncode == oracle_proc.returncode == 0
    assert rust_proc.stderr == oracle_proc.stderr == b""
    # One flagged line plus the summary; single-row order is deterministic
    # here (only one row can flag), so raw bytes still apply.
    assert rust_proc.stdout == oracle_proc.stdout
    summary = (
        "checked 4 active override(s): 1 newly flagged,"
        f" {1 if clear else 0} cleared.\n"
    ).encode()
    assert rust_proc.stdout.split(b"\n")[-2] == summary.rstrip(b"\n")
    assert_db_parity(pair)
    rust, _oracle = pair
    flags = sorted(r[7] for r in override_snapshot(rust))
    # broken-unflagged -> True; broken-flagged -> True; clean-unflagged ->
    # False; clean-flagged -> cleared or kept.
    assert flags == sorted([True, True, False, not clear])


def test_revalidate_personal_row_renders_user_scope_id(pair, owner):
    wid = make_workspace(pair, owner)
    add_override(pair, wid, UNKNOWN_KEY, "body", user_id=owner)
    rust_proc, oracle_proc = run_rust(REVALIDATE), run_oracle(REVALIDATE)
    assert_cli_parity(rust_proc, oracle_proc)
    assert rust_proc.stdout == (
        f"flagged {wid}/{UNKNOWN_KEY} (user={owner}): section key no longer exists\n"
        "checked 1 active override(s): 1 newly flagged, 0 cleared.\n"
    ).encode()
    assert_db_parity(pair)


def test_revalidate_multi_row_mixed_scan(pair, owner):
    # The scan has no ORDER BY, so flagged lines are compared as a sorted
    # multiset; the summary (always last) and the DB state compare exactly.
    wid_unknown = make_workspace(pair, owner)
    add_override(pair, wid_unknown, UNKNOWN_KEY, "body")
    wid_clean = make_workspace(pair, owner)
    add_override(pair, wid_clean, KNOWN_KEY, CLEAN_BODY, needs_attention=True)
    wid_locked = make_workspace(pair, owner)
    add_override(pair, wid_locked, LOCKED_KEY, "body")
    wid_quiet = make_workspace(pair, owner)
    add_override(pair, wid_quiet, KNOWN_KEY, CLEAN_BODY)
    wid_dead = make_workspace(pair, owner)
    dead_id = add_override(
        pair, wid_dead, UNKNOWN_KEY, "body", is_active=False, needs_attention=False
    )
    rust_proc, oracle_proc = (
        run_rust(REVALIDATE, "--clear"),
        run_oracle(REVALIDATE, "--clear"),
    )
    assert rust_proc.returncode == oracle_proc.returncode == 0
    assert rust_proc.stderr == oracle_proc.stderr == b""
    rust_lines = rust_proc.stdout.split(b"\n")
    oracle_lines = oracle_proc.stdout.split(b"\n")
    assert rust_lines[-1] == oracle_lines[-1] == b""
    assert rust_lines[-2] == oracle_lines[-2] == (
        b"checked 4 active override(s): 2 newly flagged, 1 cleared."
    )
    assert sorted(rust_lines[:-2]) == sorted(oracle_lines[:-2])
    assert len(rust_lines[:-2]) == 2  # unknown-key + locked-section lines
    assert_db_parity(pair)
    rust, oracle = pair
    for conn in (rust, oracle):
        rows = {str(r[0]): r for r in override_snapshot(conn)}
        assert rows[dead_id][5] is False and rows[dead_id][7] is False  # untouched


def test_revalidate_never_deletes_or_deactivates(pair, owner):
    # The command docstring guarantee: flag/clear flips needs_attention
    # only; row counts and is_active values never change.
    for key, body, flagged in [
        (UNKNOWN_KEY, "broken", False),
        (KNOWN_KEY, CLEAN_BODY, True),
        (KNOWN_KEY, BROKEN_RENDER_BODY, False),
    ]:
        wid = make_workspace(pair, owner)
        add_override(pair, wid, key, body, needs_attention=flagged)
    wid_dead = make_workspace(pair, owner)
    add_override(pair, wid_dead, UNKNOWN_KEY, "x", is_active=False)
    rust, oracle = pair
    before = (
        (len(override_snapshot(rust)), len(template_snapshot(rust))),
        (len(override_snapshot(oracle)), len(template_snapshot(oracle))),
    )
    for args in ((), ("--clear",)):
        rust_proc, oracle_proc = run_rust(REVALIDATE, *args), run_oracle(REVALIDATE, *args)
        assert rust_proc.returncode == oracle_proc.returncode == 0
    assert_db_parity(pair)
    for conn, (n_overrides, n_templates) in zip((rust, oracle), before):
        assert len(override_snapshot(conn)) == n_overrides
        assert len(template_snapshot(conn)) == n_templates
        actives = sorted(r[5] for r in override_snapshot(conn))
        assert actives == [False, True, True, True]
