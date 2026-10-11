# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Replay static guards (PIDASHCONV-823).

Needs no live backends: pins the replay's copies of Rust-owned strings
(queue DDL, shadow volatile allowlist) against the Rust source, and
validates the corpus manifests (placeholders, captures, auth, reasons
shape). Runs in every environment, including bare ``pytest``.
"""

import re
import string
from pathlib import Path

from . import effects
from . import snapshot as snapshot_mod
from .corpus import ALL_SESSIONS
from .corpus import reasons
from .corpus import world as world_seed

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
QUEUE_RS = REPO_ROOT / "crates" / "jobs" / "src" / "queue.rs"
SCHEDULE_RS = REPO_ROOT / "crates" / "jobs" / "src" / "schedule.rs"
SHADOW_RS = REPO_ROOT / "crates" / "api" / "src" / "edge_shadow.rs"

FAKE_WORLD = {
    "users": {key: {"id": "u"} for key in (*world_seed.USER_KEYS,)},
    "workspaces": {"ws": {"slug": "w", "id": "w"}, "other": {"slug": "o", "id": "o"}},
    "projects": {"main": {"id": "p"}, "other": {"id": "q"}},
    "states": {"backlog": {"id": "s"}, "completed": {"id": "s"}, "cancelled": {"id": "s"}},
}
WORLD_KEYS = set(world_seed.template_context(FAKE_WORLD))

HTTP_METHODS = {"GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"}


def _rust_string_literal(source: str, fn_name: str) -> str:
    """Extract a ``&'static str`` return literal, applying ``\\`` continuations."""
    match = re.search(
        r"fn " + re.escape(fn_name) + r"\(\) -> &'static str \{\s*\"(?P<body>(?:[^\"\\]|\\.)*)\"",
        source,
        re.DOTALL,
    )
    assert match, f"{fn_name} literal not found"
    body = match.group("body")
    # Rust `\` + newline + leading whitespace collapses to nothing.
    return re.sub(r"\\\n\s*", "", body)


def test_queue_ddl_matches_rust_source():
    queue_src = QUEUE_RS.read_text()
    assert _rust_string_literal(queue_src, "ensure_schema_sql") == snapshot_mod.QUEUE_TABLE_DDL
    assert (
        _rust_string_literal(queue_src, "ensure_claim_index_sql")
        == snapshot_mod.QUEUE_INDEX_DDL
    )
    schedule_src = SCHEDULE_RS.read_text()
    assert (
        _rust_string_literal(schedule_src, "ensure_schedule_schema_sql")
        == snapshot_mod.SCHEDULE_TABLE_DDL
    )


def _rust_str_list(source: str, const_name: str) -> set[str]:
    match = re.search(
        r"pub const " + re.escape(const_name) + r": &\[[^\]]*\] = &\[(?P<body>[^\]]*)\]",
        source,
        re.DOTALL,
    )
    assert match, f"{const_name} not found"
    return set(re.findall(r'"([^"]*)"', match.group("body")))


def test_volatile_mirrors_match_shadow():
    shadow_src = SHADOW_RS.read_text()
    assert _rust_str_list(shadow_src, "VOLATILE_JSON_KEYS") == set(effects.VOLATILE_JSON_KEYS)
    assert _rust_str_list(shadow_src, "VOLATILE_QUERY_PARAMS") == set(
        effects.VOLATILE_QUERY_PARAMS
    )


def _placeholders(node) -> set[str]:
    found = set()

    def walk(value):
        if isinstance(value, str):
            for _, field_name, _, _ in string.Formatter().parse(value):
                if field_name:
                    found.add(field_name.split(".")[0].split("[")[0])
        elif isinstance(value, dict):
            for key, item in value.items():
                walk(key)
                walk(item)
        elif isinstance(value, (list, tuple)):
            for item in value:
                walk(item)

    walk(node)
    return found


