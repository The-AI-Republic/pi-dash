# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Write-replay engine (PIDASHCONV-823).

Drives one corpus session: restores the snapshot into both databases,
replays the ordered requests against Django and Rust (all flags on),
and compares status + body, database state, enqueued jobs, and sink
deliveries after every request. The first divergence raises
:class:`Divergence` with the full evidence; a clean session returns a
:class:`SessionReport`.

Topology (booted by CI / the runbook, never here — the runner only
drives HTTP + SQL + broker drains against live infrastructure):

- Django-direct: ``REPLAY_DJANGO_URL`` serving ``REPLAY_DJANGO_DB``,
  broker ``CELERY_BROKER_URL`` (Redis), SMTP sink
  ``REPLAY_DJANGO_SMTP_PORT``.
- Django-upstream: serving ``REPLAY_RUST_DB`` (reached only through the
  Rust proxy), broker ``REPLAY_UPSTREAM_BROKER_URL`` (Redis), SMTP sink
  ``REPLAY_RUST_SMTP_PORT``.
- Rust: ``REPLAY_RUST_URL`` serving ``REPLAY_RUST_DB`` with every
  ``PIDASH_RUST_*`` flag on and ``PIDASH_DJANGO_UPSTREAM`` pointing at
  Django-upstream. Most handlers enqueue into ``rust_job_queue``; a few
  (forgot-password, space assets, ...) publish direct to the AMQP broker
  at ``REPLAY_RUST_AMQP_URL``. No workers and no beat anywhere:
  enqueued jobs accumulate observably instead of executing.

