"""Domain fixtures: one isolated tenant (user + workspace + project + cycle)
per test, seeded straight into Postgres, talking to the live backend over
HTTP with a forged session cookie.

Harness generation: first written against the first-generation _harness
(db.conn/create_user/auth.login); rebased onto the Seed-based baseline,
keeping the seeded rows identical. Cycle/issue rows are built here because
they need owner/creator/state linkage the generic factories don't carry.
"""

import uuid
from datetime import datetime, timezone

import httpx
import pytest

from _harness import env
from _harness.seed import ADMIN, Seed


def _client_for(seed: Seed, user: dict) -> httpx.Client:
    cookies = seed.session_cookie(
        user["id"], user["password"], env.contract_secret()
    )
    return httpx.Client(
        base_url=env.base_url(),
        headers={"Cookie": f"session-id={cookies['session-id']}"},
        timeout=30,
    )


def _create_cycle(
    seed: Seed, *, workspace_id: str, project_id: str, owner_id: str,
    name: str = "Cycle", start_date=None, end_date=None,
    archived_at=None,
) -> dict:
    cid = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with seed.conn.cursor() as cur:
        cur.execute(
            """insert into cycles (id, name, description, start_date, end_date,
                owned_by_id, project_id, workspace_id, view_props, sort_order,
                external_source, external_id, progress_snapshot, archived_at,
                logo_props, timezone, version, created_by_id, created_at,
                updated_at)
               values (%s,%s,'',%s,%s,%s,%s,%s,'{}',65535,null,null,'{}',%s,
                '{}','UTC',1,%s,%s,%s)""",
            (cid, name, start_date, end_date, owner_id, project_id,
             workspace_id, archived_at, owner_id, now, now),
        )
    seed.track("cycles", "id", cid)
    return {"id": cid}


def _create_issue(
    seed: Seed, *, workspace_id: str, project_id: str, state_id: str,
    created_by_id: str, name: str = "Contract issue", sequence_id: int = 1,
) -> dict:
    iid = str(uuid.uuid4())
    now = datetime.now(timezone.utc)
    with seed.conn.cursor() as cur:
        cur.execute(
            """insert into issues (id, name, description_json, priority,
                sequence_id, project_id, workspace_id, state_id,
                description_html, sort_order, is_draft, git_work_branch,
                workpad, complexity_score, created_at, updated_at,
                created_by_id)
               values (%s,%s,'{}','high',%s,%s,%s,%s,'<p></p>',65535,false,
                '','',0,%s,%s,%s)""",
            (iid, name, sequence_id, project_id, workspace_id, state_id,
             now, now, created_by_id),
        )
    seed.track("issues", "id", iid)
    return {"id": iid}


def _build_tenant(seed: Seed, *, role: int = ADMIN) -> dict:
    tag = f"c-{uuid.uuid4().hex[:10]}"
    user = seed.user(email=f"{tag}@contract.test")
    ws = seed.workspace(user["id"], slug=tag)
    seed.member(ws["id"], user["id"], role=role)
    project_id = seed.project(ws["id"])
    seed.project_member(project_id, ws["id"], user["id"], role=role)
    state_id = seed.state(project_id, ws["id"])
    cycle = _create_cycle(
        seed, workspace_id=ws["id"], project_id=project_id,
        owner_id=user["id"],
    )
    return {
        "user": user, "workspace": ws, "project": {"id": project_id},
        "state": {"id": state_id}, "cycle": cycle, "role": role,
        "seq": [5000], "seed": seed,
        "client": _client_for(seed, user),
    }


def create_issue_for(tenant: dict, name: str = "Contract issue") -> dict:
    """Seed an issue in the tenant's project straight into Postgres."""
    tenant["seq"][0] += 1
    return _create_issue(
        tenant["seed"], workspace_id=tenant["workspace"]["id"],
        project_id=tenant["project"]["id"], state_id=tenant["state"]["id"],
        created_by_id=tenant["user"]["id"], name=name,
        sequence_id=tenant["seq"][0],
    )


def create_cycle_for(tenant: dict, name: str = "Disposable") -> dict:
    """Seed a spare cycle in the tenant's project straight into Postgres."""
    return _create_cycle(
        tenant["seed"], workspace_id=tenant["workspace"]["id"],
        project_id=tenant["project"]["id"], owner_id=tenant["user"]["id"],
        name=name,
    )


@pytest.fixture()
def make_tenant():
    """Fresh isolated tenant per test (rows tracked, deleted at teardown).

    Forged session cookies cost no throttle budget, but shape tests still
    share the session `admin` tenant while only access tests mint tenants.
    """
    created = []

    def _make(**kwargs) -> dict:
        conn = env.connect()
        tenant = _build_tenant(Seed(conn), **kwargs)
        tenant["conn"] = conn
        created.append(tenant)
        return tenant

    yield _make

    for tenant in created:
        tenant["client"].close()
        try:
            tenant["seed"].cleanup()
        finally:
            tenant["conn"].close()


@pytest.fixture(scope="session")
def admin():
    conn = env.connect()
    tenant = _build_tenant(Seed(conn), role=ADMIN)
    yield tenant
    tenant["client"].close()
    conn.close()


def cycle_urls(tenant: dict) -> dict:
    base = (
        f"/api/workspaces/{tenant['workspace']['slug']}"
        f"/projects/{tenant['project']['id']}"
    )
    cycles = f"{base}/cycles/"
    return {
        "cycles": cycles,
        "detail": lambda pk: f"{cycles}{pk}/",
        "cycle_issues": lambda pk: f"{cycles}{pk}/cycle-issues/",
        "cycle_issue_detail": lambda pk, issue_id: (
            f"{cycles}{pk}/cycle-issues/{issue_id}/"
        ),
        "date_check": f"{base}/cycles/date-check/",
        "favorites": f"{base}/user-favorite-cycles/",
        "favorite_detail": lambda pk: f"{base}/user-favorite-cycles/{pk}/",
        "transfer": lambda pk: f"{cycles}{pk}/transfer-issues/",
        "user_properties": lambda pk: f"{cycles}{pk}/user-properties/",
        "archive": lambda pk: f"{cycles}{pk}/archive/",
        "archived": f"{base}/archived-cycles/",
        "archived_detail": lambda pk: f"{base}/archived-cycles/{pk}/",
        "progress": lambda pk: f"{cycles}{pk}/progress/",
        "analytics": lambda pk: f"{cycles}{pk}/analytics/",
        "workspace_cycles": f"/api/workspaces/{tenant['workspace']['slug']}/cycles/",
    }
