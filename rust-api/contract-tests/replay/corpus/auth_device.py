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


def _seed_engine(conn) -> None:
    """The engine-grant machine: DEV_MACHINE_ID owned by the approver.

    The engine-grant session names DEV_MACHINE_ID at start and approve;
    Django's _check_engine_grant_approval 404s dev_machine_not_found
    when the machine is absent (NEWPIDASH-58, run 42227149766). Mirrors
    _harness create_dev_machine with a fixed id.
    """
    from psycopg.rows import dict_row

    with conn.cursor(row_factory=dict_row) as cur:
        cur.execute(
            "SELECT id FROM users WHERE email = 'replay-member@example.com'"
        )
        member_id = str(cur.fetchone()["id"])
    with conn.cursor() as cur:
        cur.execute(
            """
            INSERT INTO dev_machine
                (id, owner_id, host_label, label, visibility, provisioning,
                 created_at, updated_at)
            VALUES (%s, %s, 'replay-macbook', 'replay-macbook', 0, 'manual',
                    now(), now())
            """,
            (DEV_MACHINE_ID, member_id),
        )
    conn.commit()


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
            # expires_at is minted as now+60d on both backends
            # (runner/models.py save + Rust tokens mint, PIDESKAPP-122);
            # the replay sees two instants ms apart (NEWPIDASH-58).
            "machine_token": ["last_used_at", "revoked_at", "expires_at"],
        },
        # Evidence: cli_device_codes.api_token_id (db/models/api.py,
        # added PIDESKAPP-122) points at the api_tokens row this
        # backend minted on the token exchange (request 3): each
        # side's FK must exist in its own api_tokens table.
        db_cell_refs=[
            ("cli_device_codes", "api_token_id", "api_tokens", ""),
        ],
        # process_logs embeds two timezone.now() stamps per backend
        # (Django micros, Rust seconds): each side mints its own now,
        # so both blank via the explicit time map.
        payload_time_paths={
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        # process_logs now compares exactly (868/920/937 all landed); only the now() stamps above still blank.
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
            ),
            Request(
                "GET", "/api/v1/auth/workspaces/",
                route="/api/v1/auth/workspaces/",
                headers={"X-Api-Key": "{machine_token}"},
                auth="anonymous", expect_status=200,
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
    Session(
        name="auth_engine_grant",
        domain="v1_cli_auth",
        source="v1_cli_auth/test_engine_device_flow.py",
        auth_users=("member",),
        seed=_seed_engine,
        generated_columns={"api_tokens": ["label"], "machine_token": ["token_hash", "token_fingerprint"], "engine_credential": ["token_hash", "fingerprint"]},
        timestamp_columns={"cli_device_codes": ["expires_at", "last_polled_at"], "dev_machine": ["last_seen_at"], "machine_token": ["last_used_at", "revoked_at", "expires_at"], "engine_credential": ["created_at", "expires_at", "last_used_at", "revoked_at"]},
        # The cli-grant half mints an api_tokens row like
        # auth_device_lifecycle request 3; same existence check.
        db_cell_refs=[
            ("cli_device_codes", "api_token_id", "api_tokens", ""),
        ],
        # Same process_logs now() stamps as auth_device_lifecycle
        # above: Django mints micros, Rust seconds, so each side
        # blanks its own stamps (NEWPIDASH-58, run 42265262809 req7).
        payload_time_paths={
            "pi_dash.bgtasks.logger_task.process_logs": [
                "kwargs/mongo_log/created_at",
                "kwargs/mongo_log/updated_at",
            ],
        },
        requests=(
            # engine scope start with named machine
            Request(
                "POST", "/api/v1/auth/device/start/",
                route="/api/v1/auth/device/start/",
                json={"scope": "engine", "dev_machine_id": DEV_MACHINE_ID},
                auth="anonymous",
                capture={"device_code": "device_code", "user_code": "user_code"},
                expect_status=200,
            ),
            # invalid scope
            Request(
                "POST", "/api/v1/auth/device/start/",
                route="/api/v1/auth/device/start/",
                json={"scope": "bogus"},
                auth="anonymous", expect_status=400,
            ),
            # approve interactive
            Request(
                "POST", "/api/v1/auth/device/approve/",
                route="/api/v1/auth/device/approve/",
                json={"user_code": "{user_code}", "interactive": True, "dev_machine_id": DEV_MACHINE_ID},
                auth="member", expect_status=200,
            ),
            # pending poll without bearer (machine_token_required)
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code}"},
                auth="anonymous",
                expect_status=401,
            ),
            # machine-token exchange via bridge path already covered; here verify engine-sign-in-state present
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
                json={"user_code": "{user_code2}", "interactive": True},
                auth="member", expect_status=200,
            ),
            Request(
                "POST", "/api/v1/auth/device/token/",
                route="/api/v1/auth/device/token/",
                json={"device_code": "{device_code2}"},
                auth="anonymous",
                delay_before=4.0,
                capture={"bridge": "access_token"},
                expect_status=200,
            ),
            Request(
                "POST", "/api/v1/auth/machine-token/",
                route="/api/v1/auth/machine-token/",
                json={"workspace_slug": "{ws}", "dev_machine_id": DEV_MACHINE_ID, "host_label": "replay-macbook"},
                headers={"X-Api-Key": "{bridge}"},
                auth="anonymous",
                capture={"machine_token": "machine_token"},
                expect_status=201,
            ),
            Request(
                "POST", "/api/v1/auth/revoke/",
                route="/api/v1/auth/revoke/",
                headers={"X-Api-Key": "{machine_token}"},
                auth="anonymous", expect_status=200,
            ),
        ),
    ),
)
