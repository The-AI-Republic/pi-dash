# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""D-37 ops `instance` group CLI parity (PIDASHCONV-809, fixtures F37-07/08).

Drives the built ``pidash-api ops instance …`` binary and the Django
``manage.py`` oracle side by side on scratch databases (template clones
of ``DATABASE_URL``), asserting byte-identical stdout/stderr/exit codes
plus matching DB rows. Outbound side effects go to local sinks: mail to
``_harness.sinks.SmtpSink``, the GitHub releases probe to
``FixedResponseStub`` (via ``PIDASH_RELEASES_URL``), and the
``instance_traces`` Celery message to an exclusive AMQP tap queue bound
to the ``celery`` exchange.

Environment (CI provides all of these; see
``rust-api-contract-tests-ops-instance.yml``):

- ``DATABASE_URL`` — migrated contract database (cloning template; never
  written directly).
- ``SECRET_KEY`` — Django secret (also used to decrypt seeded rows).
- ``REDIS_URL`` — configured but never connected by these commands.
- ``RABBITMQ_*``/``AMQP_URL`` — broker for the traces tap.
- ``PIDASH_API_BIN`` — optional override for the binary under test
  (default: ``rust-api/target/debug/pidash-api``; the workflow builds it
  first).

No skipped tests: every leg runs real code on both backends.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import socket
import subprocess
import sys
import time
import urllib.parse
import uuid
from contextlib import contextmanager
from email import message_from_string
from pathlib import Path

import pika
import pytest
from cryptography.fernet import Fernet

from _harness import db as dbh
from _harness.sinks import FixedResponseStub, SmtpSink

CONTRACT_ROOT = Path(__file__).resolve().parent.parent
RUST_ROOT = CONTRACT_ROOT.parent
API_ROOT = RUST_ROOT.parent / "apps" / "api"
MANAGE = API_ROOT / "manage.py"

TRACES_TASK = "pi_dash.license.bgtasks.tracer.instance_traces"
SECRET_KEY = "809test-secret-1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7"

# `strip_tags` output for the test template, recorded from the oracle
# (probe P18): the text part is derived, never hand-written.
EXPECTED_TEXT_BODY = (
    " \n\n"
    "    This is a test email sent to verify if email configuration is working"
    " as expected in your Pi Dash instance.\n\n"
    "Regards, Team Pi Dash \n"
)


# ---------------------------------------------------------------------------
# runners
# ---------------------------------------------------------------------------


def rust_bin() -> Path:
    override = os.environ.get("PIDASH_API_BIN")
    path = Path(override) if override else RUST_ROOT / "target" / "debug" / "pidash-api"
    if not path.is_file():
        raise RuntimeError(
            f"pidash-api binary missing at {path} (run: cargo build --bin pidash-api)"
        )
    return path


def _scrubbed_env() -> dict:
    env = dict(os.environ)
    for key in (
        "APP_VERSION",
        "PIDASH_RELEASES_URL",
        "PIDASH_CONFIG_ENV_KEYS",
        "SECRET_KEY",  # runners own the secret (drop_secret must truly drop it)
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ):
        env.pop(key, None)
    return env


def run_rust(
    db_url: str,
    *args: str,
    env_extra: dict | None = None,
    cwd: Path | None = None,
    drop_secret: bool = False,
):
    env = _scrubbed_env()
    env["DATABASE_URL"] = db_url
    if not drop_secret:
        env["SECRET_KEY"] = SECRET_KEY
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        [str(rust_bin()), "ops", "instance", *args],
        env=env,
        cwd=cwd or RUST_ROOT,
        capture_output=True,
        timeout=120,
    )


def run_django(
    db_url: str,
    *args: str,
    env_extra: dict | None = None,
    cwd: Path | None = None,
    drop_secret: bool = False,
):
    env = _scrubbed_env()
    env.update(
        {
            "DATABASE_URL": db_url,
            "PYTHONPATH": str(API_ROOT),
            "DJANGO_SETTINGS_MODULE": "pi_dash.settings.local",
            "EMAIL_BACKEND": "django.core.mail.backends.smtp.EmailBackend",
            "REDIS_URL": os.environ.get("REDIS_URL", "redis://localhost:6379/15"),
            "WEB_URL": "http://localhost",
            "APP_BASE_URL": "http://localhost",
        }
    )
    if not drop_secret:
        env["SECRET_KEY"] = SECRET_KEY
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        [sys.executable, str(MANAGE), *args],
        env=env,
        cwd=cwd or API_ROOT,
        capture_output=True,
        timeout=180,
    )


# ---------------------------------------------------------------------------
# scratch databases (template clones)
# ---------------------------------------------------------------------------