Sinks (``_harness.sinks.Worker*Sink``) run in the pytest process on the
fixed ports the servers were booted against.
"""

from __future__ import annotations

import os
import tempfile
from dataclasses import dataclass, field
from datetime import datetime, timezone

import httpx
import psycopg

from _harness import auth as auth_harness
from _harness import broker_probe
from _harness.sessions import ADMIN_SESSION_COOKIE_NAME

from . import compare, effects, session as session_model, snapshot


class Divergence(AssertionError):
    """One replayed request differed between backends (message = evidence)."""


@dataclass(frozen=True)
class ReplayConfig:
    django_url: str
    django_db: str
    rust_url: str
    rust_db: str
    upstream_broker_url: str
    secret_key: str
    snapshot_path: str
    django_webhook_base: str
    rust_webhook_base: str
    session_cookie_name: str = "session-id"
    work_dir: str = ""
    # Cache-Redis URLs of the Django-direct and Rust servers (their
    # REDIS_URL). The runner clears DRF throttle counters on both before
    # every session, so per-IP rate limits never leak between sessions.
    # Empty when the topology has no separate caches to reset.
    django_redis_url: str = ""
    rust_redis_url: str = ""
    # The Rust server's AMQP broker URL (its AMQP_URL). A few Rust
    # handlers publish direct to AMQP instead of rust_job_queue; the
    # runner drains this broker per request as Rust-side jobs. Empty
    # when the topology has no AMQP broker (those handlers 500 — see
    # README, this must be set).
    rust_amqp_url: str = ""
    # Comma-separated raw Fernet key list (ASSISTANT_ENCRYPTION_KEY):
    # decrypts assistant BYOK ciphertext columns declared with the
    # "assistant" schedule (compare.encrypted_columns). Empty skips.
    assistant_encryption_keys: str = ""

    @classmethod
    def from_env(cls, env=None) -> "ReplayConfig":
        env = env if env is not None else os.environ

        def need(name: str) -> str:
            value = env.get(name)
            if not value:
                raise RuntimeError(
                    f"write-replay needs {name} "
                    "(see rust-api/contract-tests/replay/README.md)"
                )
            return value

        work = env.get("REPLAY_WORK_DIR", "") or tempfile.mkdtemp(prefix="replay-")
        return cls(
            django_url=need("REPLAY_DJANGO_URL").rstrip("/"),
            django_db=need("REPLAY_DJANGO_DB"),
            rust_url=need("REPLAY_RUST_URL").rstrip("/"),
            rust_db=need("REPLAY_RUST_DB"),
            upstream_broker_url=need("REPLAY_UPSTREAM_BROKER_URL"),
            secret_key=need("SECRET_KEY"),
            snapshot_path=need("REPLAY_SNAPSHOT"),
            django_webhook_base=need("REPLAY_DJANGO_WEBHOOK_BASE").rstrip("/"),
            rust_webhook_base=need("REPLAY_RUST_WEBHOOK_BASE").rstrip("/"),
            session_cookie_name=env.get("SESSION_COOKIE_NAME", "session-id"),
            work_dir=work,
            django_redis_url=env.get("REPLAY_DJANGO_REDIS_URL", ""),
            rust_redis_url=env.get("REPLAY_RUST_REDIS_URL", ""),
            rust_amqp_url=env.get("REPLAY_RUST_AMQP_URL", ""),
            assistant_encryption_keys=env.get("ASSISTANT_ENCRYPTION_KEY", ""),
        )


@dataclass
class Sinks:
    django_smtp: object
    rust_smtp: object
    django_webhook: object
    rust_webhook: object


@dataclass
class RequestResult:
    index: int
    method: str
    route: str
    django_status: int
    rust_status: int
    tables_changed: list[str]
    jobs: int
    mail: int
    hooks: int


@dataclass
class SessionReport:
    session: str
    requests: list = field(default_factory=list)


def _clear_ephemeral_redis(redis_url: str) -> None:
    """Delete per-session-ephemeral keys on one cache Redis.

    DRF throttle counters (reused helper) and magic-link rows
    (``magic_*``, 10-minute TTL but sessions re-run faster than that):
    sessions restore their databases but never their Redis, so without
    this a re-run would observe the previous run's rate limits and
    attempt counters. Runs before the session's ``setup`` hook, which
    re-primes whatever the session needs.
    """
    import redis as redis_lib

    from _harness import redis as redis_helper

    client = redis_lib.Redis.from_url(redis_url, decode_responses=True)
    try:
        redis_helper.clear_throttle_counters(client)
        for key in client.scan_iter("magic_*"):
            client.delete(key)
    finally:
        client.close()


def _queue_max_id(database_url: str) -> int:
    with psycopg.connect(database_url, autocommit=True) as conn, conn.cursor() as cur:
        cur.execute("SELECT COALESCE(MAX(id), 0) FROM rust_job_queue")
        return cur.fetchone()[0]


def _queue_rows_since(database_url: str, last_id: int) -> list[dict]:
    from psycopg.rows import dict_row

    with psycopg.connect(database_url) as conn, conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT task, args, kwargs, queue, visible_at, attempts "
            "FROM rust_job_queue WHERE id > %s ORDER BY id",
            (last_id,),
        )
        return [dict(row) for row in cur.fetchall()]


class Runner:
    """Owns the live topology handles for a replay run."""

    def __init__(self, config: ReplayConfig, sinks: Sinks):
        self.config = config
        self.sinks = sinks
        self.world: dict | None = None
        self.extra_context: dict = {}
        self._cookie_names: dict[str, str] = {}
        self._cell_pins: list = []

    # -- session setup ---------------------------------------------------

    def restore_session(self, sess: session_model.Session) -> None:
        cfg = self.config
        snapshot.truncate_all(cfg.django_db)
        snapshot.load_data(cfg.django_db, cfg.snapshot_path)
        if sess.seed is not None:
            with psycopg.connect(cfg.django_db, autocommit=True) as conn:
                sess.seed(conn)
        clone_path = os.path.join(cfg.work_dir, f"clone-{sess.name}.sql")
        snapshot.clone_database(cfg.django_db, cfg.rust_db, clone_path)
        snapshot.ensure_queue_schema(cfg.rust_db)
        snapshot.rewrite_webhook_sinks(cfg.django_db, cfg.django_webhook_base)
        snapshot.rewrite_webhook_sinks(cfg.rust_db, cfg.rust_webhook_base)

    def forge_credentials(self, sess: session_model.Session) -> dict:
        """Forge session cookies in both DBs; return {(side, user): key}.

        ``admin:<user>`` entries forge the same sessions row but the
        client presents it as the admin-session-id cookie (the
        /api/instances/* console reads that name instead).
        """
        creds = {}
        users = self.world.get("users", {})
        for user_key in sess.auth_users:
            if user_key == "api_key":
                continue
            base_key = user_key.split(":", 1)[1] if user_key.startswith("admin:") else user_key
            user = users[base_key]
            for side, db_url in (("django", self.config.django_db), ("rust", self.config.rust_db)):
                with psycopg.connect(db_url, autocommit=True) as conn:
                    key = auth_harness.forge_session_key(
                        conn,
                        user_id=str(user["id"]),
                        password_field=user["password_field"],
                        secret=self.config.secret_key,
                    )
                creds[(side, user_key)] = key
        return creds

    def _client_for(
        self,
        side: str,
        auth: str,
        creds: dict,
        captures: dict,
        extra_cookies: dict | None = None,
    ) -> httpx.Client:
        base = self.config.django_url if side == "django" else self.config.rust_url
        headers: dict[str, str] = {}
        cookie_parts: list[str] = []
        if auth == "anonymous":
            pass
        elif auth == "api_key":
            headers["X-Api-Key"] = self.world["api_token"]
        elif auth.startswith("cookie:"):
            cap = auth.split(":", 1)[1]
            if cap not in captures:
                raise Divergence(f"auth {auth!r}: nothing captured as {cap!r} yet")
            value = captures[cap][0 if side == "django" else 1]
            cookie_name = self._cookie_names.get(cap, self.config.session_cookie_name)
            cookie_parts.append(f"{cookie_name}={value}")
        elif auth.startswith("admin:"):
            key = (side, auth)
            if key not in creds:
                raise Divergence(f"unknown auth {auth!r} (session users: {sorted({k[1] for k in creds})})")
            cookie_parts.append(f"{ADMIN_SESSION_COOKIE_NAME}={creds[key]}")
        else:
            key = (side, auth)
            if key not in creds:
                raise Divergence(f"unknown auth {auth!r} (session users: {sorted({k[1] for k in creds})})")
            cookie_parts.append(f"{self.config.session_cookie_name}={creds[key]}")
        for name, value in (extra_cookies or {}).items():
            cookie_parts.append(f"{name}={value}")
        if cookie_parts:
            headers["Cookie"] = "; ".join(cookie_parts)
        return httpx.Client(base_url=base, timeout=30.0, follow_redirects=False, headers=headers)

    # -- capture ----------------------------------------------------------

    def _extract_captures(self, req: session_model.Request, resp: httpx.Response, out: dict) -> None:
        body = None
        for name, pointer in (req.capture or {}).items():
            if pointer.startswith("@cookie:"):
                cookie_name = pointer.split(":", 1)[1]
                value = resp.cookies.get(cookie_name)
                if value is None:
                    raise Divergence(
                        f"{req.method} {req.path}: capture {name!r}: "
                        f"no cookie {cookie_name!r} in Set-Cookie {resp.headers.get('set-cookie')!r}"
                    )
                out[name] = value
                self._cookie_names[name] = cookie_name
            elif pointer.startswith("@header:"):
                header = pointer.split(":", 1)[1]
                if header not in resp.headers:
                    raise Divergence(
                        f"{req.method} {req.path}: capture {name!r}: "
                        f"no header {header!r}"
                    )
                out[name] = resp.headers[header]
            else:
                if body is None:
                    try:
                        body = resp.json()
                    except ValueError:
                        raise Divergence(
                            f"{req.method} {req.path}: capture {name!r}: "
                            "response body is not JSON"
                        ) from None
                try:
                    out[name] = session_model.extract_pointer(body, pointer)
                except KeyError as exc:
                    raise Divergence(f"{req.method} {req.path}: capture {name!r}: {exc}") from exc

    # -- main loop ---------------------------------------------------------

    def run_session(self, sess: session_model.Session) -> SessionReport:
        from .corpus import world as world_seed

        self._cookie_names: dict[str, str] = {}
        self.restore_session(sess)
        # Ids come from the restored snapshot (identical in both copies),
        # so the address book is looked up fresh after every restore.
        self.world = world_seed.load_world(self.config.django_db)
        creds = self.forge_credentials(sess)
        # Drain broker leftovers so per-request attribution starts clean.
        broker_probe.drain_queue("celery", limit=10000)
        broker_probe.drain_queue("celery", limit=10000, broker_url=self.config.upstream_broker_url)
        if self.config.rust_amqp_url:
            broker_probe.drain_queue(
                "celery", limit=10000, broker_url=self.config.rust_amqp_url
            )
        # Throttle counters and magic rows live in the cache Redises
        # (never restored), so rate limits and attempt counters would
        # otherwise leak between sessions.
        for redis_url in (self.config.django_redis_url, self.config.rust_redis_url):
            if redis_url:
                _clear_ephemeral_redis(redis_url)
        # Active cell pins: session-wide pins plus every per-request
        # pin introduced so far (cell pins accumulate — divergent
        # state persists, so its pin must too).
        self._cell_pins = list(sess.known_cell_diffs)
        # Per-run context (read back seeded ids, mint tokens, prime Redis).
        self.extra_context = dict(sess.setup(self.world, self.config)) if sess.setup else {}
        missing = [k for k in (sess.setup_keys or ()) if k not in self.extra_context]
        if missing:
            raise Divergence(
                f"session {sess.name}: setup() omitted declared keys {missing}"
            )
        for sink in (
            self.sinks.django_smtp,
            self.sinks.rust_smtp,
            self.sinks.django_webhook,
            self.sinks.rust_webhook,
        ):
            sink.clear()

        report = SessionReport(session=sess.name)
        captures: dict[str, tuple] = {}
        config = self.config

        def _row_exists(side: str, table: str, value) -> bool:
            db_url = config.django_db if side == "django" else config.rust_db
            try:
                with psycopg.connect(db_url, autocommit=True) as conn, conn.cursor() as cur:
                    cur.execute(f'SELECT 1 FROM "{table}" WHERE id = %s', (str(value),))
                    return cur.fetchone() is not None
            except Exception:
                return False

        def _db_row(side: str, table: str, value):
            from psycopg.rows import dict_row

            db_url = config.django_db if side == "django" else config.rust_db
            with psycopg.connect(db_url, autocommit=True) as conn:
                with conn.cursor(row_factory=dict_row) as cur:
                    cur.execute(f'SELECT * FROM "{table}" WHERE id = %s', (str(value),))
                    row = cur.fetchone()
                    return dict(row) if row else None

        def _db_value_exists(side: str, table: str, column: str, value) -> bool:
            import re

            if not re.fullmatch(r"[a-z_]+", table) or not re.fullmatch(r"[a-z_]+", column):
                return False
            db_url = config.django_db if side == "django" else config.rust_db
            try:
                with psycopg.connect(db_url, autocommit=True) as conn, conn.cursor() as cur:
                    cur.execute(
                        f'SELECT 1 FROM "{table}" WHERE "{column}" = %s',
                        (str(value),),
                    )
                    return cur.fetchone() is not None
            except Exception:
                return False

        def _redis_for(side: str):
            import redis as redis_lib

            url = config.django_redis_url if side == "django" else config.rust_redis_url
            if not url:
                return None
            return redis_lib.Redis.from_url(url, decode_responses=True)

        ctx = compare.CompareContext(
            timestamp_columns=dict(sess.timestamp_columns),
            generated_columns=dict(sess.generated_columns),
            skip_tables=dict(sess.skip_tables),
            django_webhook_base=self.config.django_webhook_base,
            rust_webhook_base=self.config.rust_webhook_base,
            encrypted_columns=dict(sess.encrypted_columns),
            assistant_encryption_keys=self.config.assistant_encryption_keys,
            payload_time_paths=dict(sess.payload_time_paths),
            payload_row_refs=dict(sess.payload_row_refs),
            payload_value_diffs=dict(sess.payload_value_diffs),
            payload_db_values=dict(sess.payload_db_values),
            db_value_exists=_db_value_exists,
            db_cell_refs=list(sess.db_cell_refs),
            row_exists=_row_exists,
            payload_reset_tokens=dict(sess.payload_reset_tokens),
            db_row=_db_row,
            secret_key=self.config.secret_key,
            payload_magic_codes=dict(sess.payload_magic_codes),
            redis_for=_redis_for,
            cell_diffs=list(sess.known_cell_diffs),
        )
        # Restore sanity: both copies compare equal before request 1, or
        # the restore/clone itself is broken (never a product divergence).
        dj_state = compare.normalise_snapshot(compare.snapshot_full(self.config.django_db), "django", ctx)
        rs_state = compare.normalise_snapshot(compare.snapshot_full(self.config.rust_db), "rust", ctx)
        initial = compare.compare_db(dj_state, rs_state)
        if initial:
            tables = sorted(t for t, _, _ in initial)
            raise Divergence(
                f"session {sess.name}: copies differ after restore (restore bug, "
                f"not a product divergence). Tables: {tables}"
            )

        gap_tracker: dict[tuple, int] = {}
        for index, req in enumerate(sess.requests):
            result = self._run_request(sess, req, index, captures, creds, ctx, gap_tracker)
            report.requests.append(result)
        for side, declarations in (
            ("dj", sess.known_extra_rows or []),
            ("rs", sess.known_rust_extra_rows or []),
        ):
            for decl_index, (table, issue, _match) in enumerate(declarations):
                if not gap_tracker.get((side, decl_index)):
                    raise Divergence(
                        f"session {sess.name}: known-divergence pin for {table} "
                        f"({issue}) never matched — the fix landed or the shape "
                        "changed; remove or update the pin"
                    )
        return report

    def _gap_ctx(self, sess, captures) -> dict:
        """Render context for pin match dicts: world/static raw, captures {name}."""
        from .corpus import world as world_seed

        gap_ctx = world_seed.template_context(self.world)
        gap_ctx.update(sess.static_context or {})
        gap_ctx.update(self.extra_context or {})
        for name in captures:
            gap_ctx[name] = "{%s}" % name
        return gap_ctx

    def _apply_db_gap_pins(self, db_diffs, sess, captures, gap_tracker):
        """Excuse declared one-sided rows; return the remaining diffs."""
        if not sess.known_extra_rows and not sess.known_rust_extra_rows:
            return db_diffs
        # Gap match dicts are written against NORMALISED rows: captures
        # already substituted ({name}), world/static values raw.
        gap_ctx = self._gap_ctx(sess, captures)
        remaining = []
        for table, only_dj, only_rs in db_diffs:
            if only_dj and not only_rs:
                decls = [
                    (("dj", i), match)
                    for i, (t, _issue, match) in enumerate(sess.known_extra_rows or [])
                    if t == table
                ]
                one_sided = only_dj
            elif only_rs and not only_dj:
                decls = [
                    (("rs", i), match)
                    for i, (t, _issue, match) in enumerate(sess.known_rust_extra_rows or [])
                    if t == table
                ]
                one_sided = only_rs
            else:
                # Both sides differ: pins excuse pure one-sided gaps
                # only, never a table both backends changed differently.
                remaining.append((table, only_dj, only_rs))
                continue
            if not decls:
                remaining.append((table, only_dj, only_rs))
                continue
            unmatched = []
            for row in one_sided:
                hit = None
                for decl_index, match in decls:
                    try:
                        rendered = session_model.render_template(dict(match), gap_ctx)
                    except KeyError:
                        continue
                    if row == rendered:
                        hit = decl_index
                        break
                if hit is None:
                    unmatched.append(row)
                else:
                    gap_tracker[hit] = gap_tracker.get(hit, 0) + 1
            if unmatched:
                if one_sided is only_dj:
                    remaining.append((table, unmatched, []))
                else:
                    remaining.append((table, [], unmatched))
        return remaining

    def _run_request(self, sess, req, index, captures, creds, ctx, gap_tracker) -> RequestResult:
        ctx.captures = dict(captures)
        # Per-request payload pins join the session pins for this
        # request only (rebuilt every request, so nothing leaks).
        merged_value_diffs = {
            task: list(refs) for task, refs in (sess.payload_value_diffs or {}).items()
        }
        for task, refs in (req.payload_value_diffs or {}).items():
            merged_value_diffs.setdefault(task, []).extend(refs)
        ctx.payload_value_diffs = merged_value_diffs
        # This request's response blank rules ride along for the
        # process_logs echo; pin values render against the same
        # world/static/capture context as the gap pins.
        ctx.body_blank_paths = list(req.body_blank_paths or ())
        ctx.body_regex_blanks = list(req.body_regex_blanks or ())
        ctx.echo_free_pass = bool(req.skip_body)
        ctx.pin_ctx = self._gap_ctx(sess, captures)
        from .corpus import world as world_seed

        base_ctx = world_seed.template_context(self.world)
        base_ctx.update(sess.static_context or {})
        base_ctx.update(self.extra_context or {})

        def _side_ctx(index: int) -> dict:
            side_ctx = dict(base_ctx)
            side_ctx.update({name: pair[index] for name, pair in captures.items()})
            return side_ctx

        bodies = sum([req.json is not None, req.raw_body is not None, bool(req.form), bool(req.files)])
        if bodies > 1:
            raise Divergence(
                f"session {sess.name} request {index} {req.method} {req.path}: "
                "corpus bug — set only one of json / raw_body / form / files"
            )
        dj_ctx = _side_ctx(0)
        rs_ctx = _side_ctx(1)
        dj_path = session_model.render_template(req.path, dj_ctx)
        rs_path = session_model.render_template(req.path, rs_ctx)
        dj_json = session_model.render_template(req.json, dj_ctx)
        rs_json = session_model.render_template(req.json, rs_ctx)
        dj_form = session_model.render_template(dict(req.form or {}), dj_ctx) or None
        rs_form = session_model.render_template(dict(req.form or {}), rs_ctx) or None
        dj_files = session_model.render_template(dict(req.files or {}), dj_ctx) or None
        rs_files = session_model.render_template(dict(req.files or {}), rs_ctx) or None
        dj_params = session_model.render_template(dict(req.params or {}), dj_ctx)
        rs_params = session_model.render_template(dict(req.params or {}), rs_ctx)
        dj_headers = session_model.render_template(dict(req.headers or {}), dj_ctx)
        rs_headers = session_model.render_template(dict(req.headers or {}), rs_ctx)
        dj_cookies = session_model.render_template(dict(req.cookies or {}), dj_ctx)
        rs_cookies = session_model.render_template(dict(req.cookies or {}), rs_ctx)

        dj_before = compare.snapshot_full(self.config.django_db)
        rs_before = compare.snapshot_full(self.config.rust_db)
        queue_before = _queue_max_id(self.config.rust_db)
        mail_before = (len(self.sinks.django_smtp.snapshot()), len(self.sinks.rust_smtp.snapshot()))
        hooks_before = (len(self.sinks.django_webhook.snapshot()), len(self.sinks.rust_webhook.snapshot()))

        if req.delay_before:
            import time as _time

            _time.sleep(req.delay_before)
        started = datetime.now(timezone.utc)
        with self._client_for("django", req.auth, creds, captures, dj_cookies) as client:
            dj_resp = client.request(
                req.method, dj_path, json=dj_json, content=req.raw_body,
                data=dj_form, files=dj_files, params=dj_params, headers=dj_headers,
            )
        dj_msgs = broker_probe.drain_queue("celery", limit=10000)
        with self._client_for("rust", req.auth, creds, captures, rs_cookies) as client:
            rs_resp = client.request(
                req.method, rs_path, json=rs_json, content=req.raw_body,
                data=rs_form, files=rs_files, params=rs_params, headers=rs_headers,
            )
        rs_queue_rows = _queue_rows_since(self.config.rust_db, queue_before)
        rs_msgs = broker_probe.drain_queue(
            "celery", limit=10000, broker_url=self.config.upstream_broker_url
        )
        if self.config.rust_amqp_url:
            rs_msgs = rs_msgs + broker_probe.drain_queue(
                "celery", limit=10000, broker_url=self.config.rust_amqp_url
            )
        ctx.request_start = started

        if req.expect_status is not None and dj_resp.status_code != req.expect_status:
            raise Divergence(
                f"session {sess.name} request {index} {req.method} {req.path}: "
                f"django answered {dj_resp.status_code}, session expects "
                f"{req.expect_status} (corpus bug, not a product divergence): "
                f"{dj_resp.text[:2000]}"
            )

        label = f"session {sess.name} request {index} {req.method} {req.route_template()}"
        ctx.capture_pointers = dict(req.capture or {})
        error = compare.compare_response(
            dj_resp,
            rs_resp,
            ctx,
            req.known_headers,
            self._gap_ctx(sess, captures),
            req.known_body_diffs,
            req.body_blank_paths,
            req.body_regex_blanks,
            req.skip_body,
            req.body_contains,
            req.known_body_aliases,
        )
        if error is not None:
            raise Divergence(f"{label}: {error}")

        # Captures for later requests AND for this request's DB/job/sink
        # comparison (created ids only exist from this response on).
        dj_cap: dict = {}
        rs_cap: dict = {}
        self._extract_captures(req, dj_resp, dj_cap)
        self._extract_captures(req, rs_resp, rs_cap)
        for name in dj_cap:
            if name not in rs_cap:
                raise Divergence(f"{label}: captured {name!r} on django but not rust")
            captures[name] = (dj_cap[name], rs_cap[name])
        ctx.captures = dict(captures)
        ctx.capture_pointers = {}
        if req.known_cell_diffs:
            self._cell_pins.extend(req.known_cell_diffs)
        if self._cell_pins:
            # Cell pins are templates over the gap ctx: world/static
            # values render raw (seed-random ids compare literally),
            # captures render as {name} (their normalised form). A pin
            # naming a capture not seen yet cannot match this request.
            gap_ctx = self._gap_ctx(sess, captures)
            rendered_pins = []
            for pin in self._cell_pins:
                table, issue, match, column, dj_value, rs_value = pin
                try:
                    rendered_pins.append((
                        table, issue,
                        session_model.render_template(dict(match), gap_ctx),
                        column,
                        session_model.render_template(dj_value, gap_ctx),
                        session_model.render_template(rs_value, gap_ctx),
                    ))
                except KeyError:
                    continue
            ctx.cell_diffs = rendered_pins
        dj_ctx = _side_ctx(0)

        dj_after = compare.snapshot_full(self.config.django_db)
        rs_after = compare.snapshot_full(self.config.rust_db)
        dj_norm_before = compare.normalise_snapshot(dj_before, "django", ctx)
        rs_norm_before = compare.normalise_snapshot(rs_before, "rust", ctx)
        dj_norm_after = compare.normalise_snapshot(dj_after, "django", ctx)
        rs_norm_after = compare.normalise_snapshot(rs_after, "rust", ctx)
        db_diffs = compare.compare_db(dj_norm_after, rs_norm_after)
        db_diffs = self._apply_db_gap_pins(db_diffs, sess, captures, gap_tracker)
        if db_diffs:
            detail = []
            for table, only_dj, only_rs in db_diffs[:5]:
                detail.append(
                    f"table {table}: django-only={only_dj[:3]!r} rust-only={only_rs[:3]!r}"
                )
            raise Divergence(f"{label}: database state differs:\n" + "\n".join(detail))

        dj_jobs = [
            compare.normalise_celery_message(headers, payload, "django", ctx)
            for headers, payload in dj_msgs
        ]
        rs_jobs = [compare.normalise_queue_row(row, ctx) for row in rs_queue_rows] + [
            compare.normalise_celery_message(headers, payload, "rust", ctx)
            for headers, payload in rs_msgs
        ]
        absent = dict(sess.known_absent_jobs or {})
        absent.update(req.known_absent_jobs or {})
        error = compare.compare_jobs(
            dj_jobs,
            rs_jobs,
            absent,
            req.known_job_shapes,
            self._gap_ctx(sess, captures),
        )
        if error is not None:
            raise Divergence(f"{label}: {error}")

        dj_mail = self.sinks.django_smtp.snapshot()[mail_before[0]:]
        rs_mail = self.sinks.rust_smtp.snapshot()[mail_before[1]:]
        dj_hooks = self.sinks.django_webhook.snapshot()[hooks_before[0]:]
        rs_hooks = self.sinks.rust_webhook.snapshot()[hooks_before[1]:]
        error = compare.compare_sinks(dj_mail, rs_mail, dj_hooks, rs_hooks, ctx)
        if error is not None:
            raise Divergence(f"{label}: {error}")

        changed = compare.diff_states(dj_norm_before, dj_norm_after)
        return RequestResult(
            index=index,
            method=req.method,
            route=req.route_template(),
            django_status=dj_resp.status_code,
            rust_status=rs_resp.status_code,
            tables_changed=sorted(changed),
            jobs=len(dj_jobs),
            mail=len(dj_mail),
            hooks=len(dj_hooks),
        )
