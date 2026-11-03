"""Runner websocket oracle: ``ws/runner/`` never yields a usable channel.

Source of truth is the consumer source,
``apps/api/pi_dash/runner/consumers.py``: ``RunnerConsumer.connect()``
unconditionally sends ``close(CLOSE_CODE_PROTOCOL_UNSUPPORTED)`` (1008)
*before* accepting, so the retired control-plane WebSocket is gone, and
``apps/api/pi_dash/runner/routing.py`` mounts exactly one path
(``ws/runner/``) — there are no sub-paths to multiplex.

What that means on the wire depends on the server in front of Django, and
the suite pins both renderings exactly (see ``ws_fingerprint``):

- uvicorn (the pinned WS server, ``uvicorn[standard]`` in
  ``apps/api/requirements/base.txt``): the app's close-before-accept makes
  uvicorn answer the HTTP upgrade with **403 Forbidden**; a path no route
  matches raises in Channels' ``URLRouter`` and uvicorn answers **500**.
- runserver (no WS support; what CI boots): the upgrade is routed as a
  plain GET into Django's URL router, which has no ``ws/`` route, so every
  ``ws/`` handshake is answered **404**.

Either way the invariant under test holds: the handshake never completes,
no frame is ever exchanged, and no credential — valid runner token,
machine token, session cookie, or none — opens a channel. If the control
plane is ever re-enabled (handshake completes), every test here fails.
"""

from __future__ import annotations

import asyncio
import secrets
from dataclasses import dataclass, field

import pytest
from websockets.asyncio.client import connect
from websockets.exceptions import InvalidStatus

from _harness.http import anonymous_client

from .conftest import DAEMON

pytestmark = pytest.mark.contract

# Mirror of ``CLOSE_CODE_PROTOCOL_UNSUPPORTED`` in
# ``apps/api/pi_dash/runner/consumers.py``. The constant itself never
# appears on the wire through the supported servers (close-before-accept
# is rendered as a handshake rejection, above); it is recorded here so a
# reader can trace the rejection back to the source line that mandates it.
CLOSE_CODE_PROTOCOL_UNSUPPORTED = 1008

WS_RUNNER = "/ws/runner/"


def _ws_url(base_url: str, path: str) -> str:
    if base_url.startswith("https://"):
        return "wss://" + base_url[len("https://"):] + path
    assert base_url.startswith("http://"), base_url
    return "ws://" + base_url[len("http://"):] + path


@dataclass
class WsOutcome:
    """Exactly one of: rejected (int status), or accepted (never expected)."""

    accepted: bool
    status: int | None = None
    close_code: int | None = None
    frames: list = field(default_factory=list)


async def _attempt(url: str, headers: list[tuple[str, str]] | None = None) -> WsOutcome:
    """Open one WS handshake; report whether a channel came up.

    A completed handshake is always a failure of the contract under test,
    so this helper never raises for it — it reports, and the test fails
    with the observed detail.
    """
    try:
        async with connect(url, additional_headers=headers, open_timeout=10) as ws:
            try:
                message = await asyncio.wait_for(ws.recv(), timeout=5)
                return WsOutcome(accepted=True, frames=[message])
            except asyncio.TimeoutError:
                return WsOutcome(accepted=True)
            except Exception as exc:  # noqa: BLE001 - report, don't raise
                return WsOutcome(
                    accepted=True,
                    close_code=getattr(exc, "code", None),
                    frames=[],
                )
    except InvalidStatus as exc:
        return WsOutcome(accepted=False, status=exc.response.status_code)


def _ws_attempt(settings, path: str, headers: list[tuple[str, str]] | None = None) -> WsOutcome:
    return asyncio.run(_attempt(_ws_url(settings.base_url, path), headers))


