# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Ops commands A: boot + storage contract suite (F37-01, F37-02).

Drives the built ``pidash-api ops <command>`` binary and asserts stdout /
stderr / exit codes byte for byte against the Django commands'
non-TTY output. See ``ops/__init__.py`` for the backend split (real S3 +
SigV4-verifying stub).

Environment (all required; the suite fails — never skips — without them):

* ``PIDASH_API_BIN`` — the built binary (default:
  ``rust-api/target/debug/pidash-api`` next to this checkout).
* ``DATABASE_URL`` — scratch Postgres (the suite creates and reseeds
  ``django_migrations`` itself; nothing else in the database is touched).
* ``REDIS_URL`` — scratch Redis database (the full-clear path runs a real
  ``FLUSHDB`` on it, like Django).
* ``AWS_S3_ENDPOINT_URL``, ``AWS_ACCESS_KEY_ID``,
  ``AWS_SECRET_ACCESS_KEY``, ``AWS_REGION`` — the real S3 backend for the
  happy paths. Buckets carry a uuid suffix, so reruns never collide.
"""

import ast
import base64
import hashlib
import hmac
import json
import os
import subprocess
import threading
import time
import urllib.parse
import uuid
import xml.etree.ElementTree as ET
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import psycopg
import pytest
import redis
import requests

# --------------------------------------------------------------------------
# configuration
# --------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parent.parent.parent.parent


def _require_env(name):
    value = os.environ.get(name)
    if not value:
        pytest.fail(f"ops suite needs {name} (see test module docstring)")
    return value


def pidash_bin():
    override = os.environ.get("PIDASH_API_BIN")
    if override:
        candidate = Path(override)
    else:
        candidate = Path(__file__).resolve().parent.parent / "target" / "debug" / "pidash-api"
    if not candidate.is_file():
        pytest.fail(
            f"pidash-api binary not found at {candidate} "
            "(build it: cargo build -p pidash-api-bin)"
        )
    return str(candidate)


def run_ops(*args, env_extra=None, env_del=(), cwd=None, timeout=60):
    """Run ``pidash-api ops ...``; return the completed process (text)."""
    env = dict(os.environ)
    for name in env_del:
        env.pop(name, None)
    env.update(env_extra or {})
    return subprocess.run(
        [pidash_bin(), "ops", *args],
        env=env,
        cwd=cwd,
        timeout=timeout,
        capture_output=True,
        text=True,
    )


def db_connect():
    return psycopg.connect(_require_env("DATABASE_URL"), autocommit=True)


def redis_client():
    return redis.Redis.from_url(_require_env("REDIS_URL"), decode_responses=True)


# The nine Django migration leaves (F37-01; also baked into the Rust
# binary). The ast cross-check below re-derives the five first-party ones
# from the checked-out migration files.
EXPECTED_LEAVES = [
    ("assistant", "0004_usersttconfig"),
    ("auth", "0012_alter_user_first_name_max_length"),
    ("contenttypes", "0002_remove_content_type_name"),
    ("db", "0167_wait_budget"),
    ("django_celery_beat", "0018_improve_crontab_helptext"),
    ("license", "0006_instance_is_current_version_deprecated"),
    ("prompting", "0007_reseed_directional_relations"),
    ("runner", "0029_drop_agentrun_trigger_blocker_completed"),
    ("sessions", "0001_initial"),
]
REPO_LEAVES = [leaf for leaf in EXPECTED_LEAVES if leaf[0] in {"assistant", "db", "license", "prompting", "runner"}]

UNBOUND_PERMISSIONS = (
    "cannot access local variable 'permissions' where it is not associated with a value"
)


# --------------------------------------------------------------------------
# SigV4: test-side signer + the stub's verifier
# --------------------------------------------------------------------------

SIGNED_HEADERS = "host;x-amz-content-sha256;x-amz-date"


def _hmac_sha256(key, message):
    return hmac.new(key, message, hashlib.sha256).digest()


def _signing_key(secret, date_stamp, region, service="s3"):
    key = ("AWS4" + secret).encode()
    for part in (date_stamp, region, service, "aws4_request"):
        key = _hmac_sha256(key, part.encode())
    return key


def _canonical_query(query):
    if not query:
        return ""
    pairs = []
    for part in query.split("&"):
        if "=" in part:
            name, value = part.split("=", 1)
            pairs.append(f"{name}={value}")
        else:
            pairs.append(f"{part}=")
    return "&".join(sorted(pairs))


def sign_s3(method, url, body, access_key, secret_key, region, amz_date=None, date_stamp=None):
    """Sign one S3 request exactly like the Rust client does; return headers."""
    parsed = urllib.parse.urlsplit(url)
    payload_hash = hashlib.sha256(body).hexdigest()
    if amz_date is None:
        now = time.gmtime()
        amz_date = time.strftime("%Y%m%dT%H%M%SZ", now)
        date_stamp = time.strftime("%Y%m%d", now)
    canonical = "\n".join(
        [
            method,
            parsed.path or "/",
            _canonical_query(parsed.query),
            f"host:{parsed.netloc}",
            f"x-amz-content-sha256:{payload_hash}",
            f"x-amz-date:{amz_date}",
            "",
            SIGNED_HEADERS,
            payload_hash,
        ]
    )
    scope = f"{date_stamp}/{region}/s3/aws4_request"
    string_to_sign = (
        f"AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{hashlib.sha256(canonical.encode()).hexdigest()}"
    )
    signature = hmac.new(
        _signing_key(secret_key, date_stamp, region), string_to_sign.encode(), hashlib.sha256
    ).hexdigest()
    return {
        "Authorization": (
            f"AWS4-HMAC-SHA256 Credential={access_key}/{scope}, "
            f"SignedHeaders={SIGNED_HEADERS}, Signature={signature}"
        ),
        "x-amz-date": amz_date,
        "x-amz-content-sha256": payload_hash,
    }


def verify_s3(method, path, query, headers, body, secret_key):
    """Re-verify one received request the way AWS would; True iff valid.

    Recomputes the signature over the received bytes and additionally
    requires ``x-amz-content-sha256`` to be the real payload hash (the
    Rust client always sends real hashes).
    """
    lowered = {name.lower(): value.strip() for name, value in headers.items()}
    auth = lowered.get("authorization", "")
    try:
        auth_params = auth.split(" ", 1)[1]
    except IndexError:
        return False
    fields = {}
    for part in auth_params.split(", "):
        name, _, value = part.partition("=")
        fields[name] = value
    try:
        credential = fields["Credential"]
        signed_headers = fields["SignedHeaders"]
        signature = fields["Signature"]
    except KeyError:
        return False
    try:
        _, date_stamp, region, service, _ = credential.split("/")
    except ValueError:
        return False
    if service != "s3" or not date_stamp or not region:
        return False
    amz_date = lowered.get("x-amz-date", "")
    if not amz_date:
        return False
    canonical_headers = ""
    for name in signed_headers.split(";"):
        if name not in lowered:
            return False
        canonical_headers += f"{name}:{lowered[name]}\n"
    payload_hash = lowered.get("x-amz-content-sha256", "")
    canonical = "\n".join(
        [
            method,
            path,
            _canonical_query(query),
            canonical_headers.rstrip("\n"),
            "",
            signed_headers,
            payload_hash,
        ]
    )
    scope = f"{date_stamp}/{region}/s3/aws4_request"
    string_to_sign = (
        f"AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{hashlib.sha256(canonical.encode()).hexdigest()}"
    )
    expected = hmac.new(
        _signing_key(secret_key, date_stamp, region), string_to_sign.encode(), hashlib.sha256
    ).hexdigest()
    if not hmac.compare_digest(expected, signature):
        return False
    return hmac.compare_digest(payload_hash, hashlib.sha256(body).hexdigest())


# Frozen botocore vector (2026-10-10T03:00:00Z, AKIDEXAMPLE/SECRETEXAMPLE,
# us-east-1, real payload hash): ``GET /my-bucket?list-type=2``. The
# verifier must accept it — this pins the verifier against an
# independent signer, so every stub test that passes proves the Rust
# signatures are genuine SigV4.
BOTOCORE_VECTOR_AUTH = (
    "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20261010/us-east-1/s3/aws4_request, "
    "SignedHeaders=host;x-amz-content-sha256;x-amz-date, "
    "Signature=223931058c89382f68a73a586a5e1fef18b33d93a6115b87732d58884ec1b2e3"
)
BOTOCORE_VECTOR_DATE = "20261010T030000Z"
BOTOCORE_VECTOR_PAYLOAD = (
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
)


def test_stub_verifier_accepts_botocore_vector():
    """The stub's verifier is real: it accepts botocore's own signature."""
    headers = {
        "host": "localhost:4566",
        "x-amz-content-sha256": BOTOCORE_VECTOR_PAYLOAD,
        "x-amz-date": BOTOCORE_VECTOR_DATE,
        "authorization": BOTOCORE_VECTOR_AUTH,
    }
    assert verify_s3("GET", "/my-bucket", "list-type=2", headers, b"", "SECRETEXAMPLE")
    tampered = dict(headers)
    tampered["authorization"] = BOTOCORE_VECTOR_AUTH[:-1] + (
        "0" if not BOTOCORE_VECTOR_AUTH.endswith("0") else "1"
    )
    assert not verify_s3("GET", "/my-bucket", "list-type=2", tampered, b"", "SECRETEXAMPLE")
    no_hash = dict(headers)
    del no_hash["x-amz-content-sha256"]
    assert not verify_s3("GET", "/my-bucket", "list-type=2", no_hash, b"", "SECRETEXAMPLE")


# --------------------------------------------------------------------------
# SigV4-verifying stub S3
# --------------------------------------------------------------------------

DROP = object()  # route marker: close the connection without responding


def _error_xml(code, message):
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        f"<Error><Code>{code}</Code><Message>{message}</Message></Error>"
    ).encode()


def _empty_list_xml(bucket):
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        "<ListBucketResult><EncodingType>url</EncodingType>"
        f"<Name>{bucket}</Name><KeyCount>0</KeyCount>"
        "</ListBucketResult>"
    ).encode()


def _echo_list_xml(bucket, keys):
    """Compliant list body: encoded keys plus the `<EncodingType>url</EncodingType>`
    echo (S3/MinIO/LocalStack all echo) — botocore decodes these."""
    items = "".join(f"<Contents><Key>{key}</Key></Contents>" for key in keys)
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        "<ListBucketResult><EncodingType>url</EncodingType>"
        f"<Name>{bucket}</Name><KeyCount>{len(keys)}</KeyCount>{items}</ListBucketResult>"
    ).encode()


def _raw_list_xml(bucket, keys):
    """Non-echoing list body: encoded keys WITHOUT the echo element —
    botocore passes these through raw (handlers.py:843-850)."""
    items = "".join(f"<Contents><Key>{key}</Key></Contents>" for key in keys)
    return (
        '<?xml version="1.0" encoding="UTF-8"?>'
        "<ListBucketResult>"
        f"<Name>{bucket}</Name><KeyCount>{len(keys)}</KeyCount>{items}</ListBucketResult>"
    ).encode()


class _StubHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _route(self):
        parsed = urllib.parse.urlsplit(self.path)
        parts = parsed.path.strip("/").split("/", 1)
        bucket = parts[0] if parts else ""
        query = urllib.parse.parse_qs(parsed.query, keep_blank_values=True)
        if "list-type" in query:
            kind = "list"
        elif "policy" in query:
            kind = "policy"
        elif len(parts) > 1:
            kind = "object"
        else:
            kind = "bucket"
        return bucket, kind, parsed.path, parsed.query

    def _handle(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        bucket, kind, path, query = self._route()
        stub = self.server.stub
        stub.log.append(
            {
                "method": self.command,
                "path": path,
                "query": query,
                "headers": dict(self.headers),
                "body": body,
            }
        )
        if not verify_s3(
            self.command, path, query, dict(self.headers), body, stub.secret_key
        ):
            return self._respond(
                403,
                _error_xml("SignatureDoesNotMatch", "The request signature we calculated does not match."),
            )
        script = stub.routes.get((self.command, bucket, kind))
        if script is None:
            return self._respond(404, b"")
        entry = script.pop(0) if isinstance(script, list) else script
        if entry is DROP:
            try:
                self.connection.shutdown(2)
            except OSError:
                pass
            try:
                self.connection.close()
            except OSError:
                pass
            self.close_connection = True
            return None
        if len(entry) == 3:
            status, payload, force_body = entry
        else:
            status, payload = entry
            force_body = False
        return self._respond(status, payload, force_body=force_body)

    def _respond(self, status, payload, force_body=False):
        self.send_response(status)
        self.send_header("Content-Type", "application/xml")
        self.send_header("Content-Length", str(len(payload)))
        if force_body and self.command == "HEAD":
            # Lie like a server that sends bodies on HEAD — then hang up,
            # so the unread bytes cannot poison the next request.
            self.send_header("Connection", "close")
        self.end_headers()
        if (self.command != "HEAD" or force_body) and payload:
            try:
                self.wfile.write(payload)
            except (BrokenPipeError, ConnectionResetError):
                pass
        return None

    do_HEAD = _handle
    do_GET = _handle
    do_PUT = _handle
    do_DELETE = _handle

    def log_message(self, *args):
        pass


class StubS3:
    """A scripted S3 stand-in that verifies every request's SigV4 signature.

    ``routes[(method, bucket, kind)]`` is either one ``(status, body)``
    tuple or a replay list consumed in order (``DROP`` entries close the
    connection without responding, for transport-failure paths).
    """

    def __init__(self, secret_key):
        self.secret_key = secret_key
        self.routes = {}
        self.log = []
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), _StubHandler)
        self.server.stub = self
        self.server.handle_error = lambda *a, **k: None
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def start(self):
        self.thread.start()
        return self

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=10)


@pytest.fixture()
def stub_s3():
    stub = StubS3(secret_key="stubtestsecret").start()
    yield stub
    stub.stop()


STUB_ENV = {
    "AWS_ACCESS_KEY_ID": "stubtestkey",
    "AWS_SECRET_ACCESS_KEY": "stubtestsecret",
    "AWS_REGION": "us-east-1",
}


def stub_env(stub, bucket):
    env = dict(STUB_ENV)
    env["AWS_S3_ENDPOINT_URL"] = stub.url
    env["AWS_S3_BUCKET_NAME"] = bucket
    return env


# --------------------------------------------------------------------------
# F37-01: wait_for_db / wait_for_migrations
# --------------------------------------------------------------------------


def ensure_migrations_table(conn):
    conn.execute(
        "CREATE TABLE IF NOT EXISTS django_migrations ("
        "id serial primary key, app varchar(255) NOT NULL, "
        "name varchar(255) NOT NULL, applied timestamptz NOT NULL DEFAULT now())"
    )


def reset_migrations(conn, rows):
    ensure_migrations_table(conn)
    conn.execute("DELETE FROM django_migrations")
    for app, name in rows:
        conn.execute(
            "INSERT INTO django_migrations (app, name) VALUES (%s, %s)", (app, name)
        )


def test_wait_for_db_live():
    proc = run_ops("wait_for_db")
    assert proc.returncode == 0
    assert proc.stdout == "Waiting for database...\nDatabase available!\n"
    assert proc.stderr == ""


def test_wait_for_db_down_db_still_succeeds_immediately():
    """A down database changes nothing: the one-shot check always succeeds
    (F37-01 BUGS — ``connections["default"]`` never raises), so the command
    exits 0 at once with both lines. (The suite would hang here if the
    command waited, so completing at all proves one-shot.)"""
    proc = run_ops(
        "wait_for_db",
        env_extra={"DATABASE_URL": "postgresql://127.0.0.1:9/pidash_806_dead"},
    )
    assert proc.returncode == 0
    assert proc.stdout == "Waiting for database...\nDatabase available!\n"
    assert proc.stderr == ""


def test_wait_for_migrations_complete():
    conn = db_connect()
    try:
        reset_migrations(conn, EXPECTED_LEAVES + [("db", "0001_initial")])
    finally:
        conn.close()
    proc = run_ops("wait_for_migrations")
    assert proc.returncode == 0
    assert proc.stdout == "No migrations Pending. Starting processes ...\n"
    assert proc.stderr == ""


def test_wait_for_migrations_pending_then_success():
    """Poll line while a leaf is missing; success once it lands (~10s: the
    production sleep is a faithful 10s, like Python's)."""
    conn = db_connect()
    try:
        reset_migrations(conn, EXPECTED_LEAVES[:-1])
    finally:
        conn.close()
    proc = subprocess.Popen(
        [pidash_bin(), "ops", "wait_for_migrations"],
        env=dict(os.environ),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    killer = threading.Timer(40, proc.kill)
    killer.start()
    try:
        first = proc.stdout.readline()
        assert first == "Waiting for database migrations to complete...\n"
        conn = db_connect()
        try:
            app, name = EXPECTED_LEAVES[-1]
            conn.execute(
                "INSERT INTO django_migrations (app, name) VALUES (%s, %s)", (app, name)
            )
        finally:
            conn.close()
        second = proc.stdout.readline()
        assert second == "No migrations Pending. Starting processes ...\n"
        assert proc.wait(timeout=30) == 0
        assert proc.stderr.read() == ""
    finally:
        killer.cancel()
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=10)


def test_wait_for_migrations_missing_table_polls():
    """No ``django_migrations`` yet (migrator not run): pending, like
    Django's loader, which plans every migration when the table is
    absent (verified live against manage.py)."""
    conn = db_connect()
    try:
        conn.execute("DROP TABLE IF EXISTS django_migrations")
    finally:
        conn.close()
    proc = subprocess.Popen(
        [pidash_bin(), "ops", "wait_for_migrations"],
        env=dict(os.environ),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    try:
        assert proc.stdout.readline() == "Waiting for database migrations to complete...\n"
        time.sleep(1)
        assert proc.poll() is None
    finally:
        proc.kill()
        proc.wait(timeout=10)


def test_wait_for_db_missing_database_url_exits_1():
    proc = run_ops("wait_for_db", env_del=["DATABASE_URL"])
    assert proc.returncode == 1
    assert proc.stdout == ""
    assert proc.stderr == "DATABASE_URL is not set\n"


def test_wait_for_migrations_down_db_exits_1_without_output():
    """A down database is fatal (Python's uncaught ``OperationalError``):
    exit 1 with empty stdout. The stderr text is a one-line summary, not
    libpq's message, so it is asserted loosely (one non-empty line)."""
    proc = run_ops(
        "wait_for_migrations",
        env_extra={"DATABASE_URL": "postgresql://127.0.0.1:9/pidash_806_dead"},
    )
    assert proc.returncode == 1
    assert proc.stdout == ""
    assert proc.stderr != "" and proc.stderr.endswith("\n")
    assert len(proc.stderr.strip().splitlines()) == 1


def test_migrations_leaf_crosscheck_from_repo_files():
    """Independently re-derive the five first-party leaves from the
    checked-out migration files (stdlib ``ast`` only — no Django): the
    binary's baked-in list must match the files, so this test is not
    circular with it. Any future ``run_before``/``replaces`` marker fails
    loudly here, forcing a re-derivation."""
    migrations_root = REPO_ROOT / "apps" / "api"
    assert migrations_root.is_dir(), f"apps/api not found under {REPO_ROOT}"
    nodes = set()
    dependents = set()
    for app_dir in sorted((migrations_root / "pi_dash").iterdir()):
        mig_dir = app_dir / "migrations"
        if not mig_dir.is_dir():
            continue
        app = app_dir.name
        for path in sorted(mig_dir.glob("*.py")):
            if path.name == "__init__.py":
                continue
            tree = ast.parse(path.read_text(), filename=str(path))
            migration_class = next(
                node
                for node in ast.walk(tree)
                if isinstance(node, ast.ClassDef) and node.name == "Migration"
            )
            body = {stmt.targets[0].id: stmt.value for stmt in migration_class.body if isinstance(stmt, ast.Assign)}
            assert "run_before" not in body, f"{path}: run_before needs loader re-derivation"
            assert "replaces" not in body, f"{path}: replaces needs loader re-derivation"
            name = path.stem
            nodes.add((app, name))
            deps = body.get("dependencies", [])
            for dep in getattr(deps, "elts", []):
                if isinstance(dep, ast.Tuple) and len(dep.elts) == 2:
                    dep_app = ast.literal_eval(dep.elts[0])
                    dep_name = ast.literal_eval(dep.elts[1])
                    if dep_app == app or (migrations_root / "pi_dash" / dep_app).is_dir():
                        # An edge inside first-party apps (or to one):
                        # the dependency gains a dependent.
                        dependents.add((dep_app, dep_name))
                # `swappable_dependency(...)` calls are edges *to* auth —
                # they never make a first-party node a non-leaf.
    leaves = sorted(node for node in nodes if node not in dependents)
    assert leaves == sorted(REPO_LEAVES)


# --------------------------------------------------------------------------
# F37-02: clear_cache
# --------------------------------------------------------------------------


def test_clear_cache_key():
    client = redis_client()
    client.flushdb()
    client.set(":1:ckey", "v")
    client.set("other", "v")
    proc = run_ops("clear_cache", "--key", "ckey")
    assert proc.returncode == 0
    assert proc.stdout == "Cache Cleared for key: ckey\n"
    assert proc.stderr == ""
    assert client.get(":1:ckey") is None
    assert client.get("other") == "v"


def test_clear_cache_full():
    client = redis_client()
    client.flushdb()
    client.set(":1:a", "v")
    client.set("plain", "v")
    proc = run_ops("clear_cache")
    assert proc.returncode == 0
    assert proc.stdout == "Cache Cleared\n"
    assert proc.stderr == ""
    assert client.dbsize() == 0


def test_clear_cache_bare_key_flag_clears_all():
    """``--key`` without a value behaves like an absent one (argparse
    ``nargs='?'`` with no const yields ``None`` — falsy either way)."""
    client = redis_client()
    client.flushdb()
    client.set(":1:a", "v")
    proc = run_ops("clear_cache", "--key")
    assert proc.returncode == 0
    assert proc.stdout == "Cache Cleared\n"
    assert client.dbsize() == 0


def test_clear_cache_failure_dead_port():
    proc = run_ops("clear_cache", env_extra={"REDIS_URL": "redis://127.0.0.1:9/"})
    assert proc.returncode == 0
    assert proc.stdout == "Failed to clear cache\n"
    assert proc.stderr == ""


def test_clear_cache_failure_unset_url():
    proc = run_ops("clear_cache", env_del=["REDIS_URL"])
    assert proc.returncode == 0
    assert proc.stdout == "Failed to clear cache\n"
    assert proc.stderr == ""


# --------------------------------------------------------------------------
# F37-02: S3 — test-side client (setup/verify against the real backend)
# --------------------------------------------------------------------------


def s3_call(method, url, body=b"", extra_headers=None):
    """One signed S3 request with the suite's own signer (requests)."""
    headers = sign_s3(
        method,
        url,
        body,
        _require_env("AWS_ACCESS_KEY_ID"),
        _require_env("AWS_SECRET_ACCESS_KEY"),
        _require_env("AWS_REGION"),
    )
    headers.update(extra_headers or {})
    return requests.request(method, url, data=body, headers=headers, timeout=30)


def s3_endpoint():
    return _require_env("AWS_S3_ENDPOINT_URL").rstrip("/")


def s3_bucket_url(bucket, query=""):
    url = f"{s3_endpoint()}/{bucket}"
    return f"{url}?{query}" if query else url


def s3_setup_bucket(bucket, keys=()):
    """Create a bucket (idempotent) and put the given keys into it."""
    resp = s3_call("PUT", s3_bucket_url(bucket))
    assert resp.status_code in (200, 409), resp.text
    for key in keys:
        quoted = urllib.parse.quote(key, safe="/~")
        resp = s3_call("PUT", f"{s3_endpoint()}/{bucket}/{quoted}", b"x-" + key.encode())
        assert resp.status_code == 200, resp.text


def s3_list_keys(bucket):
    """List keys via the same query the binary sends, decoded exactly like
    botocore decodes them (the server echoes ``<EncodingType>url``)."""
    resp = s3_call("GET", s3_bucket_url(bucket, "list-type=2&encoding-type=url"))
    assert resp.status_code == 200, resp.text
    root = ET.fromstring(resp.content)
    ns = {"s3": "http://s3.amazonaws.com/doc/2006-03-01/"}
    keys = [
        urllib.parse.unquote(node.text or "")
        for node in root.findall("s3:Contents/s3:Key", ns) or root.findall("Contents/Key")
    ]
    if not keys:
        keys = [
            urllib.parse.unquote(node.text or "")
            for node in root.iter()
            if node.tag.endswith("Key") and node.text
        ]
    return keys


def s3_get_policy_text(bucket):
    resp = s3_call("GET", s3_bucket_url(bucket, "policy"))
    assert resp.status_code == 200, resp.text
    stripped = resp.content.strip()
    if stripped.startswith(b"{"):
        # LocalStack serves the stored document directly.
        return stripped.decode()
    root = ET.fromstring(resp.content)
    for node in root.iter():
        if node.tag.endswith("Policy") and node.text:
            return node.text
    raise AssertionError(f"no Policy in {resp.text[:200]}")


def s3_wipe_bucket(bucket):
    for key in s3_list_keys(bucket):
        quoted = urllib.parse.quote(key, safe="/~")
        s3_call("DELETE", f"{s3_endpoint()}/{bucket}/{quoted}")
    s3_call("DELETE", s3_bucket_url(bucket))


def fresh_bucket(prefix):
    return f"{prefix}-{uuid.uuid4().hex[:12]}"


# --------------------------------------------------------------------------
# F37-02: create_bucket
# --------------------------------------------------------------------------


def test_create_bucket_exists():
    bucket = fresh_bucket("opse")
    s3_setup_bucket(bucket)
    try:
        proc = run_ops("create_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket})
        assert proc.returncode == 0
        assert proc.stdout == f"Checking bucket...\nBucket '{bucket}' exists.\n"
        assert proc.stderr == ""
    finally:
        s3_wipe_bucket(bucket)


def test_create_bucket_404_then_create():
    bucket = fresh_bucket("opscreate")
    try:
        proc = run_ops("create_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket})
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Checking bucket...\n"
            f"Bucket '{bucket}' does not exist. Creating bucket...\n"
            f"Bucket '{bucket}' created successfully.\n"
        )
        assert proc.stderr == ""
        assert s3_call("HEAD", s3_bucket_url(bucket)).status_code == 200
    finally:
        s3_wipe_bucket(bucket)


def test_create_bucket_403_via_stub(stub_s3):
    """A 403 head (empty body, like MinIO/S3 send for HEAD) takes the
    forbidden branch. The stub verifies the signature first, so passing
    proves the Rust request signed correctly."""
    bucket = "forbidden-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (403, b"")
    proc = run_ops("create_bucket", env_extra=stub_env(stub_s3, bucket))
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Access to the bucket '{bucket}' is forbidden. Check permissions.\n"
    )
    assert proc.stderr == ""
    (logged,) = stub_s3.log
    assert logged["method"] == "HEAD"
    assert logged["path"] == f"/{bucket}"
    received = {k.lower(): v for k, v in logged["headers"].items()}
    assert received["authorization"].startswith("AWS4-HMAC-SHA256 ")
    assert "x-amz-content-sha256" in received


def test_create_bucket_else_branch_and_create_failure_via_stub(stub_s3):
    """Non-404/403 numeric codes take `Failed to check bucket`; a failed
    inner create takes `Failed to create bucket` — both with live
    `ClientError` text shaped exactly like botocore's."""
    bucket = "else-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (405, b"")
    proc = run_ops("create_bucket", env_extra=stub_env(stub_s3, bucket))
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        "Failed to check bucket: "
        "An error occurred (405) when calling the HeadBucket operation: Method Not Allowed\n"
    )
    assert proc.stderr == ""

    bucket2 = "createfail-bkt"
    stub_s3.routes[("HEAD", bucket2, "bucket")] = (404, b"")
    stub_s3.routes[("PUT", bucket2, "bucket")] = (
        409,
        _error_xml("BucketAlreadyExists", "The requested bucket name is not available."),
    )
    proc = run_ops("create_bucket", env_extra=stub_env(stub_s3, bucket2))
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        f"Bucket '{bucket2}' does not exist. Creating bucket...\n"
        "Failed to create bucket: An error occurred (BucketAlreadyExists) "
        "when calling the CreateBucket operation: The requested bucket name is not available.\n"
    )
    assert proc.stderr == ""