def _admin_url() -> tuple[str, str]:
    parts = urllib.parse.urlsplit(dbh.get_database_url())
    template = parts.path.lstrip("/")
    admin = urllib.parse.urlunsplit(
        (parts.scheme, parts.netloc, "/postgres", parts.query, parts.fragment)
    )
    return admin, template


@pytest.fixture()
def scratch_db():
    admin, template = _admin_url()
    name = f"ops809_{uuid.uuid4().hex[:12]}"
    with dbh.connect(admin) as conn, conn.cursor() as cur:
        cur.execute(f'CREATE DATABASE "{name}" TEMPLATE "{template}"')
    parts = urllib.parse.urlsplit(dbh.get_database_url())
    url = urllib.parse.urlunsplit(
        (parts.scheme, parts.netloc, f"/{name}", parts.query, parts.fragment)
    )
    yield url
    with dbh.connect(admin) as conn, conn.cursor() as cur:
        cur.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


@pytest.fixture()
def scratch_pair():
    """Two independent clones: Django leg (index 0), Rust leg (index 1)."""
    admin, template = _admin_url()
    parts = urllib.parse.urlsplit(dbh.get_database_url())
    names = [f"ops809_{uuid.uuid4().hex[:12]}" for _ in range(2)]
    with dbh.connect(admin) as conn, conn.cursor() as cur:
        for name in names:
            cur.execute(f'CREATE DATABASE "{name}" TEMPLATE "{template}"')
    yield [
        urllib.parse.urlunsplit(
            (parts.scheme, parts.netloc, f"/{name}", parts.query, parts.fragment)
        )
        for name in names
    ]
    with dbh.connect(admin) as conn, conn.cursor() as cur:
        for name in names:
            cur.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


# ---------------------------------------------------------------------------
# AMQP tap (exclusive queue bound to the celery exchange)
# ---------------------------------------------------------------------------


def broker_url() -> str:
    direct = os.environ.get("AMQP_URL")
    if direct:
        return direct

    def part(name: str, default: str) -> str:
        return os.environ.get(name) or default

    vhost = urllib.parse.quote(part("RABBITMQ_VHOST", "/"), safe="")
    user = part("RABBITMQ_USER", "guest")
    password = part("RABBITMQ_PASSWORD", "guest")
    host = part("RABBITMQ_HOST", "localhost")
    port = part("RABBITMQ_PORT", "5672")
    return f"amqp://{user}:{password}@{host}:{port}/{vhost}"


@contextmanager
def traces_tap():
    """Yield a drain() collecting fresh `instance_traces` messages.

    The tap queue is exclusive to this test, so other agents' traffic
    on the shared broker only adds filterable noise, never assertions.
    """
    connection = pika.BlockingConnection(pika.URLParameters(broker_url()))
    channel = connection.channel()
    queue = channel.queue_declare(
        queue="", exclusive=True, auto_delete=True
    ).method.queue
    channel.queue_bind(queue=queue, exchange="celery", routing_key="celery")

    def drain(timeout: float = 20.0) -> list[dict]:
        deadline = time.time() + timeout
        found: list[dict] = []
        while time.time() < deadline:
            method, properties, body = channel.basic_get(queue=queue, auto_ack=True)
            if method is None:
                if found:
                    return found
                time.sleep(0.2)
                continue
            headers = dict((properties.headers or {}))
            if headers.get("task") == TRACES_TASK:
                payload = json.loads(body)
                found.append(
                    {
                        "args": payload[0],
                        "kwargs": payload[1],
                        "headers": headers,
                        "content_type": properties.content_type,
                    }
                )
        return found

    try:
        yield drain
    finally:
        connection.close()


# ---------------------------------------------------------------------------
# seeding (raw SQL; the suite never imports Django)
# ---------------------------------------------------------------------------


def seed_project(
    db_url, workspace_id, identifier, created_at, *, lead_id=None, assignee_id=None
):
    pid = str(uuid.uuid4())
    dbh.execute(
        db_url,
        """insert into projects
           (created_at, updated_at, id, name, description, network, identifier,
            workspace_id, default_assignee_id, project_lead_id, module_view, cycle_view,
            issue_views_view, page_view, intake_view, is_time_tracking_enabled,
            is_issue_type_enabled, is_default, guest_view_all_features,
            members_can_edit_states, archive_in, close_in, logo_props, timezone,
            repo_url, base_branch, agent_default_interval_seconds, agent_default_max_ticks,
            agent_review_default_interval_seconds, agent_test_default_interval_seconds,
            agent_ticking_enabled, default_agent_executor)
           values (%s,%s,%s,%s,'',2,%s,%s,%s,%s,
             false,false,false,true,false,false,false,false,false,true,
             0,0,'{}','UTC','','main',10800,10,10800,10800,true,'local_runner')""",
        (
            created_at,
            created_at,
            pid,
            identifier,
            identifier,
            workspace_id,
            assignee_id,
            lead_id,
        ),
    )
    return pid