@pytest.fixture(scope="module")
def ws_fingerprint(settings):
    """Pin this backend's WS rendering: (live-path code, unknown-path code).

    The only supported renderings are ``(403, 500)`` (uvicorn: the
    consumer's close-before-accept is answered 403, unmatched routes raise
    to 500) and ``(404, 404)`` (runserver: the upgrade is routed as GET
    into Django, which 404s every ``ws/`` path). Any handshake that
    completes, or any other code, fails the suite here, once, instead of
    in every test.
    """
    live = _ws_attempt(settings, WS_RUNNER)
    gone = _ws_attempt(settings, f"/ws/runner/gone-{secrets.token_hex(6)}/")
    assert not live.accepted, f"ws/runner/ handshake unexpectedly completed: {live}"
    assert not gone.accepted, f"gone-path handshake unexpectedly completed: {gone}"
    pair = (live.status, gone.status)
    assert pair in {(403, 500), (404, 404)}, f"unknown WS rendering: {pair}"
    return {"live": live.status, "gone": gone.status}


def _assert_rejected(outcome: WsOutcome, expected_status: int, where: str) -> None:
    assert not outcome.accepted, f"{where}: handshake unexpectedly completed: {outcome}"
    assert outcome.frames == [], f"{where}: frames before rejection: {outcome.frames}"
    assert outcome.status == expected_status, (
        f"{where}: rejection status {outcome.status} != {expected_status}"
    )


def _enroll_access_token(settings, seeder, owner: dict, workspace: dict, pod: dict) -> dict:
    """Redeem a real runner access token via the enroll flow (HTTP)."""
    raw = f"apd_en_{seeder.tag}{secrets.token_hex(8)}"
    host_label = f"ws-{seeder.tag}-{secrets.token_hex(4)}"[:200]
    seeded = seeder.enroll_runner(owner["id"], workspace["id"], pod["id"], raw)
    with anonymous_client(settings.base_url) as client:
        response = client.post(
            f"{DAEMON}/runners/enroll/",
            json={"enrollment_token": raw, "host_label": host_label},
        )
    assert response.status_code == 201, response.text
    return {
        "access_token": response.json()["access_token"],
        "runner_id": seeded["id"],
        "owner_id": owner["id"],
        "host_label": host_label,
    }


def _unenroll(seeder, flow: dict) -> None:
    """Delete exactly the rows the enroll flow minted (mirrors conftest)."""
    seeder.db.execute("DELETE FROM runner_session WHERE runner_id = %s", (flow["runner_id"],))
    seeder.db.execute(
        "DELETE FROM machine_token WHERE user_id = %s AND host_label = %s",
        (flow["owner_id"], flow["host_label"]),
    )
    seeder.db.execute(
        "UPDATE runner SET dev_machine_id = NULL WHERE id = %s", (flow["runner_id"],)
    )
    seeder.db.execute(
        "DELETE FROM dev_machine WHERE owner_id = %s AND host_label = %s",
        (flow["owner_id"], flow["host_label"]),
    )


def _second_world(seeder) -> dict:
    """A second, isolated tenant: owner + workspace + project + pod."""
    seeder.ensure_instance()
    owner = seeder.create_user()
    workspace = seeder.create_workspace(owner["id"])
    seeder.create_member(workspace["id"], owner["id"])
    project = seeder.create_project(workspace["id"])
    pod = seeder.create_pod(workspace["id"], project["id"], is_default=True)
    return {"owner": owner, "workspace": workspace, "project": project, "pod": pod}


# -- live path: never accepts ------------------------------------------------

def test_ws_runner_rejects_anonymous(settings, ws_fingerprint):
    outcome = _ws_attempt(settings, WS_RUNNER)
    _assert_rejected(outcome, ws_fingerprint["live"], "anonymous ws/runner/")


def test_ws_runner_rejects_valid_runner_auth(settings, seeder, daemon_world, ws_fingerprint):
    flow = _enroll_access_token(
        settings, seeder, daemon_world["owner"], daemon_world["workspace"], daemon_world["pod"]
    )
    try:
        authed = _ws_attempt(
            settings,
            WS_RUNNER,
            headers=[("Authorization", f"Bearer {flow['access_token']}")],
        )
        _assert_rejected(authed, ws_fingerprint["live"], "runner-token ws/runner/")
        # Auth is irrelevant to the consumer (it closes before reading any
        # header): a valid runner token is rejected exactly like no auth.
        anonymous = _ws_attempt(settings, WS_RUNNER)
        assert (authed.accepted, authed.status) == (anonymous.accepted, anonymous.status)
    finally:
        _unenroll(seeder, flow)


