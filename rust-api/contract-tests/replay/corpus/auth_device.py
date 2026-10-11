# Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Corpus: CLI device-code flow + machine tokens (PIDASHCONV-823).

Built from the ``auth_device`` and ``v1_cli_auth`` contract suites:
test_device_lifecycle.py (start -> pending poll -> browser approve ->
token poll -> machine-token exchange -> revoke), test_device_approve.py,
test_device_token.py, test_machine_token.py and test_revoke.py.
"""

from ..session import Request, Session

EXPIRED_DC = "replay-expired-device-code-01"
EXPIRED_UC = "RPXD-0001"
DENIED_DC = "replay-denied-device-code-02"
DENIED_UC = "RPXD-0002"
DEV_MACHINE_ID = "11111111-2222-3333-4444-555555555555"


def _seed_extras(conn) -> None:
    """Expired + denied device rows with fixed codes (mirrors Seed.device_code)."""
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO cli_device_codes
                (id, device_code, user_code, user_id, workspace_id, approved,
                 denied, consumed, expires_at, created_at, updated_at)
            VALUES
                (gen_random_uuid(), %s, %s, NULL, NULL, false, false, false,
                 now() - interval '1 hour', now(), now()),
                (gen_random_uuid(), %s, %s, NULL, NULL, false, true, false,
                 now() + interval '10 minutes', now(), now())
            """,
            (EXPIRED_DC, EXPIRED_UC, DENIED_DC, DENIED_UC),
        )
    conn.commit()


