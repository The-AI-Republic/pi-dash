"""Coverage floor: denied-permission + tenant-isolation for D-26.

Permission tripwires (the suite must go red if a gate is removed):

- removing ``GUEST`` from ``IssueListEndpoint.get`` flips
  ``test_guest_allowed_reads`` from 200 to 403 (demonstrated in the PR
  with a one-line patch, reverted);
- removing ``@allow_permission([ROLE.ADMIN, ROLE.MEMBER])`` from
  ``IssueViewSet.create`` flips ``test_guest_denied_writes`` from 403
  to 201;
- removing ``ProjectEntityPermission`` from ``IssueRelationViewSet``
  flips ``test_guest_denied_entity_writes`` from 403 to 201;
- removing the ``IsAuthenticated`` default flips every
  ``test_unauthenticated_is_401`` case from 401 to 200/403.

Role nuance pinned here: GUEST reads everything in this domain but
writes only comments, reactions and subscriptions; label writes are
ADMIN-only with the class (``detail`` body) and decorator (``error``
body) denying at different layers; the creator bypasses the role gate
for comment/issue deletes.
"""

import httpx

FORBIDDEN = {"error": "You don't have the required permissions."}
VIEWSET_FORBIDDEN = {"detail": "You do not have permission to perform this action."}
ANON = {"detail": "Authentication credentials were not provided."}


def _base(ws, pid):
    return f"/api/workspaces/{ws}/projects/{pid}"


def _anon(base_url: str) -> httpx.Client:
    return httpx.Client(base_url=base_url, timeout=10, follow_redirects=False)


def test_unauthenticated_is_401(base_url, seed):
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    c1 = seed["comment1"]
    with _anon(base_url) as anon:
        for method, path in (
            ("get", f"{_base(ws, pid)}/issues/?group_by=state"),
            ("get", f"{_base(ws, pid)}/issues/list/?issues={i1}"),
            ("post", f"{_base(ws, pid)}/issues/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/"),
            ("get", f"{_base(ws, pid)}/issues-detail/"),
            ("get", f"{_base(ws, pid)}/v2/issues/"),
            ("get", f"{_base(ws, pid)}/issue-labels/"),
            ("get", f"{_base(ws, pid)}/archived-issues/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/archive/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/sub-issues/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/issue-relation/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/issue-links/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/history/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/comments/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/comments/{c1}/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/reactions/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/subscribe/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/issue-attachments/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/versions/"),
            ("get", f"{_base(ws, pid)}/issues/{i1}/meta/"),
            ("get", f"/api/workspaces/{ws}/work-items/IS-1/"),
            ("get", f"{_base(ws, pid)}/deleted-issues/"),
            ("get", f"{_base(ws, pid)}/user-properties/"),
            ("post", f"{_base(ws, pid)}/work-items/{i1}/move/"),
        ):
            resp = anon.request(method, path)
            assert resp.status_code == 401, (method, path)
            assert resp.json() == ANON, (method, path)


