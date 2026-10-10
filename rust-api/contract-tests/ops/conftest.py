# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Fixtures for the D-37 ops contract suites (binary-driven).

Unlike the HTTP suites, these tests shell out to the built
``pidash-api`` binary (``RUST_BIN``, defaulting to the workspace
``target/debug`` build) with piped stdin/stdout/stderr and assert
byte-exact streams plus DB before/after diffs. ``DATABASE_URL`` is
required and points at a Django-migrated scratch database. Every
test seeds uuid-unique rows, so no cleanup pass is needed and
re-runs never collide.
"""

import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from _harness.db import connect  # noqa: E402


def _default_rust_bin() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.join(here, "..", "..", "target", "debug", "pidash-api")


@pytest.fixture(scope="session")
def rust_bin() -> str:
    path = os.environ.get("RUST_BIN", _default_rust_bin())
    path = os.path.abspath(path)
    if not (os.path.isfile(path) and os.access(path, os.X_OK)):
        raise pytest.UsageError(
            f"ops contract tests need the built binary at {path} "
            "(cargo build --bin pidash-api, or set RUST_BIN)"
        )
    return path


@pytest.fixture()
def db():
    if not os.environ.get("DATABASE_URL"):
        raise pytest.UsageError(
            "ops contract tests need DATABASE_URL "
            "(a Django-migrated scratch database)"
        )
    conn = connect()
    try:
        yield conn
    finally:
        conn.close()