SESSIONS = (
    Session(
        name="auth_device_lifecycle",
        domain="v1_cli_auth",
        source="auth_device/test_device_lifecycle.py,"
        " v1_cli_auth/test_device_approve.py, test_device_token.py,"
        " test_machine_token.py, test_revoke.py",
        auth_users=("member",),
        seed=_seed_extras,
        # The device/user codes and both tokens come back in
        # responses, so captures substitute them; only the random
        # api_tokens.label needs blanking.
        generated_columns={
            "api_tokens": ["label"],
            # token_hash/fingerprint derive from the minted machine
            # token (random per backend on both sides).
            "machine_token": ["token_hash", "token_fingerprint"],
        },
        timestamp_columns={
            "cli_device_codes": ["expires_at", "last_polled_at"],
            "dev_machine": ["last_seen_at"],
            "machine_token": ["last_used_at", "revoked_at"],
        },
        # process_logs embeds two timezone.now() stamps per backend
        # (Django micros, Rust seconds): each side mints its own now,
        # so both blank via the explicit time map.
        payload_time_paths={
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        # --- generated PIDASHCONV-920 header-order pins (one exact
        # pair per emitting request; lengths blanked; {bridge} /
        # {machine_token} substituted, "bogus" literal; bodyless legs
        # also pin Django's phantom content headers — same issue) ---
        payload_value_diffs={
            "pi_dash.bgtasks.logger_task.process_logs": [
        # req 4 GET /api/v1/auth/workspaces/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}"),
        # req 5 POST /api/v1/auth/machine-token/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{bridge}', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 6 GET /api/v1/auth/workspaces/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
        # req 7 POST /api/v1/auth/machine-token/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': 'bogus'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': 'bogus', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': 'bogus'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': 'bogus', 'Content-Length': '<replay-volatile>', 'Content-Type': 'application/json'}"),
        # req 8 POST /api/v1/auth/revoke/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Content-Length': '<replay-volatile>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Content-Length': '<replay-volatile>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
        # req 9 GET /api/v1/auth/workspaces/
        ("kwargs/log_data/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
        ("kwargs/mongo_log/headers", "PIDASHCONV-920",
         "{'Content-Length': '<replay-volatile>', 'Content-Type': 'text/plain', 'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}",
         "{'Host': '<replay-host>', 'Accept': '*/*', 'Accept-Encoding': 'gzip, deflate', 'Connection': 'keep-alive', 'User-Agent': 'python-httpx/0.28.1', 'X-Api-Key': '{machine_token}'}"),
            ],
        },
        requests=(
            # -- start -> pending poll -> approve -> token --
            Request(
                "POST", "/api/v1/auth/device/start/",
                route="/api/v1/auth/device/start/",
                json={},
                auth="anonymous",
                capture={"device_code": "device_code", "user_code": "user_code"},
                expect_status=200,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code}"},
                auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", "/api/v1/auth/device/approve/",
                route="/api/v1/auth/device/approve/",
                json={"user_code": "{user_code}"},
                auth="member", expect_status=200,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code}"},
                auth="anonymous",
                capture={"bridge": "access_token"},
                # RFC 8628 polls enforce a 3s minimum gap.
                delay_before=4.0,
                expect_status=200,
            ),
            # -- the bridge token authenticates --
            Request(
                "GET", "/api/v1/auth/workspaces/",
                route="/api/v1/auth/workspaces/",
                headers={"X-Api-Key": "{bridge}"},
                auth="anonymous", expect_status=200,
                # PIDASHCONV-937: mongo user stamps (the device-flow
                # member id vs null; the bogus/revoked 403 legs agree on
                # null and carry no pin).
                payload_value_diffs={
                    "pi_dash.bgtasks.logger_task.process_logs": [
                        ("kwargs/mongo_log/created_by", "PIDASHCONV-937",
                         "{member}", None),
                        ("kwargs/mongo_log/updated_by", "PIDASHCONV-937",
                         "{member}", None),
                    ],
                },
            ),
            # -- exchange the bridge for a machine token --
            Request(
                "POST", "/api/v1/auth/machine-token/",
                route="/api/v1/auth/machine-token/",
                json={
                    "workspace_slug": "{ws}",
                    "dev_machine_id": DEV_MACHINE_ID,
                    "host_label": "replay-macbook",
                },
                headers={"X-Api-Key": "{bridge}"},
                auth="anonymous",
                capture={"machine_token": "machine_token"},
                expect_status=201,
                # PIDASHCONV-937: mongo user stamps (the device-flow
                # member id vs null; the bogus/revoked 403 legs agree on
                # null and carry no pin).
                payload_value_diffs={
                    "pi_dash.bgtasks.logger_task.process_logs": [
                        ("kwargs/mongo_log/created_by", "PIDASHCONV-937",
                         "{member}", None),
                        ("kwargs/mongo_log/updated_by", "PIDASHCONV-937",
                         "{member}", None),
                    ],
                },
            ),
            Request(
                "GET", "/api/v1/auth/workspaces/",
                route="/api/v1/auth/workspaces/",
                headers={"X-Api-Key": "{machine_token}"},
                auth="anonymous", expect_status=200,
                # PIDASHCONV-937: mongo user stamps (the device-flow
                # member id vs null; the bogus/revoked 403 legs agree on
                # null and carry no pin).
                payload_value_diffs={
                    "pi_dash.bgtasks.logger_task.process_logs": [
                        ("kwargs/mongo_log/created_by", "PIDASHCONV-937",
                         "{member}", None),
                        ("kwargs/mongo_log/updated_by", "PIDASHCONV-937",
                         "{member}", None),
                    ],
                },
            ),
            Request(
                "POST", "/api/v1/auth/machine-token/",
                route="/api/v1/auth/machine-token/",
                json={
                    "workspace_slug": "{ws}",
                    "dev_machine_id": DEV_MACHINE_ID,
                    "host_label": "replay-macbook",
                },
                headers={"X-Api-Key": "bogus"},
                auth="anonymous", expect_status=403,
            ),
            # -- revoke kills the machine token --
            Request(
                "POST", "/api/v1/auth/revoke/",
                route="/api/v1/auth/revoke/",
                headers={"X-Api-Key": "{machine_token}"},
                auth="anonymous", expect_status=200,
                # PIDASHCONV-937: mongo user stamps (the device-flow
                # member id vs null; the bogus/revoked 403 legs agree on
                # null and carry no pin).
                payload_value_diffs={
                    "pi_dash.bgtasks.logger_task.process_logs": [
                        ("kwargs/mongo_log/created_by", "PIDASHCONV-937",
                         "{member}", None),
                        ("kwargs/mongo_log/updated_by", "PIDASHCONV-937",
                         "{member}", None),
                    ],
                },
            ),
            Request(
                "GET", "/api/v1/auth/workspaces/",
                route="/api/v1/auth/workspaces/",
                headers={"X-Api-Key": "{machine_token}"},
                auth="anonymous", expect_status=403,
            ),
            # -- the consumed grant can never be polled again --
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code}"},
                auth="anonymous", expect_status=400,
            ),
            # -- token error branches --
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "no-such-device-code"},
                auth="anonymous", expect_status=400,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": EXPIRED_DC},
                auth="anonymous", expect_status=410,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": DENIED_DC},
                auth="anonymous", expect_status=410,
            ),
            # -- approve error branches --
            Request(
                "POST", "/api/v1/auth/device/approve/",
                route="/api/v1/auth/device/approve/",
                json={"user_code": "ZZZZ-9999"},
                auth="member", expect_status=404,
            ),
            Request(
                "POST", "/api/v1/auth/device/start/",
                route="/api/v1/auth/device/start/",
                json={},
                auth="anonymous",
                capture={"device_code2": "device_code", "user_code2": "user_code"},
                expect_status=200,
            ),
            Request(
                "POST", "/api/v1/auth/device/approve/",
                route="/api/v1/auth/device/approve/",
                json={"user_code": "{user_code2}"},
                auth="anonymous", expect_status=401,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code2}"},
                auth="anonymous", expect_status=400,
            ),
        ),
    ),
)
