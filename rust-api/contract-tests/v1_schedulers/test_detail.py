"""Contract tests: v1 project scheduler detail (PIDASHCONV-1061).

Ports ``TestProjectSchedulerDetail`` from Django's
``apps/api/pi_dash/tests/contract/api/test_schedulers.py`` (PDASHOSS01-225)
plus the full-shape pin and the fields/expand-ignored quirk.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from _harness import http  # noqa: E402

KEY = "dt"

SCHEDULER_KEYS = [
    "id", "workspace", "slug", "name", "description", "prompt", "color",
    "source", "is_builtin", "is_enabled", "bindings", "created_at", "updated_at",
]

FORBIDDEN_BODY = {"detail": "You do not have permission to perform this action."}
NOT_FOUND_BODY = {"detail": "No Scheduler matches the given query."}


def url(seed, scheduler_id=None):
    sid = scheduler_id if scheduler_id is not None else seed["scheduler"]["id"]
    return (
        f"/api/v1/workspaces/{seed['ws_a']['slug']}"
        f"/projects/{seed['project']['id']}/schedulers/{sid}/"
    )


def test_member_reads_full_definition(seed):
    r = http.get(seed["keys"][KEY], url(seed))
    body = r.json()
    assert list(body.keys()) == SCHEDULER_KEYS
    assert body["prompt"] == "Scan the project for stalled issues."
    assert body["source"] == "builtin"
    assert len(body["bindings"]) == 1
    assert str(body["bindings"][0]["id"]) == str(seed["binding"]["id"])


def test_404_when_not_installed_on_project(seed):
    r = http.get(
        seed["keys"][KEY], url(seed, seed["bare_scheduler"]["id"]), expect=404
    )
    assert r.json() == NOT_FOUND_BODY


def test_404_when_scheduler_missing(seed):
    r = http.get(
        seed["keys"][KEY],
        url(seed, "00000000-0000-0000-0000-000000000000"),
        expect=404,
    )
    assert r.json() == NOT_FOUND_BODY


def test_nonmember_denied(seed):
    r = http.get(seed["keys"]["nonmember"], url(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_outsider_denied(seed):
    r = http.get(seed["keys"]["outsider"], url(seed), expect=403)
    assert r.json() == FORBIDDEN_BODY


def test_fields_and_expand_ignored(seed):
    # The detail serializer is constructed without fields/expand, so
    # both params are ignored (ported quirk).
    plain = http.get(seed["keys"][KEY], url(seed)).json()
    shaped = http.get(
        seed["keys"][KEY],
        url(seed),
        params={"fields": "slug", "expand": "workspace"},
    ).json()
    assert shaped == plain


def test_non_uuid_scheduler_id_404(seed):
    # The <uuid:> converter rejects the segment before any view runs
    # (status only: DEBUG=True serves the technical 404 page, and Rust
    # proxies the same bytes).
    http.get(
        seed["keys"][KEY],
        url(seed).replace(str(seed["scheduler"]["id"]), "not-a-uuid"),
        expect=404,
    )