def test_create_bucket_dead_port_transport_message():
    bucket = "deadport-bkt"
    proc = run_ops(
        "create_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "http://127.0.0.1:9",
            "AWS_S3_BUCKET_NAME": bucket,
        },
    )
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        f'An error occurred: Could not connect to the endpoint URL: "http://127.0.0.1:9/{bucket}"\n'
    )
    assert proc.stderr == ""


def test_create_bucket_setup_errors():
    """Missing bucket/credentials and invalid endpoints — deterministic
    botocore texts. (An unset region is NOT an error: S3 defaults to
    ``us-east-1`` — see ``test_create_bucket_region_chain_via_stub``.)"""
    # No bucket: the TypeError arises at the head_bucket call, AFTER
    # `Checking bucket...` prints (create_bucket.py:30-32).
    proc = run_ops("create_bucket", env_del=["AWS_S3_BUCKET_NAME"])
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        "An error occurred: expected string or bytes-like object, got 'NoneType'\n"
    )
    assert proc.stderr == ""

    proc = run_ops(
        "create_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "http://127.0.0.1:9",
            "AWS_S3_BUCKET_NAME": "nocreds-bkt",
        },
        env_del=["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"],
    )
    assert proc.returncode == 0
    assert proc.stdout == "Checking bucket...\nAn error occurred: Unable to locate credentials\n"
    assert proc.stderr == ""

    # Client-build failures precede the `Checking bucket...` print
    # (create_bucket.py:20-30): an empty region without an endpoint ...
    proc = run_ops(
        "create_bucket",
        env_extra={
            "AWS_REGION": "",
            "AWS_S3_BUCKET_NAME": "buildfail-bkt",
        },
        env_del=["AWS_S3_ENDPOINT_URL", "AWS_DEFAULT_REGION"],
    )
    assert proc.returncode == 0
    assert proc.stdout == "An error occurred: Invalid endpoint: https://s3..amazonaws.com\n"
    assert proc.stderr == ""

    # ... and an explicitly empty endpoint (note botocore's trailing space).
    proc = run_ops(
        "create_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "",
            "AWS_S3_BUCKET_NAME": "buildfail-bkt",
        },
    )
    assert proc.returncode == 0
    assert proc.stdout == "An error occurred: Invalid endpoint: \n"
    assert proc.stderr == ""


