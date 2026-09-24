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

from conftest import module_url, modules_url, session_headers


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
