"""GET /api/v1/workspaces/{slug}/runners/ — CLI runner list (NEWPIDASH-146).

Django: `RunnerListAPIEndpoint` (`api/views/runner.py`) — token counterpart
of the session `/api/runners/` list with shared visibility, filters and
serializer. CLI uses `workspaces/{slug}/runners/?project=<id>` and
`?assignable_for=<issue|new>` for dedicated-runner picker.
"""

from __future__ import annotations


def _seed_runner(seed, tenant, project=None, pod=None):
    project = project or seed.project(tenant["workspace"]["id"])
    pod = pod or seed.pod(tenant["workspace"]["id"], project)
    runner = seed.runner(tenant["user"]["id"], tenant["workspace"]["id"], pod)
    return runner, pod, project


def _path(slug, **params):
    base = f"/api/v1/workspaces/{slug}/runners/"
    if params:
        from urllib.parse import urlencode

        return f"{base}?{urlencode(params)}"
    return base


def _get(api, slug, token, params=None):
    headers = {"X-Api-Key": token} if token else {}
    return api.get(_path(slug, **(params or {})), headers=headers)


def test_list_requires_token(api, seed, tenant_a):
    assert _get(api, tenant_a["workspace"]["slug"], None).status_code == 401
    assert _get(api, tenant_a["workspace"]["slug"], "bogus").status_code == 403


def test_list_returns_visible_runners(api, seed, tenant_a):
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    runner, _, _ = _seed_runner(seed, tenant_a)
    resp = _get(api, tenant_a["workspace"]["slug"], token)
    assert resp.status_code == 200, resp.text
    data = resp.json()
    assert isinstance(data, list)
    assert any(str(row["id"]) == str(runner) for row in data)
    # Plain list omits the four eligibility fields.
    for row in data:
        assert "assignable" not in row
        assert "not_assignable_code" not in row
        assert "is_busy" not in row
        assert "queue_depth" not in row


def test_list_project_filter_narrows(api, seed, tenant_a):
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    runner, _, project = _seed_runner(seed, tenant_a)
    resp = _get(api, tenant_a["workspace"]["slug"], token, {"project": str(project)})
    assert resp.status_code == 200, resp.text
    assert any(str(row["id"]) == str(runner) for row in resp.json())
    # Bogus project narrows to nothing.
    empty = _get(
        api,
        tenant_a["workspace"]["slug"],
        token,
        {"project": "00000000-0000-0000-0000-000000000000"},
    )
    assert empty.status_code == 200, empty.text
    assert not any(str(row["id"]) == str(runner) for row in empty.json())


def test_list_assignable_for_adds_fields(api, seed, tenant_a, db):
    from datetime import datetime, timezone

    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    runner, pod, project = _seed_runner(seed, tenant_a)
    # Issue created by the runner owner in the same project.
    issue_id = str(seed.issue(tenant_a["workspace"]["id"], project))
    with db.cursor() as cur:
        cur.execute(
            "UPDATE issues SET created_by_id = %s WHERE id = %s",
            (tenant_a["user"]["id"], issue_id),
        )
    resp = _get(
        api,
        tenant_a["workspace"]["slug"],
        token,
        {"project": str(project), "assignable_for": issue_id},
    )
    assert resp.status_code == 200, resp.text
    row = next(r for r in resp.json() if str(r["id"]) == str(runner))
    assert row["assignable"] is True
    assert row["not_assignable_code"] == ""
    assert row["is_busy"] is False
    assert row["queue_depth"] == 0


def test_list_engine_sign_in_state_derived(api, seed, tenant_a):
    """NEWPIDASH-146 F1: dev_machine_detail derives engine_sign_in_state.

    Stored "active" + past expiry renders "expired" (never the raw stored
    value). Against Django this is the serializer's derivation; against
    Rust it is the engine_credential kernel.
    """
    from datetime import datetime, timedelta, timezone

    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    project = seed.project(tenant_a["workspace"]["id"])
    pod = seed.pod(tenant_a["workspace"]["id"], project)
    machine = seed.dev_machine(
        tenant_a["user"]["id"],
        engine_sign_in_state="active",
        engine_credential_expires_at=datetime.now(timezone.utc) - timedelta(days=1),
    )
    runner = seed.runner(
        tenant_a["user"]["id"],
        tenant_a["workspace"]["id"],
        pod,
        dev_machine_id=machine,
    )
    resp = _get(api, tenant_a["workspace"]["slug"], token)
    assert resp.status_code == 200, resp.text
    row = next(r for r in resp.json() if str(r["id"]) == str(runner))
    detail = row["dev_machine_detail"]
    assert detail is not None
    assert detail["engine_sign_in_state"] == "expired"