def test_create_bucket_no_region_proceeds_with_default():
    """Unset region (both variables): S3 defaults to ``us-east-1`` and the
    command proceeds past setup — here into a transport error, via the
    outer ``except Exception`` arm (exit 0)."""
    bucket = "noregion-bkt"
    proc = run_ops(
        "create_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "http://127.0.0.1:9",
            "AWS_S3_BUCKET_NAME": bucket,
        },
        env_del=["AWS_REGION", "AWS_DEFAULT_REGION"],
    )
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        f'An error occurred: Could not connect to the endpoint URL: "http://127.0.0.1:9/{bucket}"\n'
    )
    assert proc.stderr == ""


def test_create_bucket_region_chain_via_stub(stub_s3):
    """The region chain, pinned end to end through the signed scope of a
    real request: ``AWS_DEFAULT_REGION`` applies when ``AWS_REGION`` is
    unset, else S3's ``us-east-1`` default."""
    bucket = "regionchain-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (403, b"")
    expected = (
        f"Checking bucket...\n"
        f"Access to the bucket '{bucket}' is forbidden. Check permissions.\n"
    )

    env = stub_env(stub_s3, bucket)
    del env["AWS_REGION"]
    env["AWS_DEFAULT_REGION"] = "eu-west-2"
    proc = run_ops(
        "create_bucket", env_extra=env, env_del=["AWS_REGION", "AWS_DEFAULT_REGION"]
    )
    assert proc.returncode == 0
    assert proc.stdout == expected
    assert proc.stderr == ""
    (logged,) = stub_s3.log
    received = {k.lower(): v for k, v in logged["headers"].items()}
    assert "/eu-west-2/s3/aws4_request" in received["authorization"]

    stub_s3.log.clear()
    env = stub_env(stub_s3, bucket)
    del env["AWS_REGION"]
    proc = run_ops(
        "create_bucket", env_extra=env, env_del=["AWS_REGION", "AWS_DEFAULT_REGION"]
    )
    assert proc.returncode == 0
    assert proc.stdout == expected
    assert proc.stderr == ""
    (logged,) = stub_s3.log
    received = {k.lower(): v for k, v in logged["headers"].items()}
    assert "/us-east-1/s3/aws4_request" in received["authorization"]


