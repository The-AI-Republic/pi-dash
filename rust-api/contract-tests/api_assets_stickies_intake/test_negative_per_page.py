"""Negative per_page with a crafted cursor (PIDASHCONV-1718).

`parse_per_page` lets a negative per_page through (Django's get_per_page).
`per_page=-3&cursor=-3:-2:0` gives limit -3, page -2, offset 6, stop 4:
`queryset[6:4]` is empty in Django but `results[:limit]` with a negative
limit always raises ValueError, so Django answers every limit<0 with the
generic 500. Rust must answer 500 (a response, not a dropped connection):
before the fix the sticky/intake handlers panicked on the negative width.
"""

GENERIC_500 = {"error": "Something went wrong please try again later"}


def _intake_base(org):
    return (
        f"/api/v1/workspaces/{org.workspace['slug']}"
        f"/projects/{org.project['id']}/intake-issues/"
    )


def _sticky_base(org):
    return f"/api/v1/workspaces/{org.workspace['slug']}/stickies/"


def test_sticky_list_negative_per_page_crafted_cursor_is_500(org):
    r = org.request(
        "GET", _sticky_base(org), "member",
        params={"per_page": "-3", "cursor": "-3:-2:0"},
    )
    assert r.status_code == 500
    assert r.json() == GENERIC_500


def test_intake_list_negative_per_page_crafted_cursor_is_500(org):
    r = org.request(
        "GET", _intake_base(org), "member",
        params={"per_page": "-3", "cursor": "-3:-2:0"},
    )
    assert r.status_code == 500
    assert r.json() == GENERIC_500


def test_sticky_list_per_page_minus_one_is_500(org):
    # Regression: per_page=-1 stays 500 (Django ValueError via results[:limit]).
    r = org.request(
        "GET", _sticky_base(org), "member",
        params={"per_page": "-1", "cursor": "-1:0:0"},
    )
    assert r.status_code == 500
    assert r.json() == GENERIC_500


def test_intake_list_per_page_minus_one_is_500(org):
    r = org.request(
        "GET", _intake_base(org), "member",
        params={"per_page": "-1", "cursor": "-1:0:0"},
    )
    assert r.status_code == 500
    assert r.json() == GENERIC_500


def test_negative_lists_default_pagination_unchanged(org):
    # Regression: default pagination still returns 200 with the 12-key envelope.
    r = org.request("GET", _sticky_base(org), "member")
    assert r.status_code == 200
    body = r.json()
    assert set(body) == {
        "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
        "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
        "total_count", "total_pages", "total_results",
    }
    r = org.request("GET", _intake_base(org), "member")
    assert r.status_code == 200
    body = r.json()
    assert set(body) == {
        "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
        "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
        "total_count", "total_pages", "total_results",
    }
