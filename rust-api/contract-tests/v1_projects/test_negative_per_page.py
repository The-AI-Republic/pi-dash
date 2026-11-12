"""Negative per_page with a crafted cursor (PIDASHCONV-1718).

Django: `queryset[offset:stop]` with stop<offset is empty, then
`results[:limit]` with limit<0 raises ValueError -> generic 500.
Before the fix the project/state handlers sliced `rows[start..stop]` with
start>stop and panicked (dropped connection, no status). They must 500.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import db, http  # noqa: E402

GENERIC_500 = {"error": "Something went wrong please try again later"}

KEY_PROJECTS = "projects"
KEY_STATES = "states"


def _project_base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/projects"


def _state_base(seed):
    return (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['id']}/states"
    )


def test_project_list_negative_per_page_crafted_cursor_is_500(seed, conn):
    # Seed at least 5 visible projects so the pre-fix slice had 5+ rows.
    # Each tag is a fresh 8-hex tag: `CT<tag>`[:12] must stay unique.
    for _ in range(5):
        pid = db.create_project(
            conn, seed["ws_a"]["id"], db.new_tag(),
            created_by_id=seed["owner"]["id"],
        )
        db.add_project_member(
            conn, seed["ws_a"]["id"], pid["id"], seed["owner"]["id"], db.ADMIN
        )
    r = http.get(
        seed["keys"][KEY_PROJECTS], _project_base(seed) + "/",
        params={"per_page": "-3", "cursor": "-3:-2:0"}, expect=500,
    )
    assert r.json() == GENERIC_500


def test_state_list_negative_per_page_crafted_cursor_is_500(seed, conn):
    # A new project seeds default states; ensure 5+ states exist.
    tag = db.new_tag()
    for i in range(5):
        db.create_state(
            conn, seed["ws_a"]["id"], seed["project"]["id"], f"{tag}s{i}",
            group="backlog", sequence=float(100 + i),
        )
    r = http.get(
        seed["keys"][KEY_STATES], _state_base(seed) + "/",
        params={"per_page": "-3", "cursor": "-3:-2:0"}, expect=500,
    )
    assert r.json() == GENERIC_500


def test_project_list_per_page_minus_one_is_500(seed):
    r = http.get(
        seed["keys"][KEY_PROJECTS], _project_base(seed) + "/",
        params={"per_page": "-1", "cursor": "-1:0:0"}, expect=500,
    )
    assert r.json() == GENERIC_500


def test_state_list_per_page_minus_one_is_500(seed):
    r = http.get(
        seed["keys"][KEY_STATES], _state_base(seed) + "/",
        params={"per_page": "-1", "cursor": "-1:0:0"}, expect=500,
    )
    assert r.json() == GENERIC_500


def test_negative_lists_default_pagination_unchanged(seed):
    r = http.get(seed["keys"][KEY_PROJECTS], _project_base(seed) + "/")
    assert set(r.json().keys()) == {
        "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
        "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
        "total_count", "total_pages", "total_results",
    }
    r = http.get(seed["keys"][KEY_STATES], _state_base(seed) + "/")
    assert set(r.json().keys()) == {
        "count", "extra_stats", "grouped_by", "next_cursor", "next_page_results",
        "prev_cursor", "prev_page_results", "results", "sub_grouped_by",
        "total_count", "total_pages", "total_results",
    }
