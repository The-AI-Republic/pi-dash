"""Domain seeding for the D-21 oracle: intakes, workflow states, machine tokens.

Column lists mirror the Django schema (``intakes``, ``states``,
``machine_token``). ``projects`` rows come from ``_harness.seed`` (which sets
``intake_view=true``); tests that need intake disabled flip the flag with raw
SQL. Intake-issues and issues themselves are created through the HTTP API so
the suite exercises the real creation path.
"""

from __future__ import annotations

import hashlib
import hmac
import uuid

from _harness import config, seed


def create_intake(conn, *, workspace_id: str, project_id: str, created_by_id: str, name: str) -> dict:
    iid = seed.new_id()
    now = seed.now_iso()
    conn.execute(
        """INSERT INTO intakes (id, name, description, is_default, view_props,
            logo_props, created_by_id, project_id, workspace_id, created_at, updated_at)
        VALUES (%s,%s,'Seeded for contract tests',true,'{}','{}',%s,%s,%s,%s,%s)""",
        (iid, name, created_by_id, project_id, workspace_id, now, now),
    )
    return {"id": iid, "name": name}


def create_state(
    conn,
    *,
    workspace_id: str,
    project_id: str,
    name: str,
    group: str,
    default: bool = False,
    sequence: float = 10000,
) -> dict:
    """Insert a workflow ``states`` row (raw SQL bypasses signal-seeded states)."""
    sid = seed.new_id()
    now = seed.now_iso()
    conn.execute(
        """INSERT INTO states (id, name, description, color, slug, project_id,
            workspace_id, sequence, "group", "default", is_triage,
            created_at, updated_at)
        VALUES (%s,%s,'Seeded for contract tests','#4E5355',%s,%s,%s,%s,%s,%s,false,%s,%s)""",
        (sid, name, "ct-" + sid[:8], project_id, workspace_id, sequence, group, default, now, now),
    )
    return {"id": sid, "name": name}


def create_machine_token(
    conn,
    *,
    user_id: str,
    workspace_id: str,
    host_label: str = "contract-test-79",
    expires_at=None,
    last_used_at=None,
) -> str:
    """Insert a ``machine_token`` row; return the raw ``mt_`` bearer value.

    Hash is ``HMAC-SHA256("runner/pepper/" + SECRET_KEY, raw)``, mirroring
    ``pi_dash.runner.services.tokens`` without importing Django. Used for the
    installed-runner wire-compatibility case on the asset download endpoint.

    ``expires_at`` / ``last_used_at`` default to the column defaults (the DB
    ``now() + 60 days`` idle expiry, NULL); pass an ISO string to pin either
    (e.g. an expired token for PIDASHCONV-1714). ``host_label`` must be
    distinct per (user, workspace) unrevoked token because of the
    ``machine_token_one_active_per_user_ws_host`` partial unique index.
    """
    raw = "mt_" + uuid.uuid4().hex
    pepper = hashlib.sha256(("runner/pepper/" + config.secret_key()).encode()).digest()
    token_hash = hmac.new(pepper, raw.encode(), hashlib.sha256).hexdigest()
    conn.execute(
        """INSERT INTO machine_token (id, user_id, dev_machine_id, workspace_id,
            host_label, token_hash, token_fingerprint, label, is_service, created_at,
            expires_at, last_used_at)
        VALUES (%s,%s,NULL,%s,%s,%s,'ct79fp','contract test',true,%s,%s,%s)""",
        (
            seed.new_id(), user_id, workspace_id, host_label,
            token_hash, seed.now_iso(), expires_at, last_used_at,
        ),
    )
    return raw