def test_corpus_manifests_valid():
    names = [s.name for s in ALL_SESSIONS]
    assert len(names) == len(set(names)), "duplicate session names"
    assert names, "corpus is empty"
    for sess in ALL_SESSIONS:
        assert sess.domain, f"{sess.name}: no domain"
        assert sess.source, f"{sess.name}: no source provenance"
        assert sess.requests, f"{sess.name}: no requests"
        for user in sess.auth_users:
            base = user.split(":", 1)[1] if user.startswith("admin:") else user
            assert base in (*world_seed.USER_KEYS, "api_key"), (
                f"{sess.name}: unknown auth user {user!r}"
            )
        if sess.seed is not None:
            assert callable(sess.seed), f"{sess.name}: seed is not callable"
        if sess.setup is not None:
            assert callable(sess.setup), f"{sess.name}: setup is not callable"
            assert sess.setup_keys, (
                f"{sess.name}: setup without setup_keys declares nothing"
            )
        for task, issue in (sess.known_absent_jobs or {}).items():
            assert "." in task, f"{sess.name}: bad pinned task {task!r}"
            assert re.fullmatch(r"PIDASHCONV-\d+", issue), (
                f"{sess.name}: pinned {task} must name its fix issue"
            )
        for entry in sess.db_cell_refs or []:
            assert (
                isinstance(entry, (list, tuple))
                and len(entry) == 4
                and re.fullmatch(r"[a-z_]+", entry[0])
                and re.fullmatch(r"[a-z_]+", entry[1])
                and re.fullmatch(r"[a-z_]+", entry[2])
                and isinstance(entry[3], str)
            ), f"{sess.name}: bad db cell ref {entry!r}"
        for field_name in ("known_extra_rows", "known_rust_extra_rows"):
            for entry in getattr(sess, field_name) or []:
                assert (
                    isinstance(entry, (list, tuple))
                    and len(entry) == 3
                    and re.fullmatch(r"[a-z_]+", entry[0])
                    and re.fullmatch(r"PIDASHCONV-\d+", entry[1])
                    and isinstance(entry[2], dict)
                    and entry[2]
                ), f"{sess.name}: bad {field_name} {entry!r}"
        for entry in list(sess.known_cell_diffs or []) + [
            pin for req in sess.requests for pin in (req.known_cell_diffs or [])
        ]:
            assert (
                isinstance(entry, (list, tuple))
                and len(entry) == 6
                and re.fullmatch(r"[a-z_]+", entry[0])
                and re.fullmatch(r"PIDASHCONV-\d+", entry[1])
                and isinstance(entry[2], dict)
                and entry[2]
                and re.fullmatch(r"[a-z_]+", entry[3])
                and entry[3] not in entry[2]
                and entry[4] != entry[5]
            ), f"{sess.name}: bad known cell diff {entry!r}"
        for task, refs in (sess.payload_row_refs or {}).items():
            assert isinstance(refs, (list, tuple)), f"{sess.name}: bad row refs for {task}"
            for ref in refs:
                assert (
                    isinstance(ref, (list, tuple))
                    and len(ref) == 2
                    and (ref[0].startswith("args/") or ref[0].startswith("kwargs/"))
                    and re.fullmatch(r"[a-z_]+", ref[1])
                ), f"{sess.name}: bad row ref {ref!r} for {task}"
        value_diff_maps = [("session", sess.payload_value_diffs or {})]
        for index, req in enumerate(sess.requests):
            if req.payload_value_diffs:
                value_diff_maps.append((f"request {index}", req.payload_value_diffs))
        for owner, diff_map in value_diff_maps:
            for task, refs in diff_map.items():
                assert isinstance(refs, (list, tuple)), (
                    f"{sess.name}: bad value diffs for {task} ({owner})"
                )
                for ref in refs:
                    assert (
                        isinstance(ref, (list, tuple))
                        and len(ref) == 4
                        and (ref[0].startswith("args/") or ref[0].startswith("kwargs/"))
                        and re.fullmatch(r"PIDASHCONV-\d+", ref[1])
                        and ref[2] != ref[3]
                    ), f"{sess.name}: bad value diff {ref!r} for {task} ({owner})"
        for field_name in ("payload_reset_tokens", "payload_magic_codes"):
            for task, refs in (getattr(sess, field_name) or {}).items():
                assert isinstance(refs, (list, tuple)), (
                    f"{sess.name}: bad {field_name} for {task}"
                )
                for ref in refs:
                    assert (
                        isinstance(ref, (list, tuple))
                        and len(ref) == 2
                        and (ref[0].startswith("args/") or ref[0].startswith("kwargs/"))
                        and (ref[1].startswith("args/") or ref[1].startswith("kwargs/"))
                    ), f"{sess.name}: bad {field_name} {ref!r} for {task}"
        available = (
            set(WORLD_KEYS) | set(sess.static_context or {}) | set(sess.setup_keys or ())
        )
        for index, req in enumerate(sess.requests):
            label = f"{sess.name} request {index} {req.method} {req.path}"
            assert req.method in HTTP_METHODS, f"{label}: bad method"
            assert req.path.startswith("/"), f"{label}: path must be absolute"
            assert req.route_template().startswith("/"), f"{label}: bad route"
            for placeholder in _placeholders(
                [req.path, req.params, req.headers, req.json, req.form,
                 req.cookies, req.files]
            ):
                assert placeholder in available, (
                    f"{label}: {{{placeholder}}} is neither a world key "
                    f"({sorted(WORLD_KEYS)}) nor captured earlier "
                    f"({sorted(available - WORLD_KEYS)})"
                )
            if req.auth not in (*sess.auth_users, "anonymous", "api_key"):
                assert req.auth.startswith("cookie:"), f"{label}: bad auth {req.auth!r}"
                cap = req.auth.split(":", 1)[1]
                assert cap in available - WORLD_KEYS, (
                    f"{label}: auth cookie capture {cap!r} not captured earlier"
                )
            for name, pointer in (req.capture or {}).items():
                assert name not in available, f"{label}: capture {name!r} reused"
                assert pointer, f"{label}: empty pointer for {name!r}"
                if pointer.startswith("@cookie:") or pointer.startswith("@header:"):
                    assert len(pointer.split(":", 1)[1]) > 0, f"{label}: bad {pointer!r}"
                else:
                    assert all(part for part in pointer.split("/")), (
                        f"{label}: bad pointer {pointer!r}"
                    )
                available.add(name)
            if req.expect_status is not None:
                assert 100 <= req.expect_status <= 599, f"{label}: bad expect_status"
            for entry in req.known_job_shapes or []:
                assert (
                    isinstance(entry, (list, tuple))
                    and len(entry) == 4
                    and "." in entry[0]
                    and re.fullmatch(r"PIDASHCONV-\d+", entry[1])
                    and isinstance(entry[2], dict)
                    and entry[2]
                    and isinstance(entry[3], dict)
                    and entry[3]
                    and entry[2] != entry[3]
                ), f"{label}: bad known job shape {entry!r}"
            for task, issue in (req.known_absent_jobs or {}).items():
                assert "." in task, f"{label}: bad pinned task {task!r}"
                assert re.fullmatch(r"PIDASHCONV-\d+", issue), (
                    f"{label}: pinned {task} must name its fix issue"
                )
            for entry in req.known_headers or []:
                assert (
                    isinstance(entry, (list, tuple))
                    and len(entry) == 4
                    and re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", entry[0])
                    and re.fullmatch(r"PIDASHCONV-\d+", entry[1])
                    and (entry[2] is None or isinstance(entry[2], str))
                    and (entry[3] is None or isinstance(entry[3], str))
                    and entry[2] != entry[3]
                ), f"{label}: bad known header {entry!r}"
            for entry in req.known_body_diffs or []:
                assert (
                    isinstance(entry, (list, tuple))
                    and len(entry) == 4
                    and isinstance(entry[0], str)
                    and re.fullmatch(r"[^/ ]+(/([^/ ]+))*", entry[0])
                    and re.fullmatch(r"PIDASHCONV-\d+", entry[1])
                    and entry[2] != entry[3]
                ), f"{label}: bad known body diff {entry!r}"
            for entry in req.body_contains or []:
                assert (
                    isinstance(entry, (list, tuple))
                    and len(entry) == 2
                    and isinstance(entry[0], str)
                    and entry[0]
                    and isinstance(entry[1], str)
                    and entry[1]
                ), f"{label}: bad body contains {entry!r}"
            if req.body_contains:
                assert req.skip_body, (
                    f"{label}: body_contains without skip_body never runs"
                )
            if req.skip_body:
                assert not (
                    req.known_body_diffs or req.body_blank_paths or req.body_regex_blanks
                ), f"{label}: skip_body with JSON-subtree pins is contradictory"
            bodies = sum(
                [req.json is not None, req.raw_body is not None, bool(req.form),
                 bool(req.files)]
            )
            assert bodies <= 1, f"{label}: json, raw_body, form and files are exclusive"
            if req.auth.startswith("admin:"):
                assert req.auth in sess.auth_users, (
                    f"{label}: {req.auth!r} needs an auth_users entry"
                )


