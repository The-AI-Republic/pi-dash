"""NEWPIDASH-217: cycle work-item list relation filters (194/195 twins).

The cycle issue list (``GET .../cycles/{cid}/cycle-issues/``) compiles
the rich ``filters`` tree and the legacy query keys through one join
alias per relation. Where Django applies two separate ``.filter()``
calls (a fresh join per call), a single Rust alias forces one link row
to satisfy both predicates and lists nothing when the two sides name
different rows; likewise, negated relation leaves and ``or`` across
relations must keep/ exclude rows with subquery + LEFT-join semantics,
not per-row INNER-join semantics.

Seed (one cycle): A (assignee bot, label L1), B (bot + mention,
L1 + L2), C (nothing), D (mention), E (nothing). Expected sets are
Django's (pinned against the live oracle); the ``or`` case asserts the
distinct set because Django's own join fan-out returns B three times.
"""

import json

import pytest

from _harness.seed import ADMIN
from .conftest import create_issue_for, cycle_urls

pytestmark = pytest.mark.contract


def _seed_five(tenant):
    """Seed the A-E cycle; return the id map + user/label ids."""
    seed = tenant["seed"]
    ws_id = tenant["workspace"]["id"]
    proj_id = tenant["project"]["id"]
    urls = cycle_urls(tenant)
    bot = seed.user(email=f"bot-{tenant['cycle']['id'][:8]}@contract.test")
    mention = seed.user(
        email=f"mention-{tenant['cycle']['id'][:8]}@contract.test")
    for extra in (bot, mention):
        seed.member(ws_id, extra["id"], role=ADMIN)
        seed.project_member(proj_id, ws_id, extra["id"], role=ADMIN)
    l1 = seed.label(ws_id, proj_id, name="NEWPIDASH-217 L1")
    l2 = seed.label(ws_id, proj_id, name="NEWPIDASH-217 L2")
    a = create_issue_for(tenant, name="217 A")
    b = create_issue_for(tenant, name="217 B")
    c = create_issue_for(tenant, name="217 C")
    d = create_issue_for(tenant, name="217 D")
    e = create_issue_for(tenant, name="217 E")
    seed.issue_assignee(ws_id, proj_id, a["id"], bot["id"])
    seed.issue_label(ws_id, proj_id, a["id"], l1)
    seed.issue_assignee(ws_id, proj_id, b["id"], bot["id"])
    seed.issue_assignee(ws_id, proj_id, b["id"], mention["id"])
    seed.issue_label(ws_id, proj_id, b["id"], l1)
    seed.issue_label(ws_id, proj_id, b["id"], l2)
    seed.issue_assignee(ws_id, proj_id, d["id"], mention["id"])
    r = tenant["client"].post(
        urls["cycle_issues"](tenant["cycle"]["id"]),
        json={"issues": [a["id"], b["id"], c["id"], d["id"], e["id"]]},
    )
    assert r.status_code == 201, r.text[:500]
    return {
        "a": a["id"], "b": b["id"], "c": c["id"], "d": d["id"],
        "e": e["id"], "bot": bot["id"], "mention": mention["id"],
        "l1": l1, "l2": l2,
    }


def _teardown_bridges(tenant):
    with tenant["seed"].conn.cursor() as cur:
        cur.execute(
            "delete from cycle_issues where workspace_id=%s",
            (tenant["workspace"]["id"],),
        )
    tenant["seed"].conn.commit()


def _ids(resp):
    assert resp.status_code == 200, resp.text[:800]
    return {row["id"] for row in resp.json()["results"]}


def test_not_assignee_keeps_unassigned(make_tenant):
    """NEWPIDASH-217 (194 twin): NOT bot keeps C/D/E (D carries only the
    mention link; unlinked C/E stay; A/B carry bot and drop out)."""
    tenant = make_tenant(role=ADMIN)
    try:
        ids = _seed_five(tenant)
        urls = cycle_urls(tenant)
        got = _ids(tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]),
            params={
                "filters": json.dumps(
                    {"not": {"assignee_id__in": ids["bot"]}}),
            },
        ))
        assert got == {ids["c"], ids["d"], ids["e"]}, got
    finally:
        _teardown_bridges(tenant)


def test_not_label_keeps_unlabelled(make_tenant):
    """NEWPIDASH-217 (194 twin): NOT L1 keeps C/D/E."""
    tenant = make_tenant(role=ADMIN)
    try:
        ids = _seed_five(tenant)
        urls = cycle_urls(tenant)
        got = _ids(tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]),
            params={
                "filters": json.dumps(
                    {"not": {"label_id__in": ids["l1"]}}),
            },
        ))
        assert got == {ids["c"], ids["d"], ids["e"]}, got
    finally:
        _teardown_bridges(tenant)


def test_or_assignee_label_matches_either(make_tenant):
    """NEWPIDASH-217 (194 twin): OR across assignee and label matches
    B (both branches) and D (assignee branch only)."""
    tenant = make_tenant(role=ADMIN)
    try:
        ids = _seed_five(tenant)
        urls = cycle_urls(tenant)
        got = _ids(tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]),
            params={
                "filters": json.dumps({"or": [
                    {"assignee_id__in": ids["mention"]},
                    {"label_id__in": ids["l2"]},
                ]}),
            },
        ))
        assert got == {ids["b"], ids["d"]}, got
    finally:
        _teardown_bridges(tenant)


def test_rich_assignee_plus_legacy_assignees(make_tenant):
    """NEWPIDASH-217 (195 twin): rich mention AND legacy assignees=bot
    must join the assignee table twice and match B (both links)."""
    tenant = make_tenant(role=ADMIN)
    try:
        ids = _seed_five(tenant)
        urls = cycle_urls(tenant)
        got = _ids(tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]),
            params={
                "filters": json.dumps(
                    {"and": [{"assignee_id__in": ids["mention"]}]}),
                "assignees": ids["bot"],
            },
        ))
        assert got == {ids["b"]}, got
    finally:
        _teardown_bridges(tenant)


def test_rich_label_plus_legacy_labels(make_tenant):
    """NEWPIDASH-217 (195 twin): rich L2 AND legacy labels=L1 must join
    the label table twice and match B (both links)."""
    tenant = make_tenant(role=ADMIN)
    try:
        ids = _seed_five(tenant)
        urls = cycle_urls(tenant)
        got = _ids(tenant["client"].get(
            urls["cycle_issues"](tenant["cycle"]["id"]),
            params={
                "filters": json.dumps(
                    {"and": [{"label_id__in": ids["l2"]}]}),
                "labels": ids["l1"],
            },
        ))
        assert got == {ids["b"]}, got
    finally:
        _teardown_bridges(tenant)
