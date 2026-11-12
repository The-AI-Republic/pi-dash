# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Domain fixtures: one isolated tenant (member user + workspace + project)
per test, seeded straight into Postgres, talking to the live backend over
HTTP with a forged session cookie.

Same shape as the app_cycles tenant factory (cross-domain conftest imports
are not used in this repo, so the small factory is repeated here).
"""

import uuid

import httpx
import pytest

from _harness import env
from _harness.seed import MEMBER, Seed


def _client_for(seed: Seed, user: dict) -> httpx.Client:
    cookies = seed.session_cookie(
        user["id"], user["password"], env.contract_secret()
    )
    return httpx.Client(
        base_url=env.base_url(),
        headers={"Cookie": f"session-id={cookies['session-id']}"},
        timeout=60,
    )


def _build_tenant(seed: Seed, *, role: int = MEMBER) -> dict:
    tag = f"bl-{uuid.uuid4().hex[:10]}"
    user = seed.user(email=f"{tag}@contract.test")
    ws = seed.workspace(user["id"], slug=tag)
    seed.member(ws["id"], user["id"], role=role)
    identifier = f"BL1308{uuid.uuid4().hex[:6].upper()}"
    project_id = seed.project(ws["id"], identifier=identifier)
    seed.project_member(project_id, ws["id"], user["id"], role=role)
    return {
        "user": user, "workspace": ws,
        "project": {"id": project_id, "identifier": identifier},
        "role": role, "seed": seed,
        "client": _client_for(seed, user),
    }


@pytest.fixture()
def make_tenant():
    """Fresh isolated tenant per test (rows tracked, deleted at teardown)."""
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
            with tenant["conn"].cursor() as cur:
                # Server-created side rows Seed never tracked (the PATCH
                # get-or-creates a property row; the POST inserts a cycle):
                # sweep them first so the tracked deletes keep FK order.
                cur.execute(
                    "DELETE FROM project_user_properties WHERE project_id = %s",
                    (tenant["project"]["id"],),
                )
                cur.execute(
                    "DELETE FROM cycles WHERE project_id = %s",
                    (tenant["project"]["id"],),
                )
            tenant["seed"].cleanup()
        finally:
            tenant["conn"].close()