def seed_pod(
    db_url, project_id, workspace_id, name, *, created_by=None, description=None
):
    dbh.execute(
        db_url,
        """insert into pod
           (id, name, description, is_default, deleted_at, created_at, updated_at,
            created_by_id, workspace_id, project_id)
           values (%s,%s,%s,true,null,now(),now(),%s,%s,%s)""",
        (
            str(uuid.uuid4()),
            name,
            description
            if description is not None
            else "Auto-created default pod. Add tier pods anytime.",
            created_by,
            workspace_id,
            project_id,
        ),
    )


def seed_scheduler(db_url, workspace_id, slug):
    sid = str(uuid.uuid4())
    dbh.execute(
        db_url,
        """insert into schedulers
           (created_at, updated_at, id, slug, name, description, prompt, source,
            is_enabled, workspace_id, color)
           values (now(),now(),%s,%s,%s,'','','builtin',true,%s,'')""",
        (sid, slug, slug, workspace_id),
    )
    return sid


def seed_binding(db_url, scheduler_id, workspace_id, cron, created_at, *, enabled=True):
    bid = str(uuid.uuid4())
    dbh.execute(
        db_url,
        """insert into scheduler_bindings
           (created_at, updated_at, id, extra_context, enabled, last_error,
            workspace_id, scheduler_id, dtstart, tzid, rrule, rdates, exdates,
            outcome_mode, cron)
           values (%s,%s,%s,'',%s,'',%s,%s,%s,'UTC','','[]','[]','CREATE_ISSUE',%s)""",
        (
            created_at,
            created_at,
            bid,
            enabled,
            workspace_id,
            scheduler_id,
            created_at,
            cron,
        ),
    )
    return bid


def decrypt_value(secret: str, ciphertext: str | None) -> str | None:
    if ciphertext is None:
        return None
    if ciphertext == "":
        return ""
    key = base64.urlsafe_b64encode(
        hashlib.pbkdf2_hmac("sha256", secret.encode(), b"salt", 100000)
    )
    return Fernet(key).decrypt(ciphertext.encode()).decode()


def config_plaintext(db_url, secret=SECRET_KEY):
    rows = dbh.fetchall(
        db_url, "select key, value, category, is_encrypted from instance_configurations"
    )
    return {
        row["key"]: (
            decrypt_value(secret, row["value"])
            if row["is_encrypted"]
            else row["value"],
            row["category"],
            row["is_encrypted"],
        )
        for row in rows
    }


# ---------------------------------------------------------------------------
# configure_instance (F37-07)
# ---------------------------------------------------------------------------


def test_configure_fresh_seed_matches_oracle(scratch_pair):
    django_db, rust_db = scratch_pair
    env = {
        "EMAIL_HOST": "smtp.contract.test",
        "GOOGLE_CLIENT_SECRET": "contract-gsec",
        "GITHUB_APP_ID": "contract-app-id",
    }
    oracle = run_django(django_db, "configure_instance", env_extra=env)
    rust = run_rust(rust_db, "configure-instance", env_extra=env)
    assert oracle.returncode == 0, oracle.stderr
    assert rust.returncode == 0, rust.stderr
    assert rust.stdout == oracle.stdout
    assert rust.stderr == oracle.stderr == b""
    assert config_plaintext(django_db) == config_plaintext(rust_db)
    rows = config_plaintext(rust_db)
    assert len(rows) == 36
    assert rows["EMAIL_HOST"][0] == "smtp.contract.test"
    assert rows["GOOGLE_CLIENT_SECRET"][0] == "contract-gsec"
    assert rows["IS_GITEA_ENABLED"][0] == "0"
    assert "IS_GOOGLE_ENABLED" not in rows  # derived gate never opens (fixture BUGS)


def test_configure_second_run_all_exist(scratch_pair):
    django_db, rust_db = scratch_pair
    run_django(django_db, "configure_instance")
    run_rust(rust_db, "configure-instance")
    oracle = run_django(django_db, "configure_instance")
    rust = run_rust(rust_db, "configure-instance")
    assert oracle.returncode == rust.returncode == 0
    assert rust.stdout == oracle.stdout
    assert rust.stdout.count(b"configuration already exists\n") == 40