def _sessions_ctx(django_users: set, rust_users: set, captures=None):
    from . import compare

    present = {"django": set(django_users), "rust": set(rust_users)}
    return compare.CompareContext(
        captures=dict(captures or {}),
        row_exists=lambda side, table, value: (
            table == "users" and str(value) in present[side]
        ),
    )


def _sessions_snap(*user_ids):
    return {
        "sessions": [
            {"session_key": f"key-{u}", "user_id": u, "session_data": "x"}
            for u in user_ids
        ]
    }


def test_sessions_created_user_ids_match_by_count():
    """View-created users (sign-up) mint different uuids per backend.

    Unknown ids that exist as users rows on their own side compare by
    count; anything else (bogus ids, one-side-only sign-ins) still fails.
    """
    from . import compare

    captures = {"member": ("u-member", "u-member")}
    dj = compare.normalise_snapshot(
        _sessions_snap("u-member", "u-dj-new"),
        "django",
        _sessions_ctx({"u-member", "u-dj-new"}, {"u-member", "u-rs-new"}, captures),
    )
    rs = compare.normalise_snapshot(
        _sessions_snap("u-member", "u-rs-new"),
        "rust",
        _sessions_ctx({"u-member", "u-dj-new"}, {"u-member", "u-rs-new"}, captures),
    )
    assert dj["sessions"] == [{"user_id": "{member}"}, {"user_id": "{created_user}"}]
    assert compare.compare_db(dj, rs) == []


