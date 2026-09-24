# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared black-box helpers for Rust contract suites.

Every suite runs against a live server (Django today, Rust via the proxy
tomorrow) driven by BASE_URL, and seeds rows straight into Postgres via
DATABASE_URL. Nothing here imports Django or touches its test client.

Shared contract-test harness (first created for PIDASHCONV-92, app
notifications). Extend this package; never fork per-domain copies.

Shared contract-test harness: black-box helpers for pytest + httpx suites.
Created on first use (PIDASHCONV-15, license domain); extend, never fork.
The suite runs against a live Django server and seeds Postgres directly.
Never import Django here.
"""

# Shared contract-test helpers. Extend this package; never fork per-domain copies.
# Shared contract-test harness (first use: PIDASHCONV-19, integrations domain).
# Extend, never fork: new domains add helpers here, not copies.
"""Black-box helpers for Celery-task contract tests.

Nothing here imports Django. Tests talk to the system under test only via:
  * CELERY_BROKER_URL — publish jobs in Celery protocol-v2 wire format,
    observe queue depth and worker-published follow-ups;
  * DATABASE_URL — seed rows straight into Postgres, snapshot/diff tables;
  * BASE_URL — drive the live Django HTTP API where a DB write must pass
    through ORM signals (e.g. github_signals completion hook);
  * local stub sinks (sinks.py) — capture outbound side effects.
"""

from .db import Database
from .http import admin_client, anon_client
from .settings import base_url, database_url, secret_key

__all__ = ["Database", "admin_client", "anon_client", "base_url", "database_url", "secret_key"]
