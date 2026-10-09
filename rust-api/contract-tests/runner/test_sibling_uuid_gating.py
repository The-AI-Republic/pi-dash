"""Sibling-router UUID gating (PIDASHCONV-798).

Django's ``<uuid:>`` converter (lowercase-only hex) 404s non-canonical
path segments before auth runs. Rust falls through to Django, which renders
its own framework 404 (HTML) — byte-identical by construction. The status
plus the HTML content type pin the fallthrough: an unfixed Rust answers
401/403 anon, a JSON 404/409 authed, or proceeds past the parse for
uppercase ids.
"""

from __future__ import annotations

import pytest

from _harness.http import bearer_client
from _harness.web import web_delete, web_patch, web_post

WEB = "/api/runners"
DAEMON = "/api/v1/runner"

pytestmark = pytest.mark.contract

UPPER = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"
SEGMENTS = ["not-a-uuid", "123", UPPER]
GOOD = "12345678-1234-abcd-ef01-234567890abc"


def _assert_framework_404(response) -> None:
    assert response.status_code == 404, response.text
    assert "text/html" in response.headers.get("content-type", "")


# -- manage.rs: runner detail/patch + pod detail/patch/delete -----------------


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_runner_detail_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(user_client.get(f"{WEB}/{segment}/"))
    _assert_framework_404(anon_client.get(f"{WEB}/{segment}/"))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_runner_patch_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(web_patch(user_client, f"{WEB}/{segment}/", {"name": "x"}))
    _assert_framework_404(anon_client.patch(f"{WEB}/{segment}/", json={"name": "x"}))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_runner_delete_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(web_delete(user_client, f"{WEB}/{segment}/"))
    _assert_framework_404(anon_client.delete(f"{WEB}/{segment}/"))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_pod_detail_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(user_client.get(f"{WEB}/pods/{segment}/"))
    _assert_framework_404(anon_client.get(f"{WEB}/pods/{segment}/"))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_pod_patch_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(web_patch(user_client, f"{WEB}/pods/{segment}/", {"name": "x"}))
    _assert_framework_404(anon_client.patch(f"{WEB}/pods/{segment}/", json={"name": "x"}))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_pod_delete_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(web_delete(user_client, f"{WEB}/pods/{segment}/"))
    _assert_framework_404(anon_client.delete(f"{WEB}/pods/{segment}/"))


def test_web_runner_detail_uppercase_real_id_404s(user_client, anon_client, machine_flow):
    """Uppercase of a real runner id: the row exists and the caller may
    manage it, but the ``<uuid:>`` converter still 404s first."""
    segment = machine_flow["runner_id"].upper()
    _assert_framework_404(user_client.get(f"{WEB}/{segment}/"))
    _assert_framework_404(anon_client.get(f"{WEB}/{segment}/"))


def test_web_pod_detail_uppercase_real_id_404s(user_client, anon_client, daemon_world):
    segment = daemon_world["pod"]["id"].upper()
    _assert_framework_404(user_client.get(f"{WEB}/pods/{segment}/"))
    _assert_framework_404(anon_client.get(f"{WEB}/pods/{segment}/"))


# -- delete_cmds.rs: machine delete/create/status + daemon command result -----


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_machine_delete_non_canonical_404s(user_client, anon_client, segment):
    _assert_framework_404(web_delete(user_client, f"{WEB}/dev-machines/{segment}/"))
    _assert_framework_404(anon_client.delete(f"{WEB}/dev-machines/{segment}/"))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_machine_create_runner_non_canonical_404s(user_client, anon_client, segment):
    url = f"{WEB}/dev-machines/{segment}/create-runner/"
    _assert_framework_404(web_post(user_client, url, {"project": "x"}))
    _assert_framework_404(anon_client.post(url, json={"project": "x"}))


def _status_pairs():
    for bad in SEGMENTS:
        yield (bad, bad)
        yield (GOOD, bad)
        yield (bad, GOOD)


@pytest.mark.parametrize("mid,rid", list(_status_pairs()))
def test_web_machine_create_runner_status_non_canonical_404s(
    user_client, anon_client, mid, rid
):
    url = f"{WEB}/dev-machines/{mid}/create-runner/{rid}/"
    _assert_framework_404(user_client.get(url))
    _assert_framework_404(anon_client.get(url))


@pytest.mark.parametrize("mid,rid", list(_status_pairs()))
def test_daemon_command_result_non_canonical_404s(
    settings, machine_client, anon_client, mid, rid
):
    url = f"{DAEMON}/dev-machines/{mid}/commands/{rid}/result/"
    payload = {"status": "ok", "result": {}}
    _assert_framework_404(machine_client.post(url, json=payload))
    _assert_framework_404(anon_client.post(url, json=payload))


# -- teardown.rs: daemon refresh/self-revoke + web revoke/rotate -------------


@pytest.mark.parametrize("segment", SEGMENTS)
def test_daemon_refresh_non_canonical_404s(settings, enrolled, anon_client, segment):
    url = f"{DAEMON}/runners/{segment}/refresh/"
    with bearer_client(settings.base_url, enrolled["refresh_token"]) as client:
        _assert_framework_404(client.post(url))
    _assert_framework_404(anon_client.post(url))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_daemon_self_revoke_non_canonical_404s(runner_client, anon_client, segment):
    url = f"{DAEMON}/runners/{segment}/"
    _assert_framework_404(runner_client.delete(url))
    _assert_framework_404(anon_client.delete(url))


def test_daemon_refresh_uppercase_real_id_404s(settings, enrolled, anon_client):
    segment = enrolled["runner_id"].upper()
    url = f"{DAEMON}/runners/{segment}/refresh/"
    with bearer_client(settings.base_url, enrolled["refresh_token"]) as client:
        _assert_framework_404(client.post(url))
    _assert_framework_404(anon_client.post(url))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_machine_revoke_non_canonical_404s(user_client, anon_client, segment):
    url = f"{WEB}/dev-machines/{segment}/revoke/"
    _assert_framework_404(web_post(user_client, url, {}))
    _assert_framework_404(anon_client.post(url, json={}))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_machine_rotate_non_canonical_404s(user_client, anon_client, segment):
    url = f"{WEB}/dev-machines/{segment}/rotate/"
    _assert_framework_404(web_post(user_client, url, {}))
    _assert_framework_404(anon_client.post(url, json={}))


@pytest.mark.parametrize("segment", SEGMENTS)
def test_web_runner_revoke_non_canonical_404s(user_client, anon_client, segment):
    url = f"{WEB}/{segment}/revoke/"
    _assert_framework_404(web_post(user_client, url, {}))
    _assert_framework_404(anon_client.post(url, json={}))


def test_web_machine_revoke_uppercase_real_id_404s(
    user_client, anon_client, daemon_world, machine_flow
):
    response = user_client.get(
        f"{WEB}/dev-machines/", params={"workspace": daemon_world["workspace"]["id"]}
    )
    assert response.status_code == 200, response.text
    machine_id = next(
        machine["id"]
        for machine in response.json()
        if machine["host_label"] == machine_flow["host_label"]
    )
    url = f"{WEB}/dev-machines/{machine_id.upper()}/revoke/"
    _assert_framework_404(web_post(user_client, url, {}))
    _assert_framework_404(anon_client.post(url, json={}))
