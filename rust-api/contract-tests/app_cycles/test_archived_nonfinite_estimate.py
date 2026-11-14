"""NEWPIDASH-216: archived-cycle detail must not panic on a non-finite
estimate point value — it must answer the generic 500.

An estimate point ``value`` is a free text field (``estimate.py`` validates
length only), so ``NaN``/``inf``/``Infinity`` store fine, and Postgres
``CAST(value AS FLOAT)`` + ``SUM(...)`` happily produce non-finite floats.
Django raises ``ValueError`` while DRF renders the NaN and answers 500;
the Rust port used ``Number::from_f64(...).expect("finite")`` and aborted
the task, dropping the connection with no HTTP response
(``RemoteProtocolError``).

Body shape per backend: the ValueError fires during DRF *rendering*, which
runs after ``BaseViewSet.dispatch``'s try/except (``app/views/base.py``)
has already returned, so Django's answer is its HTML error page under
``pi_dash.settings.test`` (``DEBUG = True``) — status 500, ``text/html``,
never JSON. Rust answers its standard ``{"error": ...}`` envelope. The
tier-2 parity is the 500 status on both; the envelope is the fixed Rust
behaviour. Each arm is a pinned expectation: Rust must serve the exact
envelope, Django must serve the HTML page.
"""

import uuid
from datetime import datetime, timedelta, timezone

import pytest

from _harness.diffs import is_rust

from .conftest import cycle_urls

pytestmark = pytest.mark.contract

SERVER_ERROR = {"error": "Something went wrong please try again later"}


def _past_range(days_ago_start=40, days_ago_end=5):
    now = datetime.now(timezone.utc).date()
    return (
        (now - timedelta(days=days_ago_start)).isoformat(),
        (now - timedelta(days=days_ago_end)).isoformat(),
    )


def _seed_points_estimate(conn, *, project_id, workspace_id, value):
    """A ``points`` estimate with one point of the given raw value."""
    now = datetime.now(timezone.utc)
    est_id, ep_id = str(uuid.uuid4()), str(uuid.uuid4())
    with conn.cursor() as cur:
        cur.execute(
            "insert into estimates (id, name, description, project_id,"
            " workspace_id, type, last_used, created_at, updated_at)"
            " values (%s,%s,'',%s,%s,'points',false,%s,%s)",
            (est_id, f"Points {est_id[:8]}", project_id, workspace_id, now, now),
        )
        cur.execute(
            "insert into estimate_points (id, key, description, value,"
            " estimate_id, project_id, workspace_id, created_at, updated_at)"
            " values (%s,1,'',%s,%s,%s,%s,%s,%s)",
            (ep_id, value, est_id, project_id, workspace_id, now, now),
        )
        cur.execute(
            "update projects set estimate_id=%s where id=%s", (est_id, project_id)
        )
    return ep_id


def _seed_issue(
    conn, *, workspace_id, project_id, state_id, created_by_id, name,
    sequence_id, estimate_point_id,
):
    issue_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into issues (id, name, description_json, priority,"
            " sequence_id, project_id, workspace_id, state_id, description_html,"
            " sort_order, is_draft, git_work_branch, workpad, complexity_score,"
            " estimate_point_id, created_at, updated_at, created_by_id)"
            " values (%s,%s,'{}','high',%s,%s,%s,%s,'<p></p>',65535,false,'','',0,"
            " %s,%s,%s,%s)",
            (issue_id, name, sequence_id, project_id, workspace_id, state_id,
             estimate_point_id, now, now, created_by_id),
        )
    return issue_id


def _create_dated_cycle(
    conn, *, workspace_id, project_id, owner_id, name, start_date, end_date,
):
    cycle_id = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into cycles (id, name, description, start_date, end_date,"
            " owned_by_id, project_id, workspace_id, view_props, sort_order,"
            " external_source, external_id, progress_snapshot, archived_at,"
            " logo_props, timezone, version, created_by_id, created_at, updated_at)"
            " values (%s,%s,'',%s,%s,%s,%s,%s,'{}',65535,null,null,'{}',null,'{}',"
            " 'UTC',1,%s,%s,%s)",
            (cycle_id, name, start_date, end_date, owner_id, project_id,
             workspace_id, owner_id, now, now),
        )
    return cycle_id


def _add_to_cycle(conn, *, cycle_id, issue_id, project_id, workspace_id):
    now = datetime.now(timezone.utc)
    with conn.cursor() as cur:
        cur.execute(
            "insert into cycle_issues (id, cycle_id, issue_id, project_id,"
            " workspace_id, created_at, updated_at)"
            " values (%s,%s,%s,%s,%s,%s,%s)",
            (str(uuid.uuid4()), cycle_id, issue_id, project_id, workspace_id,
             now, now),
        )


@pytest.fixture()
def points_tenant(make_tenant):
    """A tenant whose own side rows are swept before ``make_tenant`` teardown.

    ``Seed.cleanup`` does not delete ``cycle_issues`` / ``issues`` /
    ``estimates`` / ``estimate_points`` (all FKs are NO ACTION and the
    connection autocommits), so clear ours by workspace first — otherwise
    the tracked project delete trips a live FK.
    """
    tenant = make_tenant()
    yield tenant
    ws = tenant["workspace"]["id"]
    with tenant["conn"].cursor() as cur:
        cur.execute(
            "update projects set estimate_id = null where workspace_id = %s", (ws,)
        )
        cur.execute("delete from cycle_issues where workspace_id = %s", (ws,))
        cur.execute("delete from issues where workspace_id = %s", (ws,))
        cur.execute("delete from estimate_points where workspace_id = %s", (ws,))
        cur.execute("delete from estimates where workspace_id = %s", (ws,))


@pytest.mark.parametrize("value", ["NaN", "inf", "Infinity"])
def test_archived_detail_nonfinite_estimate_answers_500(points_tenant, value):
    """A non-finite estimate point on an archived cycle answers 500, not a
    dropped connection — on both backends (Django oracle + Rust).

    The 500 status is the shared parity; the body is the Rust envelope on
    Rust and Django's HTML error page on Django (render-phase ValueError,
    see the module docstring).
    """
    tenant = points_tenant
    conn = tenant["conn"]
    ws = tenant["workspace"]["id"]
    proj = tenant["project"]["id"]
    state = tenant["state"]["id"]
    owner = tenant["user"]["id"]

    ep_id = _seed_points_estimate(conn, project_id=proj, workspace_id=ws, value=value)
    start, end = _past_range()
    cycle_id = _create_dated_cycle(
        conn, workspace_id=ws, project_id=proj, owner_id=owner,
        name="Non-finite estimate", start_date=start, end_date=end,
    )
    tenant["seed"].track("cycles", "id", cycle_id)
    issue = _seed_issue(
        conn, workspace_id=ws, project_id=proj, state_id=state,
        created_by_id=owner, name="Poisoned", sequence_id=9301,
        estimate_point_id=ep_id,
    )
    _add_to_cycle(conn, cycle_id=cycle_id, issue_id=issue,
                  project_id=proj, workspace_id=ws)

    urls = cycle_urls(points_tenant)
    r = points_tenant["client"].post(urls["archive"](cycle_id))
    assert r.status_code == 200, r.text[:500]

    r = points_tenant["client"].get(urls["archived_detail"](cycle_id))
    assert r.status_code == 500, r.text[:500]
    if is_rust():
        assert r.json() == SERVER_ERROR, r.text[:500]
    else:
        assert r.headers["content-type"].startswith("text/html"), (
            r.headers.get("content-type"), r.text[:300],
        )