def test_create_bucket_head_body_ignored_like_botocore(stub_s3):
    """A HEAD error carrying an XML body (some servers send one) is
    ignored: like botocore, the client synthesizes the numeric code from
    the status — so the symbolic ``int()`` path stays unreachable over
    real HTTP in both stacks (probed live), and this 404 takes the create
    branch."""
    bucket = "headbody-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (
        404,
        _error_xml("NoSuchBucket", "The specified bucket does not exist"),
        True,
    )
    stub_s3.routes[("PUT", bucket, "bucket")] = (200, b"")
    proc = run_ops("create_bucket", env_extra=stub_env(stub_s3, bucket))
    assert proc.returncode == 0
    assert proc.stdout == (
        "Checking bucket...\n"
        f"Bucket '{bucket}' does not exist. Creating bucket...\n"
        f"Bucket '{bucket}' created successfully.\n"
    )
    assert proc.stderr == ""


# --------------------------------------------------------------------------
# F37-02: update_bucket
# --------------------------------------------------------------------------


def _expected_policy(bucket, keys):
    return {
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Principal": "*",
                "Action": "s3:GetObject",
                "Resource": [f"arn:aws:s3:::{bucket}/{key}" for key in keys],
            }
        ],
    }


def test_update_bucket_success(tmp_path):
    bucket = fresh_bucket("opsupd")
    # The exotic key succeeds in both stacks: LocalStack echoes
    # `<EncodingType>url</EncodingType>`, so botocore — and the binary —
    # decode it before the GetObject probe (live-diffed against manage.py).
    keys = ["a.txt", "odd key+&.txt"]
    s3_setup_bucket(bucket, keys)
    try:
        proc = run_ops(
            "update_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket}, cwd=tmp_path
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Checking bucket...\n"
            f"Bucket '{bucket}' exists.\n"
            "Access key has the required permissions.\n"
            "Bucket is private, but existing objects remain public.\n"
        )
        assert proc.stderr == ""
        # The probe's test object was cleaned up; the payload objects stay.
        assert sorted(s3_list_keys(bucket)) == sorted(keys)
        # The applied policy names each object — byte-identical to what
        # CPython's own json.dumps produces for the same document.
        listed = s3_list_keys(bucket)
        assert s3_get_policy_text(bucket) == json.dumps(_expected_policy(bucket, listed))
        assert not (tmp_path / "permissions.json").exists()
    finally:
        s3_wipe_bucket(bucket)


