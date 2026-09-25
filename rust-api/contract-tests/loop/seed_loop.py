"""Loop-domain seeding: jobs, targets, preferences, LLM configs, instances.

Column lists mirror the Django schema (``loop_jobs``, ``loop_targets``,
``loop_user_preferences``, ``assistant_user_llm_config``, ``instances``,
``instance_admins``, ``assistant_thread`` / ``assistant_turn`` /
``assistant_message``). Rows are inserted with raw SQL, which bypasses model
``save()`` (no per-row user stamping, no validation): each test sees exactly
the rows it inserted, including values the HTTP API would reject (a
``MINUTELY`` job for the interval-label fallback, a numeric-string
``min_role`` echo check is done through HTTP instead).
"""

from __future__ import annotations

import datetime

from _harness.seed import new_id, now_iso

ADMIN = 20
MEMBER = 15
GUEST = 5


def hours_ago(n: float) -> str:
    return (datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(hours=n)).isoformat()


def hours_from_now(n: float) -> str:
    return (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=n)).isoformat()


def create_loop_job(
    conn,
    *,
    slug: str,
    public_name: str = "Job",
    name: str = "Admin name",
    prompt: str = "Do the thing.",
    min_role: int = MEMBER,
    enabled: bool = True,
    is_builtin: bool = False,
    rrule: str = "FREQ=DAILY;BYHOUR=3;BYMINUTE=0",
    dtstart: str | None = None,
) -> dict:
    jid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO loop_jobs (id, slug, name, public_name, public_description,
            prompt, min_role, enabled, is_builtin, dtstart, rrule, tzid,
            created_at, updated_at)
        VALUES (%s,%s,%s,%s,'seeded description',%s,%s,%s,%s,%s,%s,'UTC',%s,%s)""",
        (
            jid, slug, name, public_name, prompt, min_role, enabled,
            is_builtin, dtstart or hours_ago(25), rrule, now, now,
        ),
    )
    return {"id": jid, "slug": slug}


def create_target(
    conn,
    *,
    job_id: str,
    workspace_id: str,
    user_id: str,
    next_run_at: str | None = None,
    thread_id: str | None = None,
    last_run_id: str | None = None,
    last_skipped_at: str | None = None,
    last_skip_reason: str = "",
) -> dict:
    tid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO loop_targets (id, job_id, workspace_id, user_id, thread_id,
            next_run_at, last_run_id, last_skipped_at, last_skip_reason,
            created_at, updated_at)
        VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
        (
            tid, job_id, workspace_id, user_id, thread_id, next_run_at,
            last_run_id, last_skipped_at, last_skip_reason, now, now,
        ),
    )
    return {"id": tid}


def set_pref(conn, *, user_id: str, job_id: str | None, enabled: bool) -> str:
    pid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO loop_user_preferences (id, user_id, job_id, enabled,
            created_at, updated_at) VALUES (%s,%s,%s,%s,%s,%s)""",
        (pid, user_id, job_id, enabled, now, now),
    )
    return pid


def configure_llm(conn, *, user_id: str) -> None:
    """Give ``user_id`` usable LLM credentials (any non-null blob satisfies the
    eligibility check, which only tests ``api_key_encrypted IS NOT NULL``)."""
    now = now_iso()
    conn.execute(
        """INSERT INTO assistant_user_llm_config (user_id, provider_kind, base_url,
            model_name, api_key_encrypted, created_at, updated_at)
        VALUES (%s,'openai_compatible','https://api.example.com/v1','gpt-test',%s,%s,%s)""",
        (user_id, b"contract-test-seeded", now, now),
    )


def ensure_instance(conn, *, instance_id: str = "contract-test-instance") -> dict:
    """Return the shared instance row, making it the newest.

    ``InstanceAdminPermission`` checks ``Instance.objects.first()`` (ordered by
    ``-created_at``), so the suite keeps exactly one live instance row and
    re-touches it at session start; otherwise rows seeded by earlier runs
    would shadow it and every admin check would 403.
    """
    now = now_iso()
    row = conn.execute(
        "SELECT id FROM instances WHERE instance_id=%s", (instance_id,)
    ).fetchone()
    if row is None:
        iid = new_id()
        conn.execute(
            """INSERT INTO instances (id, instance_name, instance_id, current_version,
                edition, domain, last_checked_at, is_telemetry_enabled,
                is_support_required, is_setup_done, is_signup_screen_visited,
                is_verified, is_test, is_current_version_deprecated,
                created_at, updated_at)
            VALUES (%s,'contract-tests',%s,'1.0.0','PI_DASH_COMMUNITY','',%s,
                true,true,false,false,false,false,false,%s,%s)""",
            (iid, instance_id, now, now, now),
        )
        return {"id": iid}
    iid = str(row[0])
    conn.execute(
        "UPDATE instances SET created_at=%s, updated_at=%s WHERE id=%s", (now, now, iid)
    )
    return {"id": iid}


def make_instance_admin(conn, *, instance_id: str, user_id: str, role: int = ADMIN) -> str:
    aid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO instance_admins (id, user_id, instance_id, role, is_verified,
            created_at, updated_at) VALUES (%s,%s,%s,%s,true,%s,%s)""",
        (aid, user_id, instance_id, role, now, now),
    )
    return aid


def create_thread(
    conn, *, workspace_id: str, user_id: str, kind: str = "loop", title: str = "t",
    active_turn_id: str | None = None, archived: bool = False,
) -> dict:
    tid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO assistant_thread (id, workspace_id, user_id, title, kind,
            is_archived, active_turn_id, created_at, updated_at)
        VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s)""",
        (tid, workspace_id, user_id, title, kind, archived, active_turn_id, now, now),
    )
    return {"id": tid}


def create_turn(
    conn,
    *,
    thread_id: str,
    status: str = "queued",
    usage_total_tokens: int | None = None,
    model_used: str = "",
    error_code: str = "",
    completed_at: str | None = None,
) -> dict:
    tid = new_id()
    now = now_iso()
    import json as _json

    usage = None if usage_total_tokens is None else _json.dumps({"total_tokens": usage_total_tokens})
    conn.execute(
        """INSERT INTO assistant_turn (id, thread_id, status, usage, model_used,
            error_code, error_detail, completed_at, created_at, started_at)
        VALUES (%s,%s,%s,%s,%s,%s,'',%s,%s,%s)""",
        (tid, thread_id, status, usage, model_used, error_code, completed_at, now, now),
    )
    return {"id": tid}


def create_message(
    conn, *, thread_id: str, seq: int = 0, kind: str = "assistant",
    display_content: str = "x", status: str = "completed",
) -> str:
    mid = new_id()
    now = now_iso()
    conn.execute(
        """INSERT INTO assistant_message (id, thread_id, seq, kind, display_content,
            payload, status, created_at) VALUES (%s,%s,%s,%s,%s,'{}',%s,%s)""",
        (mid, thread_id, seq, kind, display_content, status, now),
    )
    return mid