def test_sessions_bogus_and_lopsided_ids_still_fail():
    from . import compare

    ctx = _sessions_ctx({"u-member"}, {"u-member"})
    dj = compare.normalise_snapshot(_sessions_snap("u-member", "u-bogus"), "django", ctx)
    rs = compare.normalise_snapshot(_sessions_snap("u-member"), "rust", ctx)
    assert compare.compare_db(dj, rs) != []
    same = compare.normalise_snapshot(_sessions_snap("u-member"), "django", ctx)
    assert compare.compare_db(same, rs) == []


def _mint_token_inline(user_id, password_field, last_login, email, secret, ts):
    """Independent Django-4.2 reset-token recipe (test oracle for effects)."""
    import base64
    import hashlib
    import hmac

    alphabet = "0123456789abcdefghijklmnopqrstuvwxyz"
    b36 = ""
    number = ts
    while number:
        number, remainder = divmod(number, 36)
        b36 = alphabet[remainder] + b36
    login_ts = "" if last_login is None else str(last_login)
    value = f"{user_id}{password_field}{login_ts}{ts}{email}"
    key = hashlib.sha256(
        ("django.contrib.auth.tokens.PasswordResetTokenGenerator" + secret).encode()
    ).digest()
    digest = hmac.new(key, msg=value.encode(), digestmod=hashlib.sha256).hexdigest()[::2]
    uidb64 = base64.urlsafe_b64encode(str(user_id).encode()).rstrip(b"=").decode()
    return uidb64, f"{b36}-{digest}"


def test_reset_token_validator_accepts_fresh_rejects_rest():
    import calendar
    import time
    from datetime import datetime

    from . import effects

    epoch2001 = calendar.timegm(datetime(2001, 1, 1).utctimetuple())
    now_ts = int(time.time()) - epoch2001
    uid, token = _mint_token_inline(
        "u-1", "pbkdf2$hash", None, "u@example.com", "s3cret", now_ts
    )
    assert effects.decode_reset_uid(uid) == "u-1"
    assert effects.validate_reset_token(
        token, "u-1", "pbkdf2$hash", None, "u@example.com", "s3cret"
    )
    # Tampered HMAC, wrong user row, expired stamp, garbage: all invalid.
    assert not effects.validate_reset_token(
        token[:-1] + ("0" if token[-1] != "0" else "1"),
        "u-1",
        "pbkdf2$hash",
        None,
        "u@example.com",
        "s3cret",
    )
    assert not effects.validate_reset_token(
        token, "u-2", "pbkdf2$hash", None, "u@example.com", "s3cret"
    )
    old_uid, old_token = _mint_token_inline(
        "u-1",
        "pbkdf2$hash",
        None,
        "u@example.com",
        "s3cret",
        now_ts - effects.RESET_TOKEN_TIMEOUT_SECONDS - 1,
    )
    assert effects.decode_reset_uid(old_uid) == "u-1"
    assert not effects.validate_reset_token(
        old_token, "u-1", "pbkdf2$hash", None, "u@example.com", "s3cret"
    )
    assert not effects.validate_reset_token(
        "bogus-token", "u-1", "pbkdf2$hash", None, "u@example.com", "s3cret"
    )
    assert effects.decode_reset_uid("!!!") is None