def test_update_bucket_empty_bucket_fallback(tmp_path):
    """An empty bucket skips the GetObject probe silently (no Contents),
    so the run falls back to writing permissions.json — while the policy
    probe still applies its public-read policy (ported side effect)."""
    bucket = fresh_bucket("opsempty")
    s3_setup_bucket(bucket)
    try:
        proc = run_ops(
            "update_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket}, cwd=tmp_path
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Checking bucket...\n"
            f"Bucket '{bucket}' exists.\n"
            "Generating permissions.json for manual bucket policy update.\n"
            "Permissions have been written to permissions.json.\n"
        )
        assert proc.stderr == ""
        assert (tmp_path / "permissions.json").read_bytes() == json.dumps(
            _expected_policy(bucket, [])
        ).encode()
        probe_policy = {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": "*",
                    "Action": "s3:GetObject",
                    "Resource": f"arn:aws:s3:::{bucket}/*",
                }
            ],
        }
        assert s3_get_policy_text(bucket) == json.dumps(probe_policy)
        assert s3_list_keys(bucket) == []
    finally:
        s3_wipe_bucket(bucket)


def test_update_bucket_missing_bucket(tmp_path):
    bucket = fresh_bucket("opsmissing")
    proc = run_ops("update_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket}, cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\nBucket '{bucket}' does not exist.\n"
    )
    assert proc.stderr == ""