def test_configure_missing_secret_key(scratch_pair):
    django_db, rust_db = scratch_pair
    oracle = run_django(django_db, "configure_instance", drop_secret=True)
    rust = run_rust(rust_db, "configure-instance", drop_secret=True)
    assert oracle.returncode == rust.returncode == 1
    assert rust.stdout == oracle.stdout == b""
    assert (
        rust.stderr
        == oracle.stderr
        == b"CommandError: SECRET_KEY env variable is required.\n"
    )


def test_configure_derived_gate_opens(scratch_pair):
    django_db, rust_db = scratch_pair
    env = {
        "PIDASH_CONFIG_ENV_KEYS": "IS_GITEA_ENABLED",
        "GOOGLE_CLIENT_ID": "gid-contract",
        "GOOGLE_CLIENT_SECRET": "gsec-contract",
    }
    oracle = run_django(django_db, "configure_instance", env_extra=env)
    rust = run_rust(rust_db, "configure-instance", env_extra=env)
    assert oracle.returncode == 0, oracle.stderr
    assert rust.returncode == 0, rust.stderr
    assert rust.stdout == oracle.stdout
    assert rust.stdout.count(b"loaded with value from environment variable.\n") == 39
    rows = config_plaintext(rust_db)
    assert len(rows) == 39
    assert rows["IS_GOOGLE_ENABLED"] == ("1", "AUTHENTICATION", False)
    assert rows["IS_GITHUB_ENABLED"] == ("0", "AUTHENTICATION", False)
    assert rows["IS_GITLAB_ENABLED"] == ("0", "AUTHENTICATION", False)
    assert rows["IS_GITEA_ENABLED"] == ("0", "AUTHENTICATION", False)
    assert config_plaintext(django_db) == rows


# ---------------------------------------------------------------------------
# register_instance (F37-07)
# ---------------------------------------------------------------------------

DEAD_PROXY = {"HTTPS_PROXY": "http://127.0.0.1:9", "HTTP_PROXY": "http://127.0.0.1:9"}
DEAD_RELEASES = {"PIDASH_RELEASES_URL": "http://127.0.0.1:9/releases"}


def instance_row(db_url):
    return dbh.fetchone(
        db_url,
        "select instance_name, instance_id, current_version, latest_version,"
        " last_checked_at, is_test, edition from instances",
    )


def test_register_create_fallback_update_and_traces(scratch_pair):
    django_db, rust_db = scratch_pair
    # Same cwd on both legs: the package.json probe is cwd-relative.
    with traces_tap() as drain:
        oracle = run_django(
            django_db, "register_instance", "sig-contract", env_extra=DEAD_PROXY
        )
        rust = run_rust(
            rust_db,
            "register-instance",
            "sig-contract",
            env_extra=DEAD_RELEASES,
            cwd=API_ROOT,
        )
        assert oracle.returncode == 0, oracle.stderr
        assert rust.returncode == 0, rust.stderr
        assert rust.stdout == oracle.stdout
        assert (
            rust.stdout == b"Error checking for latest version\nInstance registered\n"
        )
        django_msgs = drain()
        # Update branch on the same rows.
        oracle2 = run_django(
            django_db, "register_instance", "sig-contract", env_extra=DEAD_PROXY
        )
        rust2 = run_rust(
            rust_db,
            "register-instance",
            "sig-contract",
            env_extra=DEAD_RELEASES,
            cwd=API_ROOT,
        )
        assert (
            oracle2.stdout
            == rust2.stdout
            == b"Error checking for latest version\nInstance already registered\n"
        )
        rust_msgs = drain()
    assert len(django_msgs) >= 1 and len(rust_msgs) >= 1
    for message in django_msgs + rust_msgs:
        assert message["args"] == []
        assert message["kwargs"] == {}
        assert message["content_type"] == "application/json"
    django_row, rust_row = instance_row(django_db), instance_row(rust_db)
    for key in (
        "instance_name",
        "current_version",
        "latest_version",
        "is_test",
        "edition",
    ):
        assert django_row[key] == rust_row[key], key
    assert django_row["latest_version"] == django_row["current_version"]  # fallback
    assert len(rust_row["instance_id"]) == 24  # token_hex(12)
    assert (
        abs(
            (
                rust_row["last_checked_at"] - django_row["last_checked_at"]
            ).total_seconds()
        )
        < 300
    )


def test_register_stub_tag_matches_live_oracle(scratch_pair):
    django_db, rust_db = scratch_pair
    oracle = run_django(django_db, "register_instance", "sig-live")
    assert oracle.stdout == b"Instance registered\n", (
        oracle.stdout
    )  # live GitHub reached
    live_tag = instance_row(django_db)["latest_version"]
    with FixedResponseStub(body=json.dumps({"tag_name": live_tag}).encode()) as stub:
        assert stub.requests == []
        rust = run_rust(
            rust_db,
            "register-instance",
            "sig-live",
            env_extra={"PIDASH_RELEASES_URL": stub.url},
            cwd=API_ROOT,
        )
    assert rust.returncode == 0, rust.stderr
    assert rust.stdout == b"Instance registered\n"
    assert instance_row(rust_db)["latest_version"] == live_tag
    assert len(stub.requests) == 1
    assert stub.requests[0]["method"] == "GET"
    headers = {key.lower(): value for key, value in stub.requests[0]["headers"].items()}
    assert headers.get("user-agent", "").startswith("pidash-api/")


