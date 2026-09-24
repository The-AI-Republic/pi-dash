# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Root conftest: make ``_harness`` importable for every domain suite.

Makes _harness importable when running `pytest <domain>` from this dir.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


# --- PIDASHCONV-83: shared live-test fixtures ---
# Union with the baseline path setup above.
import pytest
from _harness import config  # noqa: E402
from _harness.db import connect  # noqa: E402
from _harness.sinks import smtp_sink, webhook_sink  # noqa: E402,F401


@pytest.fixture(scope="session")
def db_conn():
    if not os.environ.get(config.DATABASE_URL):
        raise RuntimeError(
            "live test needs DATABASE_URL "
            "(see rust-api/contract-tests/README.md)"
        )
    conn = connect()
    yield conn
    conn.close()


@pytest.fixture(scope="session")
def broker_url():
    if not os.environ.get(config.CELERY_BROKER_URL):
        raise RuntimeError(
            "live test needs CELERY_BROKER_URL "
            "(see rust-api/contract-tests/README.md)"
        )
    return os.environ[config.CELERY_BROKER_URL]
