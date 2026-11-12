"""v1_openapi oracle (domain D-23): the drf-spectacular schema surface.

No routes of its own to drive and no tables to diff — the stimuli are plain
HTTP GETs against the three served endpoints (see ``client.py``) and the
asserts read the document they return: shape, hook invariants
(``preprocess_filter_api_v1_paths``, ``postprocess_project_id_dual_form``),
exact route coverage via the committed golden file, method denials,
tenant-invariance, and public readability.

The suite needs no seeded tenant state except for the isolation tests, which
build two full worlds (``_harness.world.build``) and prove both tenants —
and anonymous callers — receive byte-identical documents.
"""

import json

import httpx
import pytest

from _harness import env
from . import client as sc


def _backend_up() -> bool:
    # ReDoc is static HTML: cheap reachability probe, no 1.4 MB doc generation.
    # Any HTTP verdict means the backend is up — including 401/403/404, which
    # the tests themselves must then fail on (permission change, flag off).
    # Only a connection failure skips the module.
    try:
        r = httpx.get(env.BASE_URL + sc.REDOC, timeout=10)
        return r.status_code in (200, 401, 403, 404, 405, 429)
    except Exception:
        return False


@pytest.fixture(scope="session", autouse=True)
def _backend():
    if not _backend_up():
        pytest.skip(
            f"backend not reachable at {env.BASE_URL}; boot Django per "
            "rust-api/contract-tests/README.md and set BASE_URL/DATABASE_URL",
            allow_module_level=True,
        )


@pytest.fixture(scope="session")
def schema_bytes() -> bytes:
    """Raw ``?format=json`` document fetched once, anonymously.

    The hard 200 also pins public readability; a permission change (e.g.
    restricting ``SERVE_PERMISSIONS``) fails here immediately — 403 is
    returned, never waited out.
    """
    r = sc.get_patient(sc.SCHEMA, params={"format": "json"})
    assert r.status_code == 200, f"schema doc unreachable: {r.status_code} {r.text[:200]}"
    return r.content


@pytest.fixture(scope="session")
def doc(schema_bytes) -> dict:
    return json.loads(schema_bytes)


@pytest.fixture(scope="session")
def tenants():
    """Two isolated tenant worlds for the isolation tests."""
    from _harness import world

    return world.build("s81a"), world.build("s81b")