def test_register_current_version_chain(scratch_pair, tmp_path):
    django_db, rust_db = scratch_pair
    # APP_VERSION wins on both backends.
    env = dict(DEAD_PROXY)
    env["APP_VERSION"] = "9.9.9-contract"
    rust_env = dict(DEAD_RELEASES)
    rust_env["APP_VERSION"] = "9.9.9-contract"
    assert (
        run_django(django_db, "register_instance", "s", env_extra=env).returncode == 0
    )
    assert (
        run_rust(rust_db, "register-instance", "s", env_extra=rust_env).returncode == 0
    )
    assert instance_row(django_db)["current_version"] == "9.9.9-contract"
    assert instance_row(rust_db)["current_version"] == "9.9.9-contract"
    # Empty APP_VERSION counts as unset; missing package.json falls back
    # with the error line (cwd without one).
    (tmp_path / "empty").mkdir()
    env = dict(DEAD_PROXY)
    env["APP_VERSION"] = ""
    rust_env = dict(DEAD_RELEASES)
    rust_env["APP_VERSION"] = ""
    oracle = run_django(
        django_db, "register_instance", "s", env_extra=env, cwd=tmp_path / "empty"
    )
    rust = run_rust(
        rust_db, "register-instance", "s", env_extra=rust_env, cwd=tmp_path / "empty"
    )
    assert rust.stdout == oracle.stdout
    assert rust.stdout == (
        b"Error checking for current version\n"
        b"Error checking for latest version\n"
        b"Instance already registered\n"
    )
    assert instance_row(django_db)["current_version"] == "v0.1.0"
    assert instance_row(rust_db)["current_version"] == "v0.1.0"
    # A package.json version is read silently (apps/api ships 1.3.0).
    env = dict(DEAD_PROXY)
    oracle = run_django(
        django_db, "register_instance", "s", env_extra=env, cwd=API_ROOT
    )
    rust = run_rust(
        rust_db, "register-instance", "s", env_extra=DEAD_RELEASES, cwd=API_ROOT
    )
    assert b"Error checking for current version" not in oracle.stdout
    assert rust.stdout == oracle.stdout
    assert (
        instance_row(rust_db)["current_version"]
        == instance_row(django_db)["current_version"]
    )


def test_register_empty_signature_rejected(scratch_pair):
    django_db, rust_db = scratch_pair
    oracle = run_django(django_db, "register_instance", "", env_extra=DEAD_PROXY)
    rust = run_rust(
        rust_db, "register-instance", "", env_extra=DEAD_RELEASES, cwd=API_ROOT
    )
    assert oracle.returncode == rust.returncode == 1
    assert rust.stdout == oracle.stdout
    assert (
        rust.stderr == oracle.stderr == b"CommandError: Machine signature is required\n"
    )
    assert dbh.fetchone(django_db, "select count(*) as n from instances")["n"] == 0
    assert dbh.fetchone(rust_db, "select count(*) as n from instances")["n"] == 0


# ---------------------------------------------------------------------------
# ensure_project_pods (F37-08)
# ---------------------------------------------------------------------------


def seed_pod_world(db_url):
    database = dbh.Database(db_url)
    owner = database.make_user("owner@t.local")
    lead = database.make_user("lead@t.local")
    assignee = database.make_user("assignee@t.local")
    workspace = database.make_workspace(
        "Pod WS", f"podws-{uuid.uuid4().hex[:8]}", owner["id"]
    )
    wid = workspace["id"]
    # Distinct created_at values pin the -created_at scan order.
    alpha = seed_project(db_url, wid, "ALPHA", "2026-01-01T00:00:00+00:00")
    seed_pod(db_url, alpha, wid, "ALPHA_pod_1")
    beta = seed_project(
        db_url, wid, "BETA", "2026-02-01T00:00:00+00:00", lead_id=lead["id"]
    )
    gamma = seed_project(
        db_url, wid, "GAMMA", "2026-03-01T00:00:00+00:00", assignee_id=assignee["id"]
    )
    return {
        "beta": beta,
        "gamma": gamma,
        "lead": lead["id"],
        "assignee": assignee["id"],
    }


