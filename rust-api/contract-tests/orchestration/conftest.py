"""Orchestration-engine oracle (domain D-12).

No routes of its own: signals, state transitions, workpad, phase machine.
Stimuli are the surfaces that drive the engine — issue state PATCHes (with
and without X-Pi-Dash-Run-Id), the wait / re-tick endpoints, the workpad and
relations endpoints, and Celery tasks published in wire format. Asserts read
the engine's tables (issue_agent_ticker, agent_run, issues, issue_comments).
"""

import httpx
import pytest

from _harness import api, db, env


def _backend_up() -> bool:
    try:
        r = httpx.get(env.BASE_URL + "/api/v1/workspaces/nope/projects/nope/work-items/nope/", timeout=5)
        return r.status_code in (401, 403, 404)
    except Exception:
        return False


def pytest_configure(config):
    config.addinivalue_line("markers", "contract: backend behavior contract tests")


@pytest.fixture(scope="session", autouse=True)
def _backend():
    if not _backend_up():
        pytest.skip(
            f"backend not reachable at {env.BASE_URL}; boot Django per "
            "rust-api/contract-tests/README.md and set BASE_URL/DATABASE_URL/CELERY_BROKER_URL",
            allow_module_level=True,
        )


@pytest.fixture
def api_client():
    from _harness import world

    w = world.build("oc")
    return w, api.Api(w.api_key)
