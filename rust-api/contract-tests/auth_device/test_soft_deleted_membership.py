"""Soft-deleted memberships stay invisible to device auth reads (PIDASHCONV-1094).

Django scopes ``WorkspaceMember.objects`` / ``Workspace.objects`` through
``SoftDeletionManager`` (``deleted_at IS NULL``), so a membership row that
was soft-deleted with ``is_active`` still true (e.g. the workspace-deletion
cascade, which stamps ``deleted_at`` without touching ``is_active``):

(a) is hidden from ``GET /api/v1/auth/workspaces/``,
(b) fails ``is_workspace_member`` inside machine-token auth, which revokes
    the ``mt_`` token on sight and answers 403, and
(c) fails the membership check in ``POST /api/v1/auth/machine-token/``,
    which answers 404 ``workspace_not_found`` without minting.
"""

import uuid


def soft_delete_membership(db, workspace_id, user_id):
    """Stamp ``deleted_at`` on the membership, keeping ``is_active`` true."""
    with db.cursor() as cur:
        cur.execute(
            "UPDATE workspace_members SET deleted_at = now()"
            " WHERE workspace_id = %s AND member_id = %s",
            (str(workspace_id), str(user_id)),
        )
        assert cur.rowcount == 1
        cur.execute(
            "SELECT is_active, deleted_at FROM workspace_members"
            " WHERE workspace_id = %s AND member_id = %s",
            (str(workspace_id), str(user_id)),
        )
        is_active, deleted_at = cur.fetchone()
    assert is_active is True
    assert deleted_at is not None


def test_workspaces_hides_soft_deleted_membership(
    api, seed, tenant_a, api_key_a, db
):
    soft_delete_membership(
        db, tenant_a["workspace"]["id"], tenant_a["user"]["id"]
    )
    r = api.get(
        "/api/v1/auth/workspaces/", headers={"X-Api-Key": api_key_a}
    )
    assert r.status_code == 200, r.text
    assert r.json() == {"workspaces": []}


def test_machine_token_revoked_for_soft_deleted_membership(
    api, seed, tenant_a, secret, db
):
    from _harness import djangocrypto

    machine = seed.dev_machine(tenant_a["user"]["id"])
    raw = seed.machine_token(
        tenant_a["user"]["id"],
        tenant_a["workspace"]["id"],
        machine,
        secret,
    )
    soft_delete_membership(
        db, tenant_a["workspace"]["id"], tenant_a["user"]["id"]
    )
    r = api.get("/api/v1/auth/workspaces/", headers={"X-Api-Key": raw})
    assert r.status_code == 403, r.text
    assert r.json() == {"detail": "Given API token is not valid"}

    with db.cursor() as cur:
        cur.execute(
            "SELECT revoked_at, last_used_at FROM machine_token"
            " WHERE token_hash = %s",
            (djangocrypto.machine_token_hash(raw, secret),),
        )
        revoked_at, last_used_at = cur.fetchone()
    assert revoked_at is not None
    assert last_used_at is None


def test_machine_token_exchange_rejects_soft_deleted_membership(
    api, seed, tenant_a, api_key_a, db
):
    soft_delete_membership(
        db, tenant_a["workspace"]["id"], tenant_a["user"]["id"]
    )
    dev_machine_id = str(uuid.uuid4())
    r = api.post(
        "/api/v1/auth/machine-token/",
        headers={"X-Api-Key": api_key_a},
        json={
            "workspace_slug": tenant_a["workspace"]["slug"],
            "dev_machine_id": dev_machine_id,
            "host_label": "ct1094-host",
        },
    )
    assert r.status_code == 404, r.text
    assert r.json() == {"error": "workspace_not_found"}

    with db.cursor() as cur:
        cur.execute(
            "SELECT COUNT(*) FROM dev_machine WHERE id = %s",
            (dev_machine_id,),
        )
        (created,) = cur.fetchone()
    assert created == 0