def test_blank_valid_tokens_only_blanks_valid():
    import calendar
    import time
    from datetime import datetime

    from . import compare
    from . import effects

    epoch2001 = calendar.timegm(datetime(2001, 1, 1).utctimetuple())
    now_ts = int(time.time()) - epoch2001
    row = {
        "id": "u-1",
        "password": "pbkdf2$hash",
        "last_login": None,
        "email": "u@example.com",
    }
    uid, token = _mint_token_inline(
        "u-1", "pbkdf2$hash", None, "u@example.com", "s3cret", now_ts
    )
    task = "pi_dash.bgtasks.forgot_password_task.forgot_password"
    ctx = compare.CompareContext(
        payload_reset_tokens={task: [("args/2", "args/3")]},
        db_row=lambda side, table, value: dict(row) if value == "u-1" else None,
        secret_key="s3cret",
    )
    args = ["N", "u@example.com", uid, token, "http://localhost"]
    blanked, _ = compare._blank_valid_tokens(list(args), {}, task, "django", ctx)
    assert blanked[3] == effects.VOLATILE_PLACEHOLDER
    assert blanked[:3] == args[:3]
    # A token minted for another user stays put (fails the compare loudly).
    _, other = _mint_token_inline(
        "u-2", "pbkdf2$hash", None, "u@example.com", "s3cret", now_ts
    )
    kept, _ = compare._blank_valid_tokens(
        ["N", "u@example.com", uid, other, "http://localhost"], {}, task, "django", ctx
    )
    assert kept[3] == other


def test_blank_value_diffs_exact_match_only():
    """Pinned payload scalars blank only on exact per-side match."""
    import json

    from . import compare
    from . import effects

    task = "pi_dash.bgtasks.issue_activities_task.issue_activity"
    ctx = compare.CompareContext(
        payload_value_diffs={
            task: [
                ("kwargs/current_instance/sequence_id", "PIDASHCONV-897", 3, 908),
                ("kwargs/plain", "PIDASHCONV-897", "a", "b"),
            ]
        },
    )
    # Nested-JSON descent: the int blanks, siblings survive, and the
    # re-serialized string stays canonical (sort_keys, compact).
    kwargs = {"current_instance": json.dumps({"sequence_id": 3, "name": "t"})}
    _, blanked = compare._blank_value_diffs([], dict(kwargs), task, "django", ctx)
    assert json.loads(blanked["current_instance"]) == {
        "sequence_id": effects.VOLATILE_PLACEHOLDER,
        "name": "t",
    }
    assert blanked["current_instance"] == json.dumps(
        json.loads(blanked["current_instance"]), sort_keys=True, separators=(",", ":")
    )
    # Wrong side value stays put (fails the compare loudly).
    kwargs = {"current_instance": json.dumps({"sequence_id": 4, "name": "t"})}
    _, kept = compare._blank_value_diffs([], dict(kwargs), task, "django", ctx)
    assert json.loads(kept["current_instance"])["sequence_id"] == 4
    # Rust side blanks its own pinned value only.
    kwargs = {"current_instance": json.dumps({"sequence_id": 908, "name": "t"})}
    _, rblanked = compare._blank_value_diffs([], dict(kwargs), task, "rust", ctx)
    assert json.loads(rblanked["current_instance"])["sequence_id"] == (
        effects.VOLATILE_PLACEHOLDER
    )
    # Plain (non-nested) pointer, and an unresolved pointer skips.
    _, pblanked = compare._blank_value_diffs([], {"plain": "a"}, task, "django", ctx)
    assert pblanked["plain"] == effects.VOLATILE_PLACEHOLDER
    _, skipped = compare._blank_value_diffs([], {"other": 1}, task, "django", ctx)
    assert skipped == {"other": 1}


def test_blank_valid_magic_only_blanks_matching():
    """A queued magic code blanks only when it equals the stored token."""
    import json

    from . import compare
    from . import effects

    class _FakeRedis:
        def __init__(self, rows):
            self.rows = rows

        def get(self, key):
            value = self.rows.get(key)
            return json.dumps(value) if value is not None else None

        def close(self):
            pass

    task = "pi_dash.bgtasks.magic_link_code_task.magic_link"
    rows = {"magic_u@example.com": {"current_attempt": 0, "email": "u", "token": "492151"}}
    ctx = compare.CompareContext(
        payload_magic_codes={task: [("args/0", "args/2")]},
        redis_for=lambda side: _FakeRedis(rows),
    )
    args = ["u@example.com", "magic_u@example.com", "492151"]
    blanked, _ = compare._blank_valid_magic(list(args), {}, task, "django", ctx)
    assert blanked[2] == effects.VOLATILE_PLACEHOLDER
    assert blanked[:2] == args[:2]
    # Stored/queued split stays put (fails the compare loudly).
    split, _ = compare._blank_valid_magic(
        ["u@example.com", "magic_u@example.com", "000000"], {}, task, "django", ctx
    )
    assert split[2] == "000000"
    # Missing row stays put too.
    missing, _ = compare._blank_valid_magic(
        ["nobody@example.com", "magic_nobody@example.com", "492151"],
        {},
        task,
        "django",
        ctx,
    )
    assert missing[2] == "492151"


