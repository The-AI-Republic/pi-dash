"""Permission + tenancy floor (PIDASHCONV-86, D-28).

Coverage floor for the domain: one denied-permission case, one
tenant-isolation case, and an unauthenticated case. The suite's
sensitivity to permission removal is demonstrated in the PR (a one-line
local patch removing a permission class turns these denials into 200s
and fails the suite); the cases below are the tripwires:

- test_create_denied_without_project_membership: removing the
  PROJECT-level role gate (allow_permission on create) flips this 403
  to 201.
- test_list_isolated_between_tenants: removing workspace scoping flips
  this 403 to 200 with leaked rows.
"""

import os

from _harness.diffs import expect, is_rust

from .conftest import module_url, modules_url, session_headers


def test_create_denied_without_project_membership(api, seed, tenant_a, project_a):
    """Workspace member with no project membership hits the role gate."""
    outsider = seed.user()
    seed.member(tenant_a["workspace"]["id"], outsider["id"], role=15)
    headers = session_headers(
        seed, outsider, outsider["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.post(
        modules_url(tenant_a, project_a), headers=headers,
        json={"name": "Nope"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_create_denied_for_guest(api, seed, tenant_a, project_a):
    """Guests may list (ADMIN/MEMBER/GUEST) but not create (ADMIN/MEMBER)."""
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    seed.project_member(project_a, tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.post(
        modules_url(tenant_a, project_a), headers=headers,
        json={"name": "Guest module"},
    )
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_guest_may_list(api, seed, tenant_a, project_a, auth_a, module_a):
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    seed.project_member(project_a, tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    r = api.get(modules_url(tenant_a, project_a), headers=headers)
    assert r.status_code == 200, r.text
    assert any(m["id"] == module_a for m in r.json())


def test_list_isolated_between_tenants(api, seed, tenant_a, tenant_b, project_a,
                                       auth_b, module_a):
    """Tenant B (no membership in A's workspace) cannot list A's modules."""
    r = api.get(modules_url(tenant_a, project_a), headers=auth_b)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_retrieve_isolated_between_tenants(api, tenant_a, project_a, auth_b, module_a):
    r = api.get(module_url(tenant_a, project_a, module_a), headers=auth_b)
    assert r.status_code == 403, r.text
    assert r.json() == {"error": "You don't have the required permissions."}


def test_unauthenticated_denied(api, tenant_a, project_a, module_a):
    for method, url, kwargs in [
        ("get", modules_url(tenant_a, project_a), {}),
        ("post", modules_url(tenant_a, project_a), {"json": {"name": "x"}}),
        ("get", module_url(tenant_a, project_a, module_a), {}),
        ("delete", module_url(tenant_a, project_a, module_a), {}),
    ]:
        r = getattr(api, method)(url, **kwargs)
        assert r.status_code == 401, (method, url, r.text)


# ---------------------------------------------------------------------------
# NEWPIDASH-281 (DIFF-51): PUT modules/<pk>/ gated like PATCH
# ---------------------------------------------------------------------------
# Harness: expect("DIFF-51", django=..., rust=...) branches on
# PIDASH_CONTRACT_BACKEND (django default, rust for the Rust run).
# Django arm pins the old IsAuthenticated-only 200; Rust arm pins the
# fixed 403/400. Every case still fails if either backend drifts.

FORBIDDEN = {"error": "You don't have the required permissions."}
ARCHIVED = {"error": "Archived module cannot be updated"}


def _module_name(api, tenant, project_id, module_id, headers):
    r = api.get(module_url(tenant, project_id, module_id), headers=headers)
    assert r.status_code == 200, r.text
    return r.json()["name"]


def test_put_denied_for_guest(api, seed, tenant_a, project_a, auth_a, module_a):
    guest = seed.user()
    seed.member(tenant_a["workspace"]["id"], guest["id"], role=5)
    seed.project_member(project_a, tenant_a["workspace"]["id"], guest["id"], role=5)
    headers = session_headers(
        seed, guest, guest["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    before = _module_name(api, tenant_a, project_a, module_a, auth_a)
    r = api.put(
        module_url(tenant_a, project_a, module_a), headers=headers,
        json={"name": "Should Not Stick"},
    )
    assert r.status_code == expect("DIFF-51", django=200, rust=403), r.text
    if is_rust():
        assert r.json() == FORBIDDEN, r.text
        assert _module_name(api, tenant_a, project_a, module_a, auth_a) == before
    else:
        assert r.json()["name"] == "Should Not Stick"


def test_put_denied_for_non_member(api, seed, tenant_a, project_a, auth_a, module_a):
    outsider = seed.user()
    seed.member(tenant_a["workspace"]["id"], outsider["id"], role=15)
    headers = session_headers(
        seed, outsider, outsider["password"], os.environ["CONTRACT_SECRET_KEY"]
    )
    before = _module_name(api, tenant_a, project_a, module_a, auth_a)
    r = api.put(
        module_url(tenant_a, project_a, module_a), headers=headers,
        json={"name": "Should Not Stick"},
    )
    assert r.status_code == expect("DIFF-51", django=200, rust=403), r.text
    if is_rust():
        assert r.json() == FORBIDDEN, r.text
        assert _module_name(api, tenant_a, project_a, module_a, auth_a) == before
    else:
        assert r.json()["name"] == "Should Not Stick"


def test_put_archived_rejected(api, seed, db, tenant_a, project_a, auth_a):
    mid = seed.module(
        tenant_a["workspace"]["id"], project_a, name="Frozen Put",
        status="completed",
    )
    archive_url = f"{modules_url(tenant_a, project_a)}{mid}/archive/"
    r = api.post(archive_url, headers=auth_a)
    assert r.status_code == 200, r.text
    with db.cursor() as cur:
        cur.execute(
            "SELECT name FROM modules WHERE id = %s", (mid,),
        )
        before = cur.fetchone()[0]
    r = api.put(
        module_url(tenant_a, project_a, mid), headers=auth_a,
        json={"name": "Should Not Stick"},
    )
    assert r.status_code == expect("DIFF-51", django=200, rust=400), r.text
    if is_rust():
        assert r.json() == ARCHIVED, r.text
        with db.cursor() as cur:
            cur.execute(
                "SELECT name FROM modules WHERE id = %s", (mid,),
            )
            assert cur.fetchone()[0] == before
    else:
        assert r.json()["name"] == "Should Not Stick"