def test_update_bucket_unset_bucket(tmp_path):
    proc = run_ops("update_bucket", env_del=["AWS_S3_BUCKET_NAME"], cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == "Please set the AWS_S3_BUCKET_NAME environment variable.\n"
    assert proc.stderr == ""


def test_update_bucket_empty_string_bucket(tmp_path):
    """``if not bucket_name`` (update_bucket.py:142): the empty string
    takes the unset line, like a missing variable (F37-02 "unset/empty")."""
    proc = run_ops(
        "update_bucket",
        env_extra={"AWS_S3_BUCKET_NAME": ""},
        cwd=tmp_path,
    )
    assert proc.returncode == 0
    assert proc.stdout == "Please set the AWS_S3_BUCKET_NAME environment variable.\n"
    assert proc.stderr == ""


def test_update_bucket_no_credentials_exits_1(tmp_path):
    """Without credentials the head call already fails (only `ClientError`
    is caught there), so the run ends after `Checking bucket...`."""
    proc = run_ops(
        "update_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "http://127.0.0.1:9",
            "AWS_S3_BUCKET_NAME": "updnocreds-bkt",
        },
        env_del=["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"],
        cwd=tmp_path,
    )
    assert proc.returncode == 1
    assert proc.stdout == "Checking bucket...\n"
    assert proc.stderr == "Unable to locate credentials\n"