def pod_shapes(db_url):
    return dbh.fetchall(
        db_url,
        "select p.identifier as project, pod.name, pod.description, pod.is_default,"
        " pod.created_by_id is not distinct from p.project_lead_id as by_lead,"
        " pod.created_by_id is not distinct from p.default_assignee_id as by_assignee"
        " from pod join projects p on p.id = pod.project_id order by pod.name",
    )


def test_pods_dry_run_create_and_rerun(scratch_pair):
    django_db, rust_db = scratch_pair
    seed_pod_world(django_db)
    seed_pod_world(rust_db)
    oracle = run_django(django_db, "ensure_project_pods", "--dry-run")
    rust = run_rust(rust_db, "ensure-project-pods", "--dry-run")
    assert oracle.returncode == rust.returncode == 0
    assert len(rust.stdout.splitlines()) == 4  # found + 2 missing + tail
    assert b"(GAMMA)" in rust.stdout.splitlines()[1]  # -created_at: newest first
    assert b"(BETA)" in rust.stdout.splitlines()[2]
    django_ids = [line.split()[1] for line in oracle.stdout.splitlines()[1:3]]
    rust_ids = [line.split()[1] for line in rust.stdout.splitlines()[1:3]]
    assert rust.stdout.replace(rust_ids[0], b"ID").replace(
        rust_ids[1], b"ID"
    ) == oracle.stdout.replace(django_ids[0], b"ID").replace(django_ids[1], b"ID")
    assert (
        dbh.fetchone(rust_db, "select count(*) as n from pod")["n"] == 1
    )  # dry-run writes nothing
    oracle = run_django(django_db, "ensure_project_pods")
    rust = run_rust(rust_db, "ensure-project-pods")
    assert (
        rust.stdout.splitlines()[-1]
        == oracle.stdout.splitlines()[-1]
        == b"Created 2 default pod(s)."
    )
    assert pod_shapes(django_db) == pod_shapes(rust_db)
    created = {row["project"]: row for row in pod_shapes(rust_db)}
    assert created["BETA"]["name"] == "BETA_pod_1"
    assert (
        created["BETA"]["description"]
        == "Auto-created default pod by ensure_project_pods."
    )
    assert created["BETA"]["is_default"] is True
    assert created["BETA"]["by_lead"] is True
    assert created["GAMMA"]["by_assignee"] is True
    oracle = run_django(django_db, "ensure_project_pods")
    rust = run_rust(rust_db, "ensure-project-pods")
    assert (
        rust.stdout
        == oracle.stdout
        == b"All projects already have a pod. Nothing to do.\n"
    )


def test_pods_no_projects(scratch_pair):
    django_db, rust_db = scratch_pair
    for flag in ([], ["--dry-run"]):
        oracle = run_django(django_db, "ensure_project_pods", *flag)
        rust = run_rust(rust_db, "ensure-project-pods", *flag)
        assert oracle.returncode == rust.returncode == 0
        assert (
            rust.stdout
            == oracle.stdout
            == b"All projects already have a pod. Nothing to do.\n"
        )


# ---------------------------------------------------------------------------
# test_email (F37-08)
# ---------------------------------------------------------------------------


def point_mail_at(db_url, host, port):
    dbh.execute(
        db_url,
        "update instance_configurations set value=%s where key='EMAIL_HOST'",
        (host,),
    )
    dbh.execute(
        db_url,
        "update instance_configurations set value=%s where key='EMAIL_PORT'",
        (str(port),),
    )
    dbh.execute(
        db_url,
        "update instance_configurations set value='0' where key in ('EMAIL_USE_TLS','EMAIL_USE_SSL')",
    )
    dbh.execute(
        db_url,
        "update instance_configurations set value='Ops <ops@example.com>' where key='EMAIL_FROM'",
    )


def parsed_mime(raw: str):
    message = message_from_string(raw)
    parts = {}
    for part in message.walk():
        content_type = part.get_content_type()
        if content_type in ("text/plain", "text/html"):
            parts[content_type] = part.get_payload(decode=True).decode()
    return {
        "subject": message["Subject"],
        "from": message["From"],
        "to": message["To"],
        "type": message.get_content_type(),
        **parts,
    }