def test_set_cookie_parses_each_line_and_blanks_expires():
    """Two Set-Cookie lines stay separate; expires values blank.

    httpx folds repeated headers into one comma-joined string; parsing
    that as one cookie glues attributes across cookies. Expires is
    now()+max-age per backend (seconds apart), while Max-Age is the
    configured duration and still compares exactly.
    """
    import httpx

    from .compare import _parse_set_cookie

    headers = httpx.Headers(
        [
            (
                b"set-cookie",
                b"csrftoken=C1; expires=Sun, 10 Oct 2027 00:50:26 GMT; "
                b"Max-Age=31449600; Path=/; SameSite=Lax; HttpOnly",
            ),
            (
                b"set-cookie",
                b"session-id=S1; expires=Sun, 18 Oct 2026 00:50:26 GMT; "
                b"Max-Age=604800; Path=/; SameSite=Lax; HttpOnly",
            ),
        ]
    )
    assert _parse_set_cookie(headers) == [
        (
            "csrftoken",
            "HttpOnly; Max-Age=31449600; Path=/; SameSite=Lax; "
            "expires=<replay-volatile>",
        ),
        (
            "session-id",
            "HttpOnly; Max-Age=604800; Path=/; SameSite=Lax; "
            "expires=<replay-volatile>",
        ),
    ]
    assert _parse_set_cookie(httpx.Headers([])) == []


def _fake_response(status: int, body: bytes, headers=None):
    import httpx

    return httpx.Response(status, content=body, headers=list((headers or {}).items()))


def test_known_body_pin_passes_exact_and_fails_on_change():
    """Exact per-side body pins excuse the divergence but stay loud."""
    import json

    from . import compare

    ctx = compare.CompareContext()
    dj = _fake_response(
        400,
        json.dumps({"error_code": 5102, "email": "a@x.com"}).encode(),
        {"content-type": "application/json"},
    )
    rs = _fake_response(
        400,
        json.dumps({"error_code": 5102, "email": "magic_a@x.com"}).encode(),
        {"content-type": "application/json"},
    )
    pin = [("email", "PIDASHCONV-881", "a@x.com", "magic_a@x.com")]
    assert compare.compare_response(dj, rs, ctx, known_body=pin) is None
    # Unpinned, the same pair fails.
    assert compare.compare_response(dj, rs, ctx) is not None
    # Fix landed (Rust returns the bare address): the pin fails loudly.
    fixed = _fake_response(
        400,
        json.dumps({"error_code": 5102, "email": "a@x.com"}).encode(),
        {"content-type": "application/json"},
    )
    error = compare.compare_response(dj, fixed, ctx, known_body=pin)
    assert error is not None and "PIDASHCONV-881" in error
    # Django changed: loud too.
    other = _fake_response(
        400,
        json.dumps({"error_code": 5102, "email": "b@x.com"}).encode(),
        {"content-type": "application/json"},
    )
    error = compare.compare_response(other, rs, ctx, known_body=pin)
    assert error is not None and "Django body changed" in error


def test_known_cell_diff_blanks_exact_and_fails_on_change():
    """Cell pins excuse one cell of matched rows, nothing else."""
    from . import compare
    from . import effects

    pin = [("issues", "PIDASHCONV-882", {"name": "Inbox idea"}, "description_stripped", "", None)]
    dj = {"issues": [{"id": "1", "name": "Inbox idea", "description_stripped": ""}]}
    rs = {"issues": [{"id": "1", "name": "Inbox idea", "description_stripped": None}]}
    dj_norm = compare.normalise_snapshot(
        dj, "django", compare.CompareContext(cell_diffs=pin)
    )
    rs_norm = compare.normalise_snapshot(rs, "rust", compare.CompareContext(cell_diffs=pin))
    assert compare.compare_db(dj_norm, rs_norm) == []
    # Unpinned, the same pair fails.
    assert (
        compare.compare_db(
            compare.normalise_snapshot(dj, "django", compare.CompareContext()),
            compare.normalise_snapshot(rs, "rust", compare.CompareContext()),
        )
        != []
    )
    # Fix landed (Rust stores ''): the pin no longer matches → loud.
    fixed = {"issues": [{"id": "1", "name": "Inbox idea", "description_stripped": ""}]}
    assert (
        compare.compare_db(
            dj_norm,
            compare.normalise_snapshot(fixed, "rust", compare.CompareContext(cell_diffs=pin)),
        )
        != []
    )
    # Other rows are untouched by the pin.
    other = {"issues": [{"id": "2", "name": "Other", "description_stripped": None}]}
    other_norm = compare.normalise_snapshot(
        other, "rust", compare.CompareContext(cell_diffs=pin)
    )
    assert other_norm["issues"][0]["description_stripped"] is None
    assert effects.VOLATILE_PLACEHOLDER in dj_norm["issues"][0].values()


