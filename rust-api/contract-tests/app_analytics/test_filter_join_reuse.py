"""Filter-join reuse on the workspace analytics plot (PIDASHCONV-1451).

``GET /api/workspaces/<slug>/analytics/`` builds the label, assignee, cycle
and module dimension (and the matching ``*_details`` extras) on the *same*
through-table join the ``issue_filters`` predicates already created: Django
runs ``Issue.issue_objects.filter(workspace__slug=slug, **filters)`` in one
call (``app/views/analytic/base.py:63``) and ``extract_axis`` annotates
``dimension=F(x_axis)`` afterwards (``utils/analytics_plot.py:60``), so the
annotation reuses the filter's join. A filter on a relation therefore also
constrains the dimension and the details.

Two observable consequences, both pinned here:

1. Assignees (the only relation whose details branch adds no explicit
   ``deleted_at`` guard): filtering by one assignee and plotting along
   ``assignees__id`` yields a single bucket and a single details row.
2. Labels, cycle and module: Django never gets that far — ``**filters``
   already contains ``<alias>__deleted_at__isnull`` (``filter_labels`` /
   ``filter_cycle`` / ``filter_module`` set it whenever the GET key is
   present) and the details branch passes the same kwarg explicitly again
   (``base.py:83,136,150``), so ``QuerySet.filter()`` raises ``TypeError:
   got multiple values for keyword argument`` and the endpoint answers
   500 ``{"error": "Something went wrong please try again later"}``. Any
   presence of the key — even ``?labels=`` — plus the matching axis or
   segment 500s. Rust ports that crash byte for byte (translate, don't
   redesign).

The fixture links everything onto the session world's issue I1 (no new
issues, so ``total`` assertions elsewhere are unaffected) and removes every
row it adds: labels LA/LB live plus a soft-deleted link to LC (a removed
label), assignees admin+member, cycles CN live + CO soft-deleted (an issue
moved out of its old cycle), modules M1+M2 live.

NOTE (adjacent, out of scope): ``assignee_details`` row *key order*
diverges even unfiltered — Django renders the ``avatar_url`` annotation
last, Rust first. Assertions below compare parsed JSON (order-insensitive),
so they pass on both backends despite that.
"""

import uuid
from datetime import datetime, timezone

import pytest

NOW = datetime.now(timezone.utc)
CRASH_BODY = {"error": "Something went wrong please try again later"}