def test_member_allowed_reads(clients, seed):
    member = clients["member"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    for url in (
        f"{_base(ws, pid)}/issues/list/?issues={i1}",
        f"{_base(ws, pid)}/issues/",
        f"{_base(ws, pid)}/issues/{i1}/",
        f"{_base(ws, pid)}/issues-detail/",
        f"{_base(ws, pid)}/v2/issues/",
        f"{_base(ws, pid)}/archived-issues/",
        f"{_base(ws, pid)}/issues/{i1}/sub-issues/",
        f"{_base(ws, pid)}/issues/{i1}/issue-relation/",
        f"{_base(ws, pid)}/issues/{i1}/history/?activity_type=issue-comment",
        f"{_base(ws, pid)}/issues/{i1}/versions/",
        f"{_base(ws, pid)}/issues/{i1}/meta/",
    ):
        assert member.get(url).status_code == 200, url


def test_guest_allowed_reads(clients, seed):
    guest = clients["guest"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    # IS has guest_view_all_features on: guests read everything.
    for url in (
        f"{_base(ws, pid)}/issues/list/?issues={i1}",
        f"{_base(ws, pid)}/issues/",
        f"{_base(ws, pid)}/issues/{i1}/",
        f"{_base(ws, pid)}/issues-detail/",
        f"{_base(ws, pid)}/v2/issues/",
        f"{_base(ws, pid)}/issues/{i1}/meta/",
        f"/api/workspaces/{ws}/work-items/IS-1/",
    ):
        assert guest.get(url).status_code == 200, url


def test_guest_denied_writes(clients, seed):
    guest = clients["guest"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    for method, url, payload, body in (
        ("post", f"{_base(ws, pid)}/issues/", {"name": "G"}, FORBIDDEN),
        ("post", f"{_base(ws, pid)}/issue-dates/", {"updates": []},
         FORBIDDEN),
        ("post", f"{_base(ws, pid)}/work-items/{i1}/move/",
         {"project": "IS"}, FORBIDDEN),
        ("get", f"{_base(ws, pid)}/archived-issues/", None, FORBIDDEN),
        # Bulk-archive also carries ProjectEntityPermission, which denies
        # first with the viewset body.
        ("post", f"{_base(ws, pid)}/bulk-archive-issues/",
         {"issue_ids": [i1]}, VIEWSET_FORBIDDEN),
        ("delete", f"{_base(ws, pid)}/issues/{i1}/", None, FORBIDDEN),
        ("patch", f"{_base(ws, pid)}/issues/{i1}/",
         {"priority": "low"}, FORBIDDEN),
    ):
        if method == "delete":
            resp = guest.request("DELETE", url)
        elif method == "patch":
            resp = guest.patch(url, json=payload)
        elif method == "get":
            resp = guest.get(url)
        else:
            resp = guest.post(url, json=payload)
        assert resp.status_code == 403, (method, url)
        assert resp.json() == body, (method, url)


def test_guest_denied_entity_writes(clients, seed):
    # ``ProjectEntityPermission`` gates unsafe methods to ADMIN/MEMBER;
    # guests get the viewset ``detail`` body.
    guest = clients["guest"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    for method, url, payload in (
        ("post", f"{_base(ws, pid)}/issues/{i1}/sub-issues/",
         {"sub_issue_ids": [i1]}),
        ("post", f"{_base(ws, pid)}/issues/{i1}/issue-relation/",
         {"relation_type": "relates_to", "issues": [i1]}),
        ("post", f"{_base(ws, pid)}/issues/{i1}/issue-links/",
         {"title": "g", "url": "https://example.com/g"}),
    ):
        resp = guest.post(url, json=payload)
        assert resp.status_code == 403, (method, url)
        assert resp.json() == VIEWSET_FORBIDDEN, (method, url)


def test_guest_label_create_denied_at_class(clients, seed):
    # Guests fail the workspace ADMIN/MEMBER class check first.
    guest = clients["guest"]
    ws, pid = seed["ws_slug"], seed["project"]
    resp = guest.post(
        f"{_base(ws, pid)}/issue-labels/",
        json={"name": "L-guest", "color": "#123456"})
    assert resp.status_code == 403
    assert resp.json() == VIEWSET_FORBIDDEN


def test_guest_can_comment_react_subscribe(clients, seed):
    guest = clients["guest"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    created = guest.post(
        f"{_base(ws, pid)}/issues/{i1}/comments/",
        json={"comment_html": "<p>g</p>", "comment_stripped": "g"},
    )
    assert created.status_code == 201
    comment_id = created.json()["id"]
    try:
        assert guest.post(
            f"{_base(ws, pid)}/issues/{i1}/reactions/",
            json={"reaction": "eyes"},
        ).status_code == 201
        assert guest.delete(
            f"{_base(ws, pid)}/issues/{i1}/reactions/eyes/",
        ).status_code == 204
        assert guest.post(
            f"{_base(ws, pid)}/issues/{i1}/subscribe/").status_code == 201
        assert guest.get(
            f"{_base(ws, pid)}/issues/{i1}/subscribe/").json() == {
                "subscribed": True}
        assert guest.delete(
            f"{_base(ws, pid)}/issues/{i1}/subscribe/").status_code == 204
    finally:
        guest.delete(f"{_base(ws, pid)}/issues/{i1}/comments/{comment_id}/")


def test_guest_scoping_without_view_all(clients, seed):
    # IS2 has guest_view_all_features off: guests see only their own.
    admin, guest = clients["admin"], clients["guest"]
    ws, p2, j1, j2 = (
        seed["ws_slug"], seed["project2"], seed["j1"], seed["j2"])
    resp = guest.get(f"{_base(ws, p2)}/issues/{j1}/")
    assert resp.status_code == 403
    assert resp.json() == {"error": "You are not allowed to view this issue"}
    assert guest.get(f"{_base(ws, p2)}/issues/{j2}/").status_code == 200
    body = guest.get(f"{_base(ws, p2)}/issues/").json()
    assert {row["id"] for row in body["results"]} == {j2}
    assert admin.get(f"{_base(ws, p2)}/issues/{j1}/").status_code == 200


def test_tenant_isolation(clients, seed):
    outsider, admin = clients["outsider"], clients["admin"]
    ws, pid, i1 = seed["ws_slug"], seed["project"], seed["issue1"]
    other, opid, x1 = (
        seed["other_slug"], seed["other_project"], seed["other_issue"])
    # Decorator-gated endpoints reject with the error body.
    for url in (
        f"{_base(ws, pid)}/issues/list/?issues={i1}",
        f"{_base(ws, pid)}/issues/",
        f"{_base(ws, pid)}/issues/{i1}/",
        f"{_base(ws, pid)}/archived-issues/",
        f"{_base(ws, pid)}/issues/{i1}/meta/",
    ):
        resp = outsider.get(url)
        assert resp.status_code == 403, url
        assert resp.json() == FORBIDDEN, url
    # Class-gated endpoints reject with the detail body.
    for url in (
        f"{_base(ws, pid)}/issues/{i1}/issue-relation/",
        f"{_base(ws, pid)}/issues/{i1}/sub-issues/",
        f"{_base(ws, pid)}/issue-labels/",
    ):
        resp = outsider.get(url)
        assert resp.status_code == 403, url
        assert resp.json() == VIEWSET_FORBIDDEN, url
    # Queryset-scoped endpoints return empty instead of leaking.
    assert outsider.get(
        f"{_base(ws, pid)}/issues/{i1}/comments/").json() == []
    # And the mirror: primary members see nothing of the other tenant.
    for url in (
        f"{_base(other, opid)}/issues/",
        f"{_base(other, opid)}/issues/{x1}/",
    ):
        resp = admin.get(url)
        assert resp.status_code == 403, url


def test_tenant_positive_control(clients, seed):
    # The other tenant's own data is visible to its own admin.
    resp = clients["outsider"].get(
        f"{_base(seed['other_slug'], seed['other_project'])}/issues/")
    assert resp.status_code == 200
    body = resp.json()
    assert body["total_count"] == 1
    assert body["results"][0]["name"] == "Foreign issue"
    assert clients["outsider"].get(
        f"{_base(seed['other_slug'], seed['other_project'])}/issues/"
        f"{seed['other_issue']}/").status_code == 200
