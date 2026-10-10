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
        elif isinstance(value, list):
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
            assert user in (*world_seed.USER_KEYS, "api_key"), (
                f"{sess.name}: unknown auth user {user!r}"
            )
        if sess.seed is not None:
            assert callable(sess.seed), f"{sess.name}: seed is not callable"
        available = set(WORLD_KEYS)
        for index, req in enumerate(sess.requests):
            label = f"{sess.name} request {index} {req.method} {req.path}"
            assert req.method in HTTP_METHODS, f"{label}: bad method"
            assert req.path.startswith("/"), f"{label}: path must be absolute"
            assert req.route_template().startswith("/"), f"{label}: bad route"
            for placeholder in _placeholders(
                [req.path, req.params, req.headers, req.json]
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
            assert not (req.json is not None and req.raw_body is not None), (
                f"{label}: json and raw_body are exclusive"
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
