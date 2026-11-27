"""Contract: extreme page cursors must not overflow the intake paginators.

NEWPIDASH-312 ("Bug:"). The intake-issues list and the
description-versions list did unchecked i64 arithmetic on the page taken
from `cursor`: near the i64 limits a debug build panicked (the client saw
the connection dropped with no response) while a release build wrapped
(a wrong `next_cursor`, a wrong `next_page_results`, or a negative
OFFSET answering 500). Django uses unbounded ints and answers 200 with
an empty page, so the Rust port must do the same.
"""

import pytest

from .conftest import intake_urls
from .test_intake import create_intake_issue

pytestmark = pytest.mark.contract

MAX_I64 = 2**63 - 1
MIN_I64 = -(2**63)


def test_intake_issues_extreme_page_cursor(make_tenant):
    """`cursor=1:MAX:0` answers 200 with an empty page, not a panic."""
    tenant = make_tenant()
    urls = intake_urls(tenant)
    create_intake_issue(tenant["client"], urls["intake_issues"])
    r = tenant["client"].get(
        urls["intake_issues"],
        params={"status": "-2", "per_page": 1, "cursor": f"1:{MAX_I64}:0"},
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["results"] == []
    assert body["count"] == 0
    assert body["next_page_results"] is False
    # Django's unbounded ints keep counting past i64::MAX.
    assert body["next_cursor"] == f"1:{MAX_I64 + 1}:0"
    assert body["prev_cursor"] == f"1:{MAX_I64 - 1}:1"
    assert body["prev_page_results"] is True


def test_intake_issues_extreme_negative_page_cursor(make_tenant):
    """`cursor=1:MIN:0` answers 400 ("Error in parsing"), not a panic."""
    tenant = make_tenant()
    urls = intake_urls(tenant)
    create_intake_issue(tenant["client"], urls["intake_issues"])
    r = tenant["client"].get(
        urls["intake_issues"],
        params={"status": "-2", "per_page": 1, "cursor": f"1:{MIN_I64}:0"},
    )
    assert r.status_code == 400, r.text[:500]


def test_versions_extreme_positive_page_cursor(make_tenant):
    """`cursor=1000:MAX:0` answers 200 with an empty page, not a panic."""
    tenant = make_tenant()
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    r = tenant["client"].get(
        urls["versions"](created["issue"]["id"]),
        params={"cursor": f"1000:{MAX_I64}:0"},
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["results"] == []
    assert body["page_count"] == 0
    assert body["next_cursor"] is None
    assert body["next_page_results"] is False
    assert body["prev_cursor"] == f"1000:{MAX_I64 - 1}:0"
    assert body["prev_page_results"] is True


def test_versions_extreme_positive_page_cursor_minus_one(make_tenant):
    """`cursor=1000:(MAX-1):0` answers 200 with an empty page, not a panic."""
    tenant = make_tenant()
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    r = tenant["client"].get(
        urls["versions"](created["issue"]["id"]),
        params={"cursor": f"1000:{MAX_I64 - 1}:0"},
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["results"] == []
    assert body["next_cursor"] is None
    assert body["next_page_results"] is False


def test_versions_extreme_negative_page_cursor(make_tenant):
    """A negative page reads from the start, exactly like Django's
    `global_paginator.paginate` (`start_index` stays 0 unless
    `current_page > 0`): the extreme negative cursor answers 200 and its
    page content matches the unpaginated first page.
    """
    tenant = make_tenant()
    urls = intake_urls(tenant)
    created = create_intake_issue(tenant["client"], urls["intake_issues"])
    versions_url = urls["versions"](created["issue"]["id"])
    first = tenant["client"].get(versions_url)
    assert first.status_code == 200, first.text[:500]
    r = tenant["client"].get(
        versions_url, params={"cursor": f"1000:{MIN_I64}:0"}
    )
    assert r.status_code == 200, r.text[:500]
    body = r.json()
    assert body["results"] == first.json()["results"]
    assert body["total_results"] == first.json()["total_results"]
    assert body["next_cursor"] == first.json()["next_cursor"]
    assert body["next_page_results"] == first.json()["next_page_results"]
    assert body["prev_page_results"] is False