def test_ws_runner_rejects_machine_token(settings, daemon_world, machine_flow, ws_fingerprint):
    outcome = _ws_attempt(
        settings,
        WS_RUNNER,
        headers=[
            ("Authorization", f"Bearer {machine_flow['machine_token']}"),
            ("X-Runner-Id", machine_flow["runner_id"]),
        ],
    )
    _assert_rejected(outcome, ws_fingerprint["live"], "machine-token ws/runner/")


# -- gone path ----------------------------------------------------------------

def test_ws_unknown_path_rejected(settings, ws_fingerprint):
    outcome = _ws_attempt(settings, f"/ws/runner/gone-{secrets.token_hex(6)}/")
    _assert_rejected(outcome, ws_fingerprint["gone"], "gone ws sub-path")
    assert outcome.close_code is None


# -- tenant isolation ----------------------------------------------------------

def test_ws_tenant_isolation(settings, seeder, daemon_world, ws_fingerprint):
    world_b = _second_world(seeder)
    flow_a = _enroll_access_token(
        settings, seeder, daemon_world["owner"], daemon_world["workspace"], daemon_world["pod"]
    )
    flow_b = _enroll_access_token(
        settings, seeder, world_b["owner"], world_b["workspace"], world_b["pod"]
    )
    try:
        outcome_a = _ws_attempt(
            settings, WS_RUNNER, headers=[("Authorization", f"Bearer {flow_a['access_token']}")]
        )
        outcome_b = _ws_attempt(
            settings, WS_RUNNER, headers=[("Authorization", f"Bearer {flow_b['access_token']}")]
        )
        # No channel exists for either tenant, so no cross-tenant stream can
        # leak: both tokens are rejected exactly like an anonymous call.
        _assert_rejected(outcome_a, ws_fingerprint["live"], "workspace-A token")
        _assert_rejected(outcome_b, ws_fingerprint["live"], "workspace-B token")
        assert (outcome_a.accepted, outcome_a.status) == (
            outcome_b.accepted,
            outcome_b.status,
        )
    finally:
        _unenroll(seeder, flow_a)
        _unenroll(seeder, flow_b)


# -- denied-permission equivalent at the WS layer -------------------------------

def test_ws_denied_credential_classes_rejected(settings, seeder, daemon_world, ws_fingerprint):
    garbage = _ws_attempt(
        settings, WS_RUNNER, headers=[("Authorization", "Bearer not-a-real-token")]
    )
    _assert_rejected(garbage, ws_fingerprint["live"], "garbage bearer")
    # A valid HTTP credential of the wrong class (API key presented as a
    # runner bearer) gains nothing at the WS layer.
    wrong_class = _ws_attempt(
        settings,
        WS_RUNNER,
        headers=[("Authorization", f"Bearer {daemon_world['api_token']['token']}")],
    )
    _assert_rejected(wrong_class, ws_fingerprint["live"], "api-key-as-bearer")
    assert (garbage.accepted, garbage.status) == (wrong_class.accepted, wrong_class.status)


def test_ws_session_cookie_rejected(settings, user_client, ws_fingerprint):
    cookie = "; ".join(f"{c.name}={c.value}" for c in user_client.cookies.jar)
    assert cookie, "expected a session cookie from the login fixture"
    outcome = _ws_attempt(settings, WS_RUNNER, headers=[("Cookie", cookie)])
    _assert_rejected(outcome, ws_fingerprint["live"], "session-cookie ws/runner/")


# -- the path is not served over HTTP either -------------------------------------

def test_ws_path_not_served_over_http(anon_client):
    assert anon_client.get("/ws/runner/").status_code == 404
    assert anon_client.get(f"/ws/runner/gone-{secrets.token_hex(6)}/").status_code == 404