def test_body_blank_paths_blank_presence_and_fail_on_missing():
    """Blank paths excuse volatile subtrees but never dropped fields."""
    import json

    from . import compare

    ctx = compare.CompareContext()
    dj = _fake_response(
        200,
        json.dumps({"asset_id": "a", "key": "ws/111-shot.png"}).encode(),
        {"content-type": "application/json"},
    )
    rs = _fake_response(
        200,
        json.dumps({"asset_id": "b", "key": "ws/222-shot.png"}).encode(),
        {"content-type": "application/json"},
    )
    # Unblanked, the ids and keys fail.
    assert compare.compare_response(dj, rs, ctx) is not None
    # Blanked, only the stable leaf compares.
    assert compare.compare_response(dj, rs, ctx, blank_paths=["asset_id", "key"]) is None
    # A path missing on one side fails loudly (never a silent excuse).
    dropped = _fake_response(
        200,
        json.dumps({"asset_id": "b"}).encode(),
        {"content-type": "application/json"},
    )
    error = compare.compare_response(dj, dropped, ctx, blank_paths=["key"])
    assert error is not None and "key" in error and "rust" in error


def test_skip_body_skips_scaffolding_but_enforces_contains():
    """skip_body excuses framework 500 pages, never a different failure."""
    from . import compare

    ctx = compare.CompareContext()
    dj = _fake_response(
        500,
        b"<html><title>AttributeError\n at /api/x</title>...18223...</html>",
        {"content-type": "text/html; charset=utf-8"},
    )
    rs = _fake_response(
        500,
        b'{"error":"Something went wrong please try again later"}',
        {"content-type": "application/json"},
    )
    # Unskipped, content-type + bytes fail.
    assert compare.compare_response(dj, rs, ctx) is not None
    pins = [("<title>AttributeError", "Something went wrong")]
    assert (
        compare.compare_response(dj, rs, ctx, skip_body=True, body_contains=pins)
        is None
    )
    # A side that stops failing the pinned way fails loudly.
    other = _fake_response(
        500,
        b'{"error":"Something went wrong please try again later"}',
        {"content-type": "application/json"},
    )
    error = compare.compare_response(
        other, rs, ctx, skip_body=True, body_contains=pins
    )
    assert error is not None and "Django body lacks" in error
    # Status still compares under the skip.
    ok = _fake_response(200, b"{}", {"content-type": "application/json"})
    error = compare.compare_response(ok, rs, ctx, skip_body=True, body_contains=pins)
    assert error is not None and "status differs" in error


def test_db_jsonb_nested_timestamps_blank_but_null_still_compares():
    """TIMESTAMP_JSON_KEYS apply inside JSONB DB cells (chat_timing)."""
    from . import compare

    ctx = compare.CompareContext()
    dj = {"agent_chat_event": [
        {"id": "a", "payload": {"stage": "x", "recorded_at": "2026-10-11T04:00:00+00:00"}},
    ]}
    rs = {"agent_chat_event": [
        {"id": "a", "payload": {"stage": "x", "recorded_at": "2026-10-11T04:00:01+00:00"}},
    ]}
    dj_norm = compare.normalise_snapshot(dj, "django", ctx)
    rs_norm = compare.normalise_snapshot(rs, "rust", ctx)
    assert compare.compare_db(dj_norm, rs_norm) == []
    # A missing stamp (null vs stamped) still fails.
    rs_null = {"agent_chat_event": [
        {"id": "a", "payload": {"stage": "x", "recorded_at": None}},
    ]}
    rs_null_norm = compare.normalise_snapshot(rs_null, "rust", ctx)
    assert compare.compare_db(dj_norm, rs_null_norm) != []


def test_fernet_decrypt_round_trip_and_rejects_garbage():
    """License ciphertext decrypts under Django's key schedule only."""
    import base64
    import hashlib

    from cryptography.fernet import Fernet

    from . import effects

    secret = "test-secret-key"
    dk = hashlib.pbkdf2_hmac("sha256", secret.encode(), b"salt", 100000)
    token = Fernet(base64.urlsafe_b64encode(dk)).encrypt(b"fresh").decode()
    assert effects.fernet_decrypt(token, secret) == "fresh"
    # Same plaintext encrypts differently every time (random IV).
    other = Fernet(base64.urlsafe_b64encode(dk)).encrypt(b"fresh").decode()
    assert other != token
    assert effects.fernet_decrypt(other, secret) == "fresh"
    # Garbage, empty, and wrong-key cells stay raw (compare loudly).
    assert effects.fernet_decrypt("old", secret) is None
    assert effects.fernet_decrypt("", secret) is None
    assert effects.fernet_decrypt(token, "wrong-secret") is None
    assert effects.fernet_decrypt(token, "") is None


