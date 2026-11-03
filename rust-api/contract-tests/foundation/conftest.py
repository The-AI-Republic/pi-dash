"""Shared fixtures for the foundation contract suite (PIDASHCONV-723).

Cross-cutting middleware behavior, not one domain: a session whose
``_auth_user_id`` has no ``users`` row (deleted user, stale session) must
read as anonymous — Django's ``get_user`` 401 — on every ported route.

Probes need no domain seeding: the 401 fires before any handler reads.
"""

from __future__ import annotations

import pytest

from _harness.client import base_url  # noqa: F401  (re-exported fixture)
from _harness.db import db_conn, get_database_url  # noqa: F401
