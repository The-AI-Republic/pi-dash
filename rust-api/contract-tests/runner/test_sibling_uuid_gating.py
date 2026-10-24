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
    # Framework 404s never pass a DRF view, so Django stamps no Allow;
    # the Rust proxy fallthrough must not add one either (PIDASHCONV-1048).
    assert "allow" not in response.headers


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


# -- daemon session routes (PIDASHCONV-1212) --------------------------------
# ``runner_sessions/machine.rs`` + ``runner_sessions/runner_poll.rs`` parsed
# the path UUIDs with ``Uuid::from_str`` (uppercase / hyphenless accepted)
# instead of the ``<uuid:>`` converter, so they served spellings Django
# 404s before any view runs. Each route must answer Django's framework 404
# for a non-canonical spelling and leave the session table untouched.


def _machine_id(seeder, machine_flow) -> str:
    """The dev machine bound to the flow's ``mt_`` token."""
    dev_machine_id = seeder.db.fetchval(
        "SELECT dev_machine_id FROM machine_token WHERE user_id = "
        "(SELECT owner_id FROM runner WHERE id = %s) AND host_label = %s",
        (machine_flow["runner_id"], machine_flow["host_label"]),
    )
    assert dev_machine_id, "machine token has no bound dev machine"
    return str(dev_machine_id)


def _non_canonical_spellings(value: str) -> list[str]:
    return [value.upper(), value.replace("-", "")]


def test_daemon_runner_session_poll_non_canonical_404s(
    machine_client, anon_client, machine_flow
):
    rid = machine_flow["runner_id"]
    opened = machine_client.post(f"{DAEMON}/runners/{rid}/sessions/", json={})
    assert opened.status_code == 201, opened.text
    sid = opened.json()["session_id"]
    for bad_rid in _non_canonical_spellings(rid):
        _assert_framework_404(machine_client.post(f"{DAEMON}/runners/{bad_rid}/sessions/{sid}/poll", json={}))
        _assert_framework_404(anon_client.post(f"{DAEMON}/runners/{bad_rid}/sessions/{sid}/poll", json={}))
    for bad_sid in _non_canonical_spellings(sid):
        _assert_framework_404(machine_client.post(f"{DAEMON}/runners/{rid}/sessions/{bad_sid}/poll", json={}))
        _assert_framework_404(anon_client.post(f"{DAEMON}/runners/{rid}/sessions/{bad_sid}/poll", json={}))
    machine_client.delete(f"{DAEMON}/runners/{rid}/sessions/{sid}/")


def test_daemon_machine_session_open_non_canonical_404s(
    settings, machine_flow, seeder, anon_client
):
    mid = _machine_id(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        for bad_mid in _non_canonical_spellings(mid):
            url = f"{DAEMON}/dev-machines/{bad_mid}/sessions/"
            _assert_framework_404(client.post(url, json={}))
            _assert_framework_404(anon_client.post(url, json={}))
    created = seeder.db.fetchval(
        "SELECT count(*) FROM machine_session WHERE dev_machine_id = %s", (mid,)
    )
    assert created == 0, "non-canonical open created a machine session"


def test_daemon_machine_session_delete_non_canonical_404s(
    settings, machine_flow, seeder, anon_client
):
    mid = _machine_id(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        opened = client.post(f"{DAEMON}/dev-machines/{mid}/sessions/", json={})
        assert opened.status_code == 201, opened.text
        sid = opened.json()["session_id"]
        for bad_mid in _non_canonical_spellings(mid):
            url = f"{DAEMON}/dev-machines/{bad_mid}/sessions/{sid}/"
            _assert_framework_404(client.delete(url))
            _assert_framework_404(anon_client.delete(url))
        for bad_sid in _non_canonical_spellings(sid):
            url = f"{DAEMON}/dev-machines/{mid}/sessions/{bad_sid}/"
            _assert_framework_404(client.delete(url))
            _assert_framework_404(anon_client.delete(url))
        revoked = seeder.db.fetchval(
            "SELECT revoked_at FROM machine_session WHERE id = %s", (sid,)
        )
        assert revoked is None, "non-canonical delete revoked the session"
        client.delete(f"{DAEMON}/dev-machines/{mid}/sessions/{sid}/")
    seeder.db.execute("DELETE FROM machine_session WHERE dev_machine_id = %s", (mid,))


def test_daemon_machine_session_poll_non_canonical_404s(
    settings, machine_flow, seeder, anon_client
):
    mid = _machine_id(seeder, machine_flow)
    with bearer_client(settings.base_url, machine_flow["machine_token"], timeout=40.0) as client:
        opened = client.post(f"{DAEMON}/dev-machines/{mid}/sessions/", json={})
        assert opened.status_code == 201, opened.text
        sid = opened.json()["session_id"]
        for bad_mid in _non_canonical_spellings(mid):
            url = f"{DAEMON}/dev-machines/{bad_mid}/sessions/{sid}/poll"
            _assert_framework_404(client.post(url, json={}))
            _assert_framework_404(anon_client.post(url, json={}))
        for bad_sid in _non_canonical_spellings(sid):
            url = f"{DAEMON}/dev-machines/{mid}/sessions/{bad_sid}/poll"
            _assert_framework_404(client.post(url, json={}))
            _assert_framework_404(anon_client.post(url, json={}))
        client.delete(f"{DAEMON}/dev-machines/{mid}/sessions/{sid}/")
    seeder.db.execute("DELETE FROM machine_session WHERE dev_machine_id = %s", (mid,))