def test_list_machine_display_name_this_computer(api, seed, tenant_a):
    """NEWPIDASH-146 F2: a bound machine with empty label+host renders
    "This computer", not the runner's own name."""
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    project = seed.project(tenant_a["workspace"]["id"])
    pod = seed.pod(tenant_a["workspace"]["id"], project)
    machine = seed.dev_machine(tenant_a["user"]["id"], host_label="")
    runner = seed.runner(
        tenant_a["user"]["id"],
        tenant_a["workspace"]["id"],
        pod,
        dev_machine_id=machine,
    )
    resp = _get(api, tenant_a["workspace"]["slug"], token)
    assert resp.status_code == 200, resp.text
    row = next(r for r in resp.json() if str(r["id"]) == str(runner))
    assert row["dev_machine_detail"] is not None
    assert row["machine_display_name"] == "This computer"


def test_list_live_state_snapshot(api, seed, tenant_a, db):
    """NEWPIDASH-146 F3: live_state is the RunnerLiveState snapshot when a
    row exists, null when it does not."""
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    project = seed.project(tenant_a["workspace"]["id"])
    pod = seed.pod(tenant_a["workspace"]["id"], project)
    runner = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod)
    bare = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod)
    with db.cursor() as cur:
        cur.execute(
            "INSERT INTO runner_live_state "
            "(runner_id, observed_run_id, last_event_at, last_event_kind, "
            "last_event_summary, agent_pid, agent_subprocess_alive, "
            "approvals_pending, usage, llm_model, turn_count, "
            "agent_health, agent_last_activity_at, updated_at) "
            "VALUES (%s, NULL, NULL, 'tool', 'ran ls', "
            "4242, true, 0, '{}', 'm', 3, NULL, NULL, now())",
            (str(runner),),
        )
    resp = _get(api, tenant_a["workspace"]["slug"], token)
    assert resp.status_code == 200, resp.text
    rows = {str(r["id"]): r for r in resp.json()}
    live = rows[str(runner)]["live_state"]
    assert live is not None
    assert live["agent_pid"] == 4242
    assert live["llm_model"] == "m"
    assert live["turn_count"] == 3
    assert rows[str(bare)]["live_state"] is None


def test_list_unknown_workspace_404s(api, seed, tenant_a):
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    resp = _get(api, "no-such-slug-xyz", token)
    assert resp.status_code == 404, resp.text
    assert resp.json() == {"error": "workspace not found"}


def test_list_non_member_forbidden(api, seed, tenant_a, tenant_b):
    # tenant_b's workspace slug is visible only to tenant_b; tenant_a is not
    # a member there, so the check is 403 forbidden.
    token_b = seed.api_token(tenant_b["user"]["id"], tenant_b["workspace"]["id"])
    # tenant_a is not a member of tenant_b's workspace.
    resp = _get(api, tenant_b["workspace"]["slug"], token_b)
    # tenant_b is a member of its own workspace, so it should be 200.
    assert resp.status_code == 200
    token_a = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    resp2 = _get(api, tenant_b["workspace"]["slug"], token_a)
    assert resp2.status_code == 403, resp2.text
    assert resp2.json() == {"error": "forbidden"}


def test_list_ordering_updated_at(api, seed, tenant_a, db):
    token = seed.api_token(tenant_a["user"]["id"], tenant_a["workspace"]["id"])
    project = seed.project(tenant_a["workspace"]["id"])
    pod = seed.pod(tenant_a["workspace"]["id"], project)
    r1 = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod)
    r2 = seed.runner(tenant_a["user"]["id"], tenant_a["workspace"]["id"], pod)
    # Newer updated_at first (Django orders by -updated_at).
    with db.cursor() as cur:
        cur.execute(
            "UPDATE runner SET updated_at = NOW() - INTERVAL '10 seconds' WHERE id = %s",
            (str(r1),),
        )
    resp = _get(api, tenant_a["workspace"]["slug"], token)
    assert resp.status_code == 200, resp.text
    rows = resp.json()
    ids = [str(r["id"]) for r in rows if str(r["id"]) in (str(r1), str(r2))]
    # r2 is newer, so it should come first if both are visible.
    if len(ids) == 2:
        assert ids[0] == str(r2), f"expected {r2} first, got {ids}"