def test_update_bucket_no_region_proceeds_past_setup(tmp_path):
    """Unset region (both variables): S3 defaults to ``us-east-1`` and the
    command proceeds past setup — here the head call fails, which escapes
    ``handle`` (traceback, exit 1 in Python; one stderr line here)."""
    bucket = "updnoregion-bkt"
    proc = run_ops(
        "update_bucket",
        env_extra={
            "AWS_S3_ENDPOINT_URL": "http://127.0.0.1:9",
            "AWS_S3_BUCKET_NAME": bucket,
        },
        env_del=["AWS_REGION", "AWS_DEFAULT_REGION"],
        cwd=tmp_path,
    )
    transport = f'Could not connect to the endpoint URL: "http://127.0.0.1:9/{bucket}"'
    assert proc.returncode == 1
    assert proc.stdout == "Checking bucket...\n"
    assert proc.stderr == f"{transport}\n"


def test_update_bucket_invalid_endpoint_exits_1(tmp_path):
    """``get_s3_client()`` at update_bucket.py:138 is outside any ``try``:
    the client-build ``ValueError`` escapes (traceback, exit 1 in Python;
    one stderr line and empty stdout here)."""
    proc = run_ops(
        "update_bucket",
        env_extra={
            "AWS_REGION": "",
            "AWS_S3_BUCKET_NAME": "buildfail-bkt",
        },
        env_del=["AWS_S3_ENDPOINT_URL", "AWS_DEFAULT_REGION"],
        cwd=tmp_path,
    )
    assert proc.returncode == 1
    assert proc.stdout == ""
    assert proc.stderr == "Invalid endpoint: https://s3..amazonaws.com\n"


def test_update_bucket_denied_probes_fallback_via_stub(stub_s3, tmp_path):
    """Every probe denied with `AccessDenied`: the denied lines,
    `Couldn't delete test object`, then the permissions.json fallback
    (the third list serves the fallback)."""
    bucket = "denied-bkt"
    denied = _error_xml("AccessDenied", "Access Denied")
    stub_s3.routes[("HEAD", bucket, "bucket")] = (200, b"")
    stub_s3.routes[("GET", bucket, "list")] = [(403, denied), (403, denied), (200, _empty_list_xml(bucket))]
    stub_s3.routes[("PUT", bucket, "object")] = (403, denied)
    stub_s3.routes[("DELETE", bucket, "object")] = (403, denied)
    stub_s3.routes[("PUT", bucket, "policy")] = (403, denied)
    proc = run_ops("update_bucket", env_extra=stub_env(stub_s3, bucket), cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Bucket '{bucket}' exists.\n"
        "ListBucket permission denied.\n"
        "GetObject permission denied.\n"
        "PutObject permission denied.\n"
        "Couldn't delete test object\n"
        "PutBucketPolicy permission denied.\n"
        "Generating permissions.json for manual bucket policy update.\n"
        "Permissions have been written to permissions.json.\n"
    )
    assert proc.stderr == ""
    assert (tmp_path / "permissions.json").read_bytes() == json.dumps(
        _expected_policy(bucket, [])
    ).encode()
    # Request shapes, botocore-identical: the list query, the 4-byte probe
    # body with auto-sent Content-MD5.
    methods = [(entry["method"], entry["query"]) for entry in stub_s3.log]
    assert ("GET", "list-type=2&encoding-type=url") in methods
    puts = [entry for entry in stub_s3.log if entry["method"] == "PUT" and not entry["query"].startswith("policy")]
    assert puts and puts[0]["body"] == b"Test"
    md5 = base64.b64encode(hashlib.md5(b"Test").digest()).decode()
    put_headers = {k.lower(): v for k, v in puts[0]["headers"].items()}
    assert put_headers.get("content-md5") == md5


def test_update_bucket_error_text_parity_via_stub(stub_s3, tmp_path):
    """Non-denied service errors interpolate live `ClientError` text shaped
    exactly like botocore's, built here from the same Code/Message the
    stub served."""
    bucket = "errtext-bkt"
    code, message = "InvalidAccessKeyId", "The AWS Access Key Id you provided does not exist in our records."
    failure = _error_xml(code, message)
    stub_s3.routes[("HEAD", bucket, "bucket")] = (200, b"")
    stub_s3.routes[("GET", bucket, "list")] = [(403, failure), (403, failure), (200, _empty_list_xml(bucket))]
    stub_s3.routes[("PUT", bucket, "object")] = (403, failure)
    stub_s3.routes[("DELETE", bucket, "object")] = (204, b"")
    stub_s3.routes[("PUT", bucket, "policy")] = (403, failure)

    def err(probe, operation):
        return f"Error in {probe}: An error occurred ({code}) when calling the {operation} operation: {message}\n"

    proc = run_ops("update_bucket", env_extra=stub_env(stub_s3, bucket), cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Bucket '{bucket}' exists.\n"
        + err("ListBucket", "ListObjectsV2")
        + err("GetObject", "ListObjectsV2")
        + err("PutObject", "PutObject")
        + err("PutBucketPolicy", "PutBucketPolicy")
        + "Generating permissions.json for manual bucket policy update.\n"
        + "Permissions have been written to permissions.json.\n"
    )
    assert proc.stderr == ""