def test_email_success_matches_oracle(scratch_pair):
    django_db, rust_db = scratch_pair
    run_django(django_db, "configure_instance")
    run_rust(rust_db, "configure-instance")
    with SmtpSink() as django_sink, SmtpSink() as rust_sink:
        point_mail_at(django_db, "127.0.0.1", django_sink.port)
        point_mail_at(rust_db, "127.0.0.1", rust_sink.port)
        oracle = run_django(django_db, "test_email", "contract@example.com")
        rust = run_rust(rust_db, "test-email", "contract@example.com")
    assert oracle.returncode == 0, oracle.stderr
    assert rust.returncode == 0, rust.stderr
    assert (
        rust.stdout
        == oracle.stdout
        == b"Trying to send test email...\nEmail successfully sent\n"
    )
    assert len(django_sink.messages) == len(rust_sink.messages) == 1
    assert django_sink.messages[0]["rcpt_tos"] == ["contract@example.com"]
    assert rust_sink.messages[0]["rcpt_tos"] == ["contract@example.com"]
    assert (
        rust_sink.messages[0]["mail_from"]
        == django_sink.messages[0]["mail_from"]
        == "ops@example.com"
    )
    django_mime = parsed_mime(django_sink.messages[0]["data"])
    rust_mime = parsed_mime(rust_sink.messages[0]["data"])
    assert rust_mime == django_mime
    assert rust_mime["subject"] == "Test email from Pi Dash"
    assert rust_mime["from"] == "Ops <ops@example.com>"
    assert rust_mime["to"] == "contract@example.com"
    # Decoded payloads keep MIME CRLFs; the literals use LF.
    assert rust_mime["text/plain"].replace("\r\n", "\n") == EXPECTED_TEXT_BODY
    template = (API_ROOT / "templates" / "emails" / "test_email.html").read_text()
    assert rust_mime["text/html"].replace("\r\n", "\n") == template
    assert django_mime["text/html"].replace("\r\n", "\n") == template


def test_email_delivery_failure_shape(scratch_pair):
    django_db, rust_db = scratch_pair
    run_django(django_db, "configure_instance")
    run_rust(rust_db, "configure-instance")
    # A freshly-closed port refuses connections on both backends.
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        closed = sock.getsockname()[1]
    point_mail_at(django_db, "127.0.0.1", closed)
    point_mail_at(rust_db, "127.0.0.1", closed)
    oracle = run_django(django_db, "test_email", "contract@example.com")
    rust = run_rust(rust_db, "test-email", "contract@example.com")
    assert oracle.returncode == rust.returncode == 0
    assert (
        rust.stdout.splitlines()[0]
        == oracle.stdout.splitlines()[0]
        == b"Trying to send test email..."
    )
    # The SMTP error text is client-specific; the shape is the contract.
    assert oracle.stdout.splitlines()[1].startswith(
        b"Error: Email could not be delivered due to "
    )
    assert rust.stdout.splitlines()[1].startswith(
        b"Error: Email could not be delivered due to "
    )


def test_email_receiver_required(scratch_pair):
    django_db, rust_db = scratch_pair
    oracle = run_django(django_db, "test_email", "")
    rust = run_rust(rust_db, "test-email", "")
    assert oracle.returncode == rust.returncode == 1
    assert rust.stdout == oracle.stdout == b""
    assert rust.stderr == oracle.stderr == b"CommandError: Receiver email is required\n"
    # Missing positional: argparse vs clap usage differs; only the exit
    # code is portable.
    oracle = run_django(django_db, "test_email")
    rust = run_rust(rust_db, "test-email")
    assert oracle.returncode == rust.returncode == 2


# ---------------------------------------------------------------------------
# dry_run_scheduler_migration (F37-08)
# ---------------------------------------------------------------------------

CRON_GONE = "scheduler_bindings.cron column no longer exists — migration 0140 has already run.\n".encode()


def test_dryrun_post_migration_notice(scratch_pair):
    django_db, rust_db = scratch_pair
    for flag in ([], ["--json"]):
        before = dbh.fetchone(
            django_db, "select count(*) as n from scheduler_bindings"
        )["n"]
        oracle = run_django(django_db, "dry_run_scheduler_migration", *flag)
        rust = run_rust(
            rust_db,
            "dry-run-scheduler-migration",
            *(["--json"] if flag == ["--json"] else []),
        )
        assert oracle.returncode == rust.returncode == 0
        assert rust.stdout == oracle.stdout == CRON_GONE
        assert rust.stderr == oracle.stderr == b""
        after = dbh.fetchone(rust_db, "select count(*) as n from scheduler_bindings")[
            "n"
        ]
        assert before == after == 0  # read-only


