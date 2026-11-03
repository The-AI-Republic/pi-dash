"""POST /api/v1/auth/machine-token/ — API token becomes a dev-machine token."""

import uuid

from .conftest import approve, device_start, poll_token

CLI_DESCRIPTION = "Issued by pidash auth login (device-code flow)."


def _cli_api_token(api, seed, tenant, secret):
    """Run the full device flow; return the minted APIToken string."""
    codes = device_start(api).json()
    cookies = seed.session_cookie(
        tenant["user"]["id"], tenant["user"]["password"], secret
    )
    assert approve(api, cookies, codes["user_code"]).status_code == 200
    r = poll_token(api, codes["device_code"])
    assert r.status_code == 200, r.text
    return r.json()["access_token"]


def _exchange(api, token, **fields):
    return api.post(
        "/api/v1/auth/machine-token/",
        json=fields,
        headers={"X-Api-Key": token},
    )


import pytest as _pytest


@_pytest.fixture(scope="module")
def shared_probe(api):
    """One device-flow token shared by the tests that never consume it.

    Keeps the suite under the 20/minute device-start throttle: only the
    shape and rotation tests mint their own token.
    """
    import sys
    from pathlib import Path

    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from _harness import env
    from _harness.seed import Seed

    conn = env.connect()
    seed = Seed(conn)
    tenant = seed.tenant()
    token = _cli_api_token(api, seed, tenant, env.contract_secret())
    yield tenant, token
    seed.cleanup()
    conn.close()


def test_machine_token_shape(api, seed, tenant_a, secret, db):
    api_token = _cli_api_token(api, seed, tenant_a, secret)
    dev_machine_id = str(uuid.uuid4())
    r = _exchange(
        api,
        api_token,
        workspace_slug=tenant_a["workspace"]["slug"],
        dev_machine_id=dev_machine_id,
        host_label="ct80-macbook",
    )
    assert r.status_code == 201, r.text
    body = r.json()
    assert set(body) == {
        "machine_token",
        "workspace_slug",
        "dev_machine_id",
        "host_label",
    }
    assert body["machine_token"].startswith("mt_")
    assert body["workspace_slug"] == tenant_a["workspace"]["slug"]
    assert body["dev_machine_id"] == dev_machine_id
    assert body["host_label"] == "ct80-macbook"

    with db.cursor() as cur:
        cur.execute(
            "SELECT is_active FROM api_tokens WHERE token = %s", (api_token,)
        )
        (still_active,) = cur.fetchone()
    assert still_active is False, "device APIToken must be deactivated"

    # The machine token authenticates as the user (wire compatibility).
    workspaces = api.get(
        "/api/v1/auth/workspaces/",
        headers={"X-Api-Key": body["machine_token"]},
    )
    assert workspaces.status_code == 200, workspaces.text


def test_machine_token_requires_auth(api, shared_probe):
    tenant, _ = shared_probe
    r = _exchange(
        api,
        "bogus",
        workspace_slug=tenant["workspace"]["slug"],
        dev_machine_id=str(uuid.uuid4()),
        host_label="x",
    )
    assert r.status_code == 403, r.text


def test_machine_token_validation(api, shared_probe):
    tenant, api_token = shared_probe
    base = {
        "workspace_slug": tenant["workspace"]["slug"],
        "dev_machine_id": str(uuid.uuid4()),
        "host_label": "ct80-host",
    }
    for drop in ("workspace_slug", "dev_machine_id", "host_label"):
        fields = {k: v for k, v in base.items() if k != drop}
        r = _exchange(api, api_token, **fields)
        assert r.status_code == 400, (drop, r.text)
    r = _exchange(
        api, api_token, workspace_slug=base["workspace_slug"],
        dev_machine_id="not-a-uuid", host_label="ct80-host",
    )
    assert r.status_code == 400, r.text
    assert r.json() == {"error": "invalid_dev_machine_id"}


def test_machine_token_unknown_workspace(api, shared_probe):
    tenant, api_token = shared_probe
    r = _exchange(
        api,
        api_token,
        workspace_slug="no-such-workspace",
        dev_machine_id=str(uuid.uuid4()),
        host_label="ct80-host",
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "workspace_not_found"}


def test_machine_token_other_workspace_isolation(
    api, shared_probe, tenant_b
):
    """A caller who is not a member of the target workspace gets 404."""
    tenant, api_token = shared_probe
    r = _exchange(
        api,
        api_token,
        workspace_slug=tenant_b["workspace"]["slug"],
        dev_machine_id=str(uuid.uuid4()),
        host_label="ct80-host",
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "workspace_not_found"}


def test_machine_token_other_machine_isolation(
    api, seed, shared_probe, tenant_b
):
    """A dev machine owned by another user cannot be claimed."""
    other_machine = seed.dev_machine(tenant_b["user"]["id"])
    tenant, api_token = shared_probe
    r = _exchange(
        api,
        api_token,
        workspace_slug=tenant["workspace"]["slug"],
        dev_machine_id=other_machine,
        host_label="ct80-host",
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "dev_machine_not_found"}


def test_machine_token_rotation(api, seed, tenant_a, secret, db):
    api_token = _cli_api_token(api, seed, tenant_a, secret)
    dev_machine_id = str(uuid.uuid4())
    first = _exchange(
        api,
        api_token,
        workspace_slug=tenant_a["workspace"]["slug"],
        dev_machine_id=dev_machine_id,
        host_label="ct80-host",
    )
    assert first.status_code == 201, first.text
    # A plain (non-device) API token survives the exchange, so it can
    # rotate again on the same machine.
    plain = seed.api_token(
        tenant_a["user"]["id"], tenant_a["workspace"]["id"]
    )
    second = _exchange(
        api,
        plain,
        workspace_slug=tenant_a["workspace"]["slug"],
        dev_machine_id=dev_machine_id,
        host_label="ct80-host",
    )
    assert second.status_code == 201, second.text
    assert second.json()["machine_token"] != first.json()["machine_token"]

    from _harness import djangocrypto

    with db.cursor() as cur:
        cur.execute(
            "SELECT revoked_at FROM machine_token WHERE token_hash = %s",
            (
                djangocrypto.machine_token_hash(
                    first.json()["machine_token"], secret
                ),
            ),
        )
        (revoked_at,) = cur.fetchone()
    assert revoked_at is not None, "rotation must revoke the previous token"
