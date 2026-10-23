"""Bad-REDIS-env boot probe (PIDASHCONV-1316): a backend started with
``REDIS_MAX_CONNECTIONS=`` (empty) and ``REDIS_SOCKET_TIMEOUT=abc``
(non-numeric) must still boot and serve requests.

Django stores these tuning vars as raw strings
(``common.py:238-241``) and coerces leniently at use
(``redis.py:22-46``: defaults for the timeouts/interval, ``None`` for
max-connections), so such an env boots fine. Pre-fix Rust failed the
whole settings load at boot (``TypeMismatch``) and ``serve``/``worker``
exited before listening.

This test is a plain ``BASE_URL`` smoke probe: it asserts the backend
it runs against boots and serves. It is executed locally against
bad-env boots of each backend — pre-fix it passes against Django and
errors against Rust (connection refused: the binary never listens);
post-fix it passes against both. The durable regression pins for the
coercion values themselves are the ``settings.rs`` unit tests (cargo
test, run in CI), since CI boots both backends with a good env and a
subprocess-spawning test cannot run there (the Django leg runs before
``cargo build``).
"""

from __future__ import annotations

import httpx
import pytest

pytestmark = pytest.mark.contract

UNAUTHORIZED_BODY = b'{"detail":"Authentication credentials were not provided."}'


def test_bad_redis_env_backend_boots_and_serves(base_url):
    """The backend under test answers an unauthenticated request.

    Run with the backend booted under ``REDIS_MAX_CONNECTIONS=`` and
    ``REDIS_SOCKET_TIMEOUT=abc``: Django serves; pre-fix Rust never
    listens (TypeMismatch at boot) so this errors; post-fix both serve.
    """
    resp = httpx.get(f"{base_url}/api/workspaces/", timeout=10.0)
    assert resp.status_code == 401, (resp.status_code, resp.content[:200])
    assert resp.content == UNAUTHORIZED_BODY, resp.content[:200]
