"""Exporter path (``app/urls/exporter.py``): ``export-issues`` GET + POST.

POST enqueues ``issue_export_task`` via ``.delay()``; the suite runs the
server with ``AMQP_URL=memory://`` so the publish succeeds with no broker
and no worker — the 200 only promises acceptance, and the suite pins the
created ``exporters`` row (status ``queued``) through the GET list.
"""

import httpx


def _history(admin: httpx.Client, ws: str) -> list:
    resp = admin.get(f"/api/workspaces/{ws}/export-issues/?per_page=10&cursor=10:0:0")
    assert resp.status_code == 200
    return resp.json()


def test_export_history_list_shape(clients, seed):
    body = _history(clients["admin"], seed["ws_slug"])
    # >=: the POST test below adds a second row in the same session.
    assert body["total_count"] >= 1
    assert body["next_page_results"] is False
    assert body["prev_page_results"] is False
    rows = {r["id"]: r for r in body["results"]}
    assert seed["exporter"] in rows
    row = rows[seed["exporter"]]
    assert row["project"] == [seed["project"]]
    assert row["provider"] == "csv"
    assert row["status"] == "completed"
    assert row["url"] is None
    assert row["initiated_by"] == seed["admin"]
    assert row["token"]
    assert row["created_by"] is None
    assert row["updated_by"] is None
    assert row["initiated_by_detail"] == {
        "id": seed["admin"],
        "first_name": "an_admin",
        "last_name": "User",
        "avatar": "",
        "avatar_url": None,
        "is_bot": False,
        "display_name": "an_admin",
    }


def test_export_history_requires_pagination_params(clients, seed):
    resp = clients["admin"].get(f"/api/workspaces/{seed['ws_slug']}/export-issues/")
    assert resp.status_code == 400
    assert resp.json() == {"error": "per_page and cursor are required"}


def test_export_issues_post_creates_history(clients, seed):
    admin = clients["admin"]
    ws = seed["ws_slug"]
    resp = admin.post(
        f"/api/workspaces/{ws}/export-issues/",
        json={"provider": "csv", "project": [seed["project"]]},
    )
    assert resp.status_code == 200
    assert resp.json() == {
        "message": "Once the export is ready you will be able to download it"
    }

    body = _history(admin, ws)
    assert body["total_count"] == 2
    queued = [r for r in body["results"] if r["status"] == "queued"]
    assert len(queued) == 1
    assert queued[0]["provider"] == "csv"
    assert queued[0]["project"] == [seed["project"]]
    assert queued[0]["initiated_by"] == seed["admin"]
    assert queued[0]["token"]


def test_export_issues_post_bad_provider(clients, seed):
    resp = clients["admin"].post(
        f"/api/workspaces/{seed['ws_slug']}/export-issues/",
        json={"provider": "xml"},
    )
    assert resp.status_code == 400
    assert resp.json() == {"error": "Provider 'xml' not found."}