@pytest.fixture()
def links(db_conn, seed):
    ids = {
        "la": str(uuid.uuid4()),
        "lb": str(uuid.uuid4()),
        "lc": str(uuid.uuid4()),
        "cn": str(uuid.uuid4()),
        "co": str(uuid.uuid4()),
        "m1": str(uuid.uuid4()),
        "m2": str(uuid.uuid4()),
    }
    issue = seed["issue1"]
    with db_conn.cursor() as cur:
        for key, name, color in (("la", "LA", "#aa0000"),
                                 ("lb", "LB", "#00aa00"),
                                 ("lc", "LC", "#0000aa")):
            cur.execute(
                "INSERT INTO labels (created_at, updated_at, id, name,"
                " description, created_by_id, project_id, updated_by_id,"
                " workspace_id, color, sort_order)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, ids[key], name, "", seed["admin"], seed["project"],
                 seed["admin"], seed["ws"], color, 0.0),
            )
        # LA + LB live; LC soft-deleted (label removed from the issue).
        for key, deleted in (("la", None), ("lb", None), ("lc", NOW)):
            cur.execute(
                "INSERT INTO issue_labels (created_at, updated_at, id, created_by_id,"
                " issue_id, label_id, project_id, updated_by_id, workspace_id,"
                " deleted_at) VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), seed["admin"], issue,
                 ids[key], seed["project"], seed["admin"], seed["ws"], deleted),
            )
        for user_key in ("admin", "member"):
            cur.execute(
                "INSERT INTO issue_assignees (created_at, updated_at, id, assignee_id,"
                " created_by_id, issue_id, project_id, updated_by_id, workspace_id)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), seed[user_key], seed["admin"],
                 issue, seed["project"], seed["admin"], seed["ws"]),
            )
        for key, name in (("cn", "CN new"), ("co", "CO old")):
            cur.execute(
                "INSERT INTO cycles (id, created_at, updated_at, name, description,"
                " start_date, end_date, owned_by_id, project_id, workspace_id,"
                " view_props, sort_order, progress_snapshot, archived_at,"
                " logo_props, timezone, version)"
                " VALUES (%s, now(), now(), %s, '',"
                " now() - interval '1 day', now() + interval '6 days',"
                " %s, %s, %s, '{}', 65535, '{}', NULL, '{}', 'UTC', 1)",
                (ids[key], name, seed["admin"], seed["project"], seed["ws"]),
            )
        for key, deleted in (("cn", None), ("co", NOW)):
            cur.execute(
                "INSERT INTO cycle_issues (id, created_at, updated_at, cycle_id,"
                " issue_id, project_id, workspace_id, deleted_at) VALUES"
                " (gen_random_uuid(), now(), now(), %s, %s, %s, %s, %s)",
                (ids[key], issue, seed["project"], seed["ws"], deleted),
            )
        for key, name in (("m1", "M1"), ("m2", "M2")):
            cur.execute(
                "INSERT INTO modules (id, created_at, updated_at, name, description,"
                " status, project_id, workspace_id,"
                " view_props, sort_order, archived_at, logo_props)"
                " VALUES (%s, now(), now(), %s, '', 'planned', %s, %s,"
                " '{}', 65535, NULL, '{}')",
                (ids[key], name, seed["project"], seed["ws"]),
            )
            cur.execute(
                "INSERT INTO module_issues (created_at, updated_at, id, created_by_id,"
                " issue_id, module_id, project_id, updated_by_id, workspace_id)"
                " VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)",
                (NOW, NOW, str(uuid.uuid4()), seed["admin"], issue,
                 ids[key], seed["project"], seed["admin"], seed["ws"]),
            )
    yield ids
    with db_conn.cursor() as cur:
        cur.execute(
            "DELETE FROM issue_labels WHERE label_id IN (%s,%s,%s)",
            (ids["la"], ids["lb"], ids["lc"]),
        )
        cur.execute(
            "DELETE FROM issue_assignees WHERE issue_id = %s", (issue,),
        )
        cur.execute(
            "DELETE FROM cycle_issues WHERE cycle_id IN (%s,%s)",
            (ids["cn"], ids["co"]),
        )
        cur.execute(
            "DELETE FROM module_issues WHERE module_id IN (%s,%s)",
            (ids["m1"], ids["m2"]),
        )
        cur.execute(
            "DELETE FROM labels WHERE id IN (%s,%s,%s)",
            (ids["la"], ids["lb"], ids["lc"]),
        )
        cur.execute(
            "DELETE FROM cycles WHERE id IN (%s,%s)", (ids["cn"], ids["co"]),
        )
        cur.execute(
            "DELETE FROM modules WHERE id IN (%s,%s)", (ids["m1"], ids["m2"]),
        )


def _detail_ids(body, details_key, id_key):
    rows = body["extras"][details_key]
    assert isinstance(rows, list)
    return sorted(row[id_key] for row in rows)


def test_assignee_filter_reuses_join_x_axis(clients, seed, links):
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?x_axis=assignees__id&y_axis=issue_count"
        "&assignees=%s" % (seed["ws_slug"], seed["member"])
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 1
    # Only the filtered assignee buckets; the second assignee on I1 and any
    # other link must not appear.
    assert sorted(body["distribution"]) == [seed["member"]]
    buckets = body["distribution"][seed["member"]]
    assert buckets == [{"dimension": seed["member"], "count": 1}]
    assert _detail_ids(body, "assignee_details", "assignees__id") == [seed["member"]]


