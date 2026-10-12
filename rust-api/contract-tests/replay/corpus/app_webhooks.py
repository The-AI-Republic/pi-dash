# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: app-tier webhook write flows (PIDASHCONV-823).

Built from the ``app_integrations`` contract suite
(test_webhooks.py: CRUD, duplicate-URL 409, URL validation 400s,
member 403s, secret regenerate). Session-cookie auth (admin/member),
like the suite.

The minted ``secret_key`` (``pi_dash_wh_`` + per-backend randomness,
on create and on every regenerate) is captured out of each backend's
own response, so capture substitution aligns it in bodies, rows and
jobs (the v1_projects logo pattern); ``example.com`` create URLs mirror
the suite (validated offline, never called).
"""

from ..session import Request, Session

_W = "/api/workspaces/{ws}/webhooks"
_WH1 = "https://example.com/hook/replay-wh1"

MISSING_ID = "00000000-0000-0000-0000-000000000000"


SESSIONS = (
    Session(
        name="app_webhooks_lifecycle",
        domain="app_integrations",
        source="app_integrations/test_webhooks.py",
        auth_users=("admin", "member"),
        requests=(
            # Creates: the world seed already owns one webhook row, so
            # the list is not empty -> create -> duplicate 409 ->
            # scheme 400 -> local-URL 400 -> member 403.
            Request(
                "GET", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                auth="admin", expect_status=200,
            ),
            Request(
                "POST", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                json={"url": _WH1, "issue": True, "project": True},
                auth="admin",
                capture={"wh": "id", "secret": "secret_key"},
                expect_status=201,
            ),
            Request(
                "POST", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                json={"url": _WH1},
                auth="admin", expect_status=409,
            ),
            Request(
                "POST", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                json={"url": "not-a-url"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                json={"url": "http://127.0.0.1/hook/x"},
                auth="admin", expect_status=400,
            ),
            Request(
                "POST", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                json={"url": "https://example.com/hook/replay-member"},
                auth="member", expect_status=403,
            ),
            # Reads + patch: list -> detail -> unknown 404 ->
            # toggle -> unknown 404.
            Request(
                "GET", _W + "/",
                route="/api/workspaces/{slug}/webhooks/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _W + "/{wh}/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="admin", expect_status=200,
            ),
            Request(
                "GET", _W + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "PATCH", _W + "/{wh}/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                json={"is_active": False},
                auth="admin", expect_status=200,
            ),
            Request(
                "PATCH", _W + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                json={"is_active": False},
                auth="admin", expect_status=404,
            ),
            # Regenerate rotates the secret (captured under a fresh
            # name — capture names are unique per session; the old
            # value exists nowhere after the rotation) -> unknown
            # 404 -> member 403 on the live row -> delete -> gone.
            Request(
                "POST", _W + "/{wh}/regenerate/",
                route="/api/workspaces/{slug}/webhooks/{pk}/regenerate/",
                auth="admin",
                capture={"secret2": "secret_key"},
                expect_status=200,
            ),
            Request(
                "POST", _W + "/" + MISSING_ID + "/regenerate/",
                route="/api/workspaces/{slug}/webhooks/{pk}/regenerate/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _W + "/{wh}/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="member", expect_status=403,
            ),
            Request(
                "DELETE", _W + "/{wh}/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="admin", expect_status=204,
            ),
            Request(
                "GET", _W + "/{wh}/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "DELETE", _W + "/" + MISSING_ID + "/",
                route="/api/workspaces/{slug}/webhooks/{pk}/",
                auth="admin", expect_status=404,
            ),
            Request(
                "GET", "/api/workspaces/{ws}/webhook-logs/{wh}/",
                route="/api/workspaces/{slug}/webhook-logs/{webhook_id}/",
                auth="admin", expect_status=200,
            ),
        ),
    ),
)