def seed_dryrun_world(db_url, slug_suffix):
    database = dbh.Database(db_url)
    owner = database.make_user(f"dry-{slug_suffix}@t.local")
    workspace = database.make_workspace("Dry WS", f"dryws-{slug_suffix}", owner["id"])
    other = database.make_workspace("Other WS", f"other-{slug_suffix}", owner["id"])
    scheduler = seed_scheduler(db_url, workspace["id"], f"sch-{slug_suffix}")
    dbh.execute(db_url, "alter table scheduler_bindings add column cron text")
    seed_binding(
        db_url, scheduler, workspace["id"], "*/15 * * * *", "2026-01-01T00:00:00+00:00"
    )
    seed_binding(
        db_url, scheduler, workspace["id"], "not a cron", "2026-01-02T00:00:00+00:00"
    )
    seed_binding(db_url, scheduler, workspace["id"], None, "2026-01-03T00:00:00+00:00")
    seed_binding(
        db_url, scheduler, other["id"], "0 9 * * *", "2026-01-04T00:00:00+00:00"
    )
    return {"workspace": workspace["slug"], "other": other["slug"]}


def test_dryrun_pre_migration_shapes(scratch_pair):
    django_db, rust_db = scratch_pair
    seed_dryrun_world(django_db, "dj")
    rust_world = seed_dryrun_world(rust_db, "rs")
    assert (
        dbh.fetchone(django_db, "select count(*) as n from scheduler_bindings")["n"]
        == 4
    )
    # The live Django branch cannot run: `.values("cron")` raises
    # FieldError (latent bug, ported as intent via raw SQL).
    oracle = run_django(django_db, "dry_run_scheduler_migration")
    assert oracle.returncode == 1
    assert oracle.stdout == b""
    assert b"FieldError" in oracle.stderr
    assert b"Cannot resolve keyword 'cron'" in oracle.stderr
    # The Rust port implements the evident intent with croniter-absent
    # end-state semantics (no croniter exists in Rust): every
    # convertible row is FAIL.
    rust = run_rust(rust_db, "dry-run-scheduler-migration", "--json")
    assert rust.returncode == 0, rust.stderr
    envelope = json.loads(rust.stdout)
    assert envelope["summary"] == {"total": 4, "match": 0, "mismatch": 0, "fail": 4}
    assert list(envelope.keys()) == ["summary", "rows"]
    by_cron = {row["cron"]: row for row in envelope["rows"]}
    assert set(by_cron) == {"*/15 * * * *", "not a cron", "", "0 9 * * *"}
    for row in envelope["rows"]:
        assert list(row.keys()) == [
            "binding_id",
            "workspace",
            "enabled",
            "cron",
            "cron_next_fire",
            "rrule",
            "rrule_next_fire",
            "verdict",
            "reason",
        ]
    valid = by_cron["*/15 * * * *"]
    assert valid["verdict"] == "FAIL"
    assert valid["workspace"] == rust_world["workspace"]
    assert valid["enabled"] is True
    assert valid["rrule"] == "FREQ=MINUTELY;INTERVAL=15"
    assert valid["cron_next_fire"] is None
    assert valid["rrule_next_fire"] is not None and valid["rrule_next_fire"].endswith(
        "+00:00"
    )
    assert valid["reason"] == "could not compute one or both next-fires"
    bad = by_cron["not a cron"]
    assert bad["verdict"] == "FAIL" and bad["rrule"] is None
    assert (
        bad["reason"]
        == "conversion error: cron must have exactly 5 fields, got 3: 'not a cron'"
    )
    assert by_cron[""]["reason"].startswith("conversion error: ")
    # Human render: header counts, per-entry lines, footer, review notice.
    rust = run_rust(rust_db, "dry-run-scheduler-migration")
    text = rust.stdout.decode()
    assert text.startswith(
        "Dry-run cron → RRULE for 4 binding(s) — MATCH=0 MISMATCH=0 FAIL=4\n"
    )
    assert f"workspace='{rust_world['workspace']}'" in text
    assert "cron:   '*/15 * * * *'" in text
    assert "rrule:  FREQ=MINUTELY;INTERVAL=15" in text
    assert "reason: could not compute one or both next-fires" in text
    assert "Totals — MATCH=0  MISMATCH=0  FAIL=4" in text
    assert (
        "Non-MATCH bindings need manual review before deploying migration 0140." in text
    )
    # --workspace narrows to one workspace's bindings.
    rust = run_rust(
        rust_db, "dry-run-scheduler-migration", "--workspace", rust_world["other"]
    )
    assert rust.stdout.startswith(b"Dry-run cron \xe2\x86\x92 RRULE for 1 binding(s)")
    rust = run_rust(rust_db, "dry-run-scheduler-migration", "--workspace", "nosuch")
    assert rust.returncode == 0
    assert rust.stdout == (
        b"Dry-run cron \xe2\x86\x92 RRULE for 0 binding(s) \xe2\x80\x94 MATCH=0 MISMATCH=0 FAIL=0\n"
        + b"=" * 80
        + b"\n\n"
        + b"=" * 80
        + b"\nTotals \xe2\x80\x94 MATCH=0  MISMATCH=0  FAIL=0\n"
    )