def test_update_bucket_echoing_server_decodes_keys_via_stub(stub_s3, tmp_path):
    """Echoing list body (the compliant shape): the exotic key decodes, so
    the GetObject probe re-encodes it once — the logged GET path is
    single-encoded — and the run succeeds with decoded ARNs."""
    bucket = "echo-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (200, b"")
    stub_s3.routes[("GET", bucket, "list")] = [
        (200, _echo_list_xml(bucket, ["odd%20key%2B%26.txt"])),
        (200, _echo_list_xml(bucket, ["odd%20key%2B%26.txt"])),
        (200, _echo_list_xml(bucket, ["a.txt"])),
    ]
    stub_s3.routes[("GET", bucket, "object")] = (200, b"x")
    stub_s3.routes[("PUT", bucket, "object")] = (200, b"")
    stub_s3.routes[("DELETE", bucket, "object")] = (204, b"")
    stub_s3.routes[("PUT", bucket, "policy")] = (200, b"")
    proc = run_ops("update_bucket", env_extra=stub_env(stub_s3, bucket), cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Bucket '{bucket}' exists.\n"
        "Access key has the required permissions.\n"
        "Bucket is private, but existing objects remain public.\n"
    )
    assert proc.stderr == ""
    gets = [
        entry for entry in stub_s3.log
        if entry["method"] == "GET" and not entry["query"]
    ]
    assert [entry["path"] for entry in gets] == [f"/{bucket}/odd%20key%2B%26.txt"]
    policies = [entry for entry in stub_s3.log if entry["query"] == "policy"]
    assert f"arn:aws:s3:::{bucket}/a.txt" in policies[-1]["body"].decode()


def test_update_bucket_non_echoing_server_keeps_raw_keys_via_stub(stub_s3, tmp_path):
    """Non-echoing list body (no `<EncodingType>` element): botocore passes
    the keys through raw, so the GetObject probe double-encodes (`%25`) and
    the server answers NoSuchKey — the run prints the GetObject error line
    and falls back, writing permissions.json with the raw (encoded) ARNs
    (live-diffed against manage.py on the same stub shape)."""
    bucket = "noecho-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (200, b"")
    stub_s3.routes[("GET", bucket, "list")] = [
        (200, _raw_list_xml(bucket, ["odd%20key%2B%26.txt"])),
    ] * 3
    stub_s3.routes[("GET", bucket, "object")] = (
        404,
        _error_xml("NoSuchKey", "The specified key does not exist."),
    )
    stub_s3.routes[("PUT", bucket, "object")] = (200, b"")
    stub_s3.routes[("DELETE", bucket, "object")] = (204, b"")
    stub_s3.routes[("PUT", bucket, "policy")] = (200, b"")
    proc = run_ops("update_bucket", env_extra=stub_env(stub_s3, bucket), cwd=tmp_path)
    assert proc.returncode == 0
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Bucket '{bucket}' exists.\n"
        "Error in GetObject: An error occurred (NoSuchKey) "
        "when calling the GetObject operation: The specified key does not exist.\n"
        "Generating permissions.json for manual bucket policy update.\n"
        "Permissions have been written to permissions.json.\n"
    )
    assert proc.stderr == ""
    assert (tmp_path / "permissions.json").read_bytes() == json.dumps(
        _expected_policy(bucket, ["odd%20key%2B%26.txt"])
    ).encode()
    gets = [
        entry for entry in stub_s3.log
        if entry["method"] == "GET" and not entry["query"]
    ]
    assert [entry["path"] for entry in gets] == [f"/{bucket}/odd%2520key%252B%2526.txt"]


def test_update_bucket_dropped_list_takes_quirk_path_via_stub(stub_s3, tmp_path):
    """Head succeeds but the permission-check list hangs up: `Error:`,
    the unbound-`permissions` line (F37-02 quirk), `Generating...`, then
    the fallback list fails too and the run exits 1 — exactly Python's
    control flow."""
    bucket = "drop-bkt"
    stub_s3.routes[("HEAD", bucket, "bucket")] = (200, b"")
    stub_s3.routes[("GET", bucket, "list")] = [DROP, DROP]
    proc = run_ops("update_bucket", env_extra=stub_env(stub_s3, bucket), cwd=tmp_path)
    list_url = f"{stub_s3.url}/{bucket}?list-type=2&encoding-type=url"
    transport = f'Could not connect to the endpoint URL: "{list_url}"'
    assert proc.returncode == 1
    assert proc.stdout == (
        f"Checking bucket...\n"
        f"Bucket '{bucket}' exists.\n"
        f"Error: {transport}\n"
        f"Error: {UNBOUND_PERMISSIONS}\n"
        "Generating permissions.json for manual bucket policy update.\n"
    )
    assert proc.stderr == f"{transport}\n"
    assert not (tmp_path / "permissions.json").exists()


def test_update_bucket_unwritable_cwd(tmp_path):
    """The fallback write failing surfaces CPython's `IOError` text."""
    bucket = fresh_bucket("opsro")
    s3_setup_bucket(bucket)
    readonly = tmp_path / "ro"
    readonly.mkdir()
    os.chmod(readonly, 0o555)
    try:
        proc = run_ops(
            "update_bucket", env_extra={"AWS_S3_BUCKET_NAME": bucket}, cwd=readonly
        )
        assert proc.returncode == 0
        assert proc.stdout == (
            f"Checking bucket...\n"
            f"Bucket '{bucket}' exists.\n"
            "Generating permissions.json for manual bucket policy update.\n"
            "Error writing permissions.json: [Errno 13] Permission denied: 'permissions.json'\n"
        )
        assert proc.stderr == ""
    finally:
        os.chmod(readonly, 0o755)
        s3_wipe_bucket(bucket)


def test_unknown_option_exits_2_like_argparse():
    proc = run_ops("clear_cache", "--bogus")
    assert proc.returncode == 2