def test_assignee_filter_reuses_join_segment(clients, seed, links):
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?x_axis=priority&y_axis=issue_count"
        "&segment=assignees__id&assignees=%s" % (seed["ws_slug"], seed["member"])
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 1
    assert sorted(body["distribution"]) == ["high"]
    buckets = body["distribution"]["high"]
    assert buckets == [
        {"dimension": "high", "segment": seed["member"], "count": 1}
    ]
    assert _detail_ids(body, "assignee_details", "assignees__id") == [seed["member"]]


def test_assignee_filter_reuses_join_estimate(clients, seed, links):
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?x_axis=assignees__id&y_axis=estimate"
        "&assignees=%s" % (seed["ws_slug"], seed["member"])
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 1
    assert sorted(body["distribution"]) == [seed["member"]]
    assert _detail_ids(body, "assignee_details", "assignees__id") == [seed["member"]]


@pytest.mark.parametrize("axis", ["x_axis", "segment"])
def test_label_filter_matches_django_500(clients, seed, links, axis):
    if axis == "x_axis":
        qs = "x_axis=labels__id&y_axis=issue_count&labels=%s" % links["la"]
    else:
        qs = ("x_axis=priority&y_axis=issue_count&segment=labels__id&labels=%s"
              % links["la"])
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?%s" % (seed["ws_slug"], qs)
    )
    assert resp.status_code == 500
    assert resp.json() == CRASH_BODY


@pytest.mark.parametrize("axis", ["x_axis", "segment"])
def test_cycle_filter_matches_django_500(clients, seed, links, axis):
    if axis == "x_axis":
        qs = "x_axis=issue_cycle__cycle_id&y_axis=issue_count&cycle=%s" % links["cn"]
    else:
        qs = ("x_axis=priority&y_axis=issue_count&segment=issue_cycle__cycle_id"
              "&cycle=%s" % links["cn"])
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?%s" % (seed["ws_slug"], qs)
    )
    assert resp.status_code == 500
    assert resp.json() == CRASH_BODY


@pytest.mark.parametrize("axis", ["x_axis", "segment"])
def test_module_filter_matches_django_500(clients, seed, links, axis):
    if axis == "x_axis":
        qs = "x_axis=issue_module__module_id&y_axis=issue_count&module=%s" % links["m1"]
    else:
        qs = ("x_axis=priority&y_axis=issue_count&segment=issue_module__module_id"
              "&module=%s" % links["m1"])
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?%s" % (seed["ws_slug"], qs)
    )
    assert resp.status_code == 500
    assert resp.json() == CRASH_BODY


def test_empty_label_filter_with_label_axis_matches_django_500(clients, seed, links):
    # Key presence alone triggers the duplicate kwarg (`filter_labels` sets
    # the guard unconditionally); the value never matters.
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?x_axis=labels__id&y_axis=issue_count&labels="
        % seed["ws_slug"]
    )
    assert resp.status_code == 500
    assert resp.json() == CRASH_BODY


@pytest.mark.parametrize(
    "param,value_key",
    [("labels", "la"), ("assignees", "member"), ("cycle", "cn"), ("module", "m1")],
)
def test_relation_filter_without_matching_axis_ok(clients, seed, links, param, value_key):
    # The details branch (and its crash) only runs for its own axis; a
    # relation filter with an unrelated axis is a plain 200 on both backends.
    value = links[value_key] if value_key in links else seed[value_key]
    resp = clients["admin"].get(
        "/api/workspaces/%s/analytics/?x_axis=priority&y_axis=issue_count&%s=%s"
        % (seed["ws_slug"], param, value)
    )
    assert resp.status_code == 200
    body = resp.json()
    assert body["total"] == 1
    assert sorted(body["distribution"]) == ["high"]
