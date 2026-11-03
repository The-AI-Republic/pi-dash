"""Contract tests: v1 workspace scheduler list (PIDASHCONV-1061).

Ports ``TestWorkspaceSchedulerList`` from Django's
``apps/api/pi_dash/tests/contract/api/test_schedulers.py`` (PDASHOSS01-225)
plus the envelope/shape pins, the fields/expand matrix, auth-preamble cases
and tenant isolation.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "ws"

PAGE_KEYS = [
    "grouped_by", "sub_grouped_by", "total_count", "next_cursor",
    "prev_cursor", "next_page_results", "prev_page_results", "count",
    "total_pages", "total_results", "extra_stats", "results",
]

SCHEDULER_KEYS = [
    "id", "workspace", "slug", "name", "description", "prompt", "color",
    "source", "is_builtin", "is_enabled", "bindings", "created_at", "updated_at",
]

FORBIDDEN_BODY = {"detail": "You do not have permission to perform this action."}
UNAUTHENTICATED_BODY = {"detail": "Authentication credentials were not provided."}
INVALID_TOKEN_BODY = {"detail": "Given API token is not valid"}


def base(seed):
    return f"/api/v1/workspaces/{seed['ws_a']['slug']}/schedulers/"


def test_member_lists_workspace_schedulers(seed):
    r = http.get(seed["keys"][KEY], base(seed))
    body = r.json()
    assert list(body.keys()) == PAGE_KEYS
    rows = {row["slug"]: row for row in body["results"]}
    assert seed["scheduler"]["slug"] in rows
    row = rows[seed["scheduler"]["slug"]]
    assert list(row.keys()) == SCHEDULER_KEYS
    assert row["prompt"] == "Scan the project for stalled issues."
    assert row["is_enabled"] is True
    binding_projects = [str(b["project"]) for b in row["bindings"]]
    assert str(seed["project"]["id"]) in binding_projects


def test_bindings_hidden_for_non_project_members(seed):
    # A workspace member who is not on the project still sees the
    # definition, but not the install on a project they cannot access.
    r = http.get(seed["keys"]["nonmember"], base(seed))
    rows = {row["slug"]: row for row in r.json()["results"]}
    assert rows[seed["scheduler"]["slug"]]["bindings"] == []


def test_outsider_denied(seed):
    r = http.get(seed["keys"]["outsider"], base(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_write_methods_rejected(seed):
    # POST is the create (PDASHOSS01-318, `test_writes.py`).
    with http.client(seed["keys"][KEY]) as c:
        r = c.request("PUT", base(seed), json={})
    assert r.status_code == 405
    assert r.json() == {"detail": 'Method "PUT" not allowed.'}


def test_anonymous_401(seed):
    r = http.get(None, base(seed), expect=401)
    assert r.json() == UNAUTHENTICATED_BODY


def test_invalid_token_403(seed):
    r = http.get("ct-bogus-token", base(seed), expect=403)
    assert r.json() == INVALID_TOKEN_BODY


def test_unknown_slug_403(seed):
    # Unknown slug 403s (never 404s): the gate's slug-scoped lookup
    # misses before any object lookup runs.
    r = http.get(
        seed["keys"][KEY],
        f"/api/v1/workspaces/no-such-{seed['tag']}/schedulers/",
        expect=403,
    )
    assert r.json() == FORBIDDEN_BODY


def test_other_workspace_member_denied(seed):
    r = http.get(seed["keys"]["owner_b"], base(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_other_workspace_scheduler_not_listed(seed):
    r = http.get(seed["keys"][KEY], base(seed))
    slugs = [row["slug"] for row in r.json()["results"]]
    assert seed["other_scheduler"]["slug"] not in slugs


def test_fields_filter_keeps_subset_in_order(seed):
    r = http.get(seed["keys"][KEY], base(seed), params={"fields": "name,slug"})
    for row in r.json()["results"]:
        assert list(row.keys()) == ["slug", "name"]


def test_fields_unknown_names_ignored(seed):
    r = http.get(
        seed["keys"][KEY], base(seed), params={"fields": "slug,no-such-field"}
    )
    for row in r.json()["results"]:
        assert list(row.keys()) == ["slug"]


def test_expand_workspace_returns_lite(seed):
    r = http.get(seed["keys"][KEY], base(seed), params={"expand": "workspace"})
    for row in r.json()["results"]:
        lite = row["workspace"]
        assert list(lite.keys()) == ["name", "slug", "id"]
        assert lite["slug"] == seed["ws_a"]["slug"]
        assert lite["id"] == seed["ws_a"]["id"]


def test_expand_unknown_name_nulls_field(seed):
    # `expand=bindings` (or any name outside the expansion map) renders
    # the `<name>_id` getattr fallback, which is None on a scheduler.
    r = http.get(seed["keys"][KEY], base(seed), params={"expand": "bindings"})
    for row in r.json()["results"]:
        assert row["bindings"] is None


def test_expand_missing_field_ignored(seed):
    # expand only applies to fields present after fields-filtering.
    r = http.get(
        seed["keys"][KEY],
        base(seed),
        params={"fields": "slug", "expand": "workspace"},
    )
    for row in r.json()["results"]:
        assert list(row.keys()) == ["slug"]
