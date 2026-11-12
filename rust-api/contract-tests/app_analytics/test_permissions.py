"""Coverage floor: denied-permission + tenant-isolation for D-35.

Permission tripwires (the suite must go red if a gate is removed):

- removing ``@allow_permission([ADMIN, MEMBER])`` from any analytic or
  exporter endpoint flips the matching ``test_guest_denied_*`` case from
  403 to 200;
- removing ``WorkSpaceAdminPermission`` from ``AnalyticViewViewset``
  flips ``test_viewset_guest_forbidden`` from 403 to 201/200;
- removing the ``IsAuthenticated`` default flips every
  ``test_unauthenticated_is_401`` case from 401 to 200/403.

Role nuance pinned here: GUEST is allowed on ``default-analytics``,
``project-stats`` and the project chart endpoint, denied everywhere
else in this domain.
"""

import httpx

FORBIDDEN = {"error": "You don't have the required permissions."}
VIEWSET_FORBIDDEN = {"detail": "You do not have permission to perform this action."}
ANON = {"detail": "Authentication credentials were not provided."}

ANALYTICS_GETS = (
    "/analytics/?x_axis=priority&y_axis=issue_count",
    "/default-analytics/",
    "/project-stats/",
    "/advance-analytics/",
    "/advance-analytics/?tab=work-items",
    "/advance-analytics-stats/",
    "/advance-analytics-charts/",
)


def _anon(base_url: str) -> httpx.Client:
    return httpx.Client(base_url=base_url, timeout=10, follow_redirects=False)


def test_unauthenticated_is_401(base_url, seed):
    ws = seed["ws_slug"]
    pid = seed["project"]
    vid = seed["view"]
    with _anon(base_url) as anon:
        for path in ANALYTICS_GETS:
            resp = anon.get(f"/api/workspaces/{ws}{path}")
            assert resp.status_code == 401, path
            assert resp.json() == ANON, path
        for method, path in (
            ("get", f"/analytic-view/"),
            ("get", f"/analytic-view/{vid}/"),
            ("get", f"/saved-analytic-view/{vid}/"),
            ("post", f"/export-analytics/"),
            ("get", f"/projects/{pid}/advance-analytics/"),
            ("get", f"/projects/{pid}/advance-analytics-stats/"),
            ("get", f"/projects/{pid}/advance-analytics-charts/?type=work-items"),
            ("get", f"/export-issues/?per_page=10&cursor=10:0:0"),
            ("post", f"/export-issues/"),
        ):
            resp = anon.request(method, f"/api/workspaces/{ws}{path}")
            assert resp.status_code == 401, (method, path)
            assert resp.json() == ANON, (method, path)


def test_member_allowed_everywhere_readable(clients, seed):
    # MEMBER is on the allow-list of every ADMIN/MEMBER gate in the domain.
    member = clients["member"]
    ws = seed["ws_slug"]
    for path in ANALYTICS_GETS:
        assert member.get(f"/api/workspaces/{ws}{path}").status_code == 200, path


def test_guest_denied_admin_member_endpoints(clients, seed):
    guest = clients["guest"]
    ws = seed["ws_slug"]
    pid = seed["project"]
    vid = seed["view"]
    denied_gets = [
        f"/api/workspaces/{ws}/analytics/?x_axis=priority&y_axis=issue_count",
        f"/api/workspaces/{ws}/saved-analytic-view/{vid}/",
        f"/api/workspaces/{ws}/advance-analytics/",
        f"/api/workspaces/{ws}/advance-analytics/?tab=work-items",
        f"/api/workspaces/{ws}/advance-analytics-stats/",
        f"/api/workspaces/{ws}/advance-analytics-charts/",
        f"/api/workspaces/{ws}/projects/{pid}/advance-analytics/",
        f"/api/workspaces/{ws}/projects/{pid}/advance-analytics-stats/",
        f"/api/workspaces/{ws}/export-issues/?per_page=10&cursor=10:0:0",
    ]
    for url in denied_gets:
        resp = guest.get(url)
        assert resp.status_code == 403, url
        assert resp.json() == FORBIDDEN, url
    for url, payload in (
        (f"/api/workspaces/{ws}/export-analytics/",
         {"x_axis": "priority", "y_axis": "issue_count"}),
        (f"/api/workspaces/{ws}/export-issues/", {"provider": "csv"}),
    ):
        resp = guest.post(url, json=payload)
        assert resp.status_code == 403, url
        assert resp.json() == FORBIDDEN, url


def test_guest_allowed_read_endpoints(clients, seed):
    guest = clients["guest"]
    ws = seed["ws_slug"]
    pid = seed["project"]
    for url in (
        f"/api/workspaces/{ws}/default-analytics/",
        f"/api/workspaces/{ws}/project-stats/",
        f"/api/workspaces/{ws}/projects/{pid}/advance-analytics-charts/?type=work-items",
    ):
        resp = guest.get(url)
        assert resp.status_code == 200, url


def test_viewset_guest_forbidden(clients, seed):
    guest = clients["guest"]
    ws = seed["ws_slug"]
    vid = seed["view"]
    resp = guest.post(
        f"/api/workspaces/{ws}/analytic-view/",
        json={"name": "GX", "description": "", "query_dict": {}},
    )
    assert resp.status_code == 403
    assert resp.json() == VIEWSET_FORBIDDEN
    resp = guest.get(f"/api/workspaces/{ws}/analytic-view/{vid}/")
    assert resp.status_code == 403
    assert resp.json() == VIEWSET_FORBIDDEN


def test_tenant_isolation(clients, seed):
    outsider = clients["outsider"]
    admin = clients["admin"]
    ws = seed["ws_slug"]
    other = seed["other_slug"]
    vid = seed["view"]
    # A member of the other tenant sees nothing of the primary workspace.
    for url in (
        f"/api/workspaces/{ws}/analytics/?x_axis=priority&y_axis=issue_count",
        f"/api/workspaces/{ws}/default-analytics/",
        f"/api/workspaces/{ws}/advance-analytics/",
        f"/api/workspaces/{ws}/analytic-view/{vid}/",
        f"/api/workspaces/{ws}/export-issues/?per_page=10&cursor=10:0:0",
    ):
        resp = outsider.get(url)
        assert resp.status_code == 403, url
    # And the mirror: primary members see nothing of the other workspace.
    for url in (
        f"/api/workspaces/{other}/default-analytics/",
        f"/api/workspaces/{other}/analytics/?x_axis=priority&y_axis=issue_count",
        f"/api/workspaces/{other}/analytic-view/{vid}/",
    ):
        resp = admin.get(url)
        assert resp.status_code == 403, url


def test_tenant_positive_control(clients, seed):
    # The other tenant's own data is visible to its own admin: isolation
    # denies cross-tenant reads, not all reads.
    resp = clients["outsider"].get(
        f"/api/workspaces/{seed['other_slug']}/default-analytics/"
    )
    assert resp.status_code == 200
    assert resp.json()["total_issues"] == 1
    workitems = clients["outsider"].get(
        f"/api/workspaces/{seed['other_slug']}/advance-analytics/?tab=work-items"
    )
    assert workitems.status_code == 200
    assert workitems.json()["total_work_items"] == {"count": 1}