def test_fernet_direct_decrypts_frozen_hex_cells():
    """Assistant-schedule columns decrypt raw-key ciphertext, hex or str."""
    from cryptography.fernet import Fernet

    from . import compare, effects

    key = Fernet.generate_key().decode()
    token = Fernet(key.encode()).encrypt(b"test-key-123")
    assert effects.fernet_decrypt_direct(token.hex(), [key]) == "test-key-123"
    assert effects.fernet_decrypt_direct(token, [key]) == "test-key-123"
    assert effects.fernet_decrypt_direct(token.hex(), ["bogus"]) is None
    assert effects.fernet_decrypt_direct("deadbeef", [key]) is None
    assert effects.fernet_decrypt_direct(None, [key]) is None
    # End to end through normalise_snapshot: two encryptions of the
    # same secret compare equal with no is_encrypted flag involved.
    other = Fernet(key.encode()).encrypt(b"test-key-123").hex()
    table = "assistant_llmconfig"
    tuned = compare.CompareContext(
        encrypted_columns={table: [("api_key_encrypted", "assistant")]},
        assistant_encryption_keys=key,
    )
    dj = {table: [{"id": "1", "api_key_encrypted": token.hex()}]}
    rs = {table: [{"id": "1", "api_key_encrypted": other}]}
    assert token.hex() != other
    assert (
        compare.compare_db(
            compare.normalise_snapshot(dj, "django", tuned),
            compare.normalise_snapshot(rs, "rust", tuned),
        )
        == []
    )


def test_encrypted_columns_decrypt_flagged_rows_only():
    """Encrypted columns compare decrypted plaintext, nothing else."""
    import base64
    import hashlib

    from cryptography.fernet import Fernet

    from . import compare

    secret = "test-secret-key"
    dk = hashlib.pbkdf2_hmac("sha256", secret.encode(), b"salt", 100000)
    cipher = Fernet(base64.urlsafe_b64encode(dk))
    dj_ct = cipher.encrypt(b"fresh").decode()
    rs_ct = cipher.encrypt(b"fresh").decode()
    assert dj_ct != rs_ct
    table = "instance_configurations"
    tuned = compare.CompareContext(
        encrypted_columns={table: ["value"]}, secret_key=secret
    )
    dj = {table: [{"id": "1", "key": "S", "value": dj_ct, "is_encrypted": True}]}
    rs = {table: [{"id": "1", "key": "S", "value": rs_ct, "is_encrypted": True}]}
    dj_norm = compare.normalise_snapshot(dj, "django", tuned)
    rs_norm = compare.normalise_snapshot(rs, "rust", tuned)
    assert compare.compare_db(dj_norm, rs_norm) == []
    assert dj_norm[table][0]["value"] == "fresh"
    # Unflagged rows still compare raw.
    plain = {table: [{"id": "2", "key": "P", "value": "new", "is_encrypted": False}]}
    assert (
        compare.normalise_snapshot(plain, "django", tuned)[table][0]["value"]
        == "new"
    )
    # A plaintext seed under a true flag stays raw on both sides.
    seed = {table: [{"id": "3", "key": "S", "value": "old", "is_encrypted": True}]}
    assert (
        compare.normalise_snapshot(seed, "django", tuned)[table][0]["value"]
        == "old"
    )


def test_reasons_shape():
    seen = set()
    for bucket in ("MISSING_FIX", "EXCLUDED", "OWNED_UNCOVERED"):
        entries = getattr(reasons, bucket)
        assert isinstance(entries, dict), f"{bucket} is not a dict"
        for key, reason in entries.items():
            assert isinstance(key, tuple) and len(key) == 2, f"{bucket}: bad key {key!r}"
            shape, method = key
            assert shape.startswith("/"), f"{bucket}: bad shape {shape!r}"
            assert method in effects.WRITE_METHODS, f"{bucket}: bad method {method!r}"
            assert isinstance(reason, str) and reason.strip(), (
                f"{bucket}: empty reason for {key!r}"
            )
            assert key not in seen, f"{key!r} listed in two buckets"
            seen.add(key)
