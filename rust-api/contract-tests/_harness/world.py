"""SQL seed graph for contract suites: user → workspace → project → states → issue.

Every row is a plain INSERT with explicit columns — no ORM, no backend
imports. Column lists were taken from the migrated schema (all NOT NULL
columns without a database default). Timestamps are `now()`; uuids are
generated client-side so tests can reference them before insert.
"""

from __future__ import annotations

import secrets
import uuid
from dataclasses import dataclass, field

from . import db
from .db import get_database_url

ROLE_ADMIN, ROLE_MEMBER, ROLE_GUEST = 20, 15, 5


def _exec(sql: str, params: tuple = ()) -> None:
    # Merged db API takes (database_url, sql, params); this keeps the
    # call sites below to one database_url lookup each.
    db.execute(get_database_url(), sql, params)


def uid() -> str:
    return str(uuid.uuid4())


def slug(prefix: str) -> str:
    return f"{prefix}-{secrets.token_hex(4)}"


@dataclass
class World:
    user_id: str = ""
    api_key: str = ""
    workspace_id: str = ""
    workspace_slug: str = ""
    project_id: str = ""
    project_identifier: str = ""
    states: dict = field(default_factory=dict)  # name -> {"id":..., "group":...}
    issues: dict = field(default_factory=dict)  # alias -> issue id
    seq: int = 0


# -- primitives ------------------------------------------------------------

def make_user(username: str | None = None) -> str:
    user_id = uid()
    name = username or f"u-{secrets.token_hex(4)}@example.com"
    _exec(
        """INSERT INTO users (id, username, email, password, first_name, last_name,
               avatar, date_joined, created_at, updated_at, last_location,
               created_location, is_superuser, is_managed, is_password_expired,
               is_active, is_staff, is_email_verified, is_password_autoset, token,
               user_timezone, last_login_ip, last_logout_ip, last_login_medium,
               last_login_uagent, is_bot, display_name, is_email_valid,
               is_password_reset_required)
           VALUES (%s,%s,%s,%s,%s,%s,%s,now(),now(),now(),%s,%s,
                   false,false,false,true,false,false,false,%s,
                   %s,%s,%s,%s,%s,false,%s,false,false)""",
        (user_id, name, name, "!" + secrets.token_hex(20), name, "",
         "", "", "", "x" * 32, "UTC", "", "", "email", "", name),
    )
    return user_id


def make_token(user_id: str, workspace_id: str | None = None) -> str:
    token = "ct_" + secrets.token_hex(24)
    _exec(
        """INSERT INTO api_tokens (id, token, label, description, is_active,
               user_id, user_type, workspace_id, is_service, allowed_rate_limit,
               created_at, updated_at)
           VALUES (%s,%s,%s,%s,true,%s,0,%s,false,%s,now(),now())""",
        (uid(), token, "contract", "contract suite", user_id, workspace_id, "100000/minute"),
    )
    return token


def make_workspace(owner_id: str, slug_: str | None = None) -> tuple[str, str]:
    ws_id, ws_slug = uid(), slug_ or slug("ws")
    _exec(
        """INSERT INTO workspaces (id, name, slug, owner_id, timezone,
               background_color, created_at, updated_at)
           VALUES (%s,%s,%s,%s,'UTC','#ffffff',now(),now())""",
        (ws_id, ws_slug, ws_slug, owner_id),
    )
    return ws_id, ws_slug


def add_workspace_member(workspace_id: str, user_id: str, role: int = ROLE_ADMIN) -> None:
    _exec(
        """INSERT INTO workspace_members (id, role, member_id, workspace_id,
               view_props, default_props, issue_props, is_active,
               explored_features, getting_started_checklist, tips,
               created_at, updated_at)
           VALUES (%s,%s,%s,%s,'{}','{}','{}',true,'{}','{}','{}',now(),now())""",
        (uid(), role, user_id, workspace_id),
    )


def make_project(workspace_id: str, owner_id: str, identifier: str | None = None) -> tuple[str, str]:
    proj_id, ident = uid(), (identifier or f"P{secrets.token_hex(2).upper()}")
    _exec(
        """INSERT INTO projects (id, name, description, network, identifier,
               workspace_id, cycle_view, module_view, issue_views_view, page_view,
               intake_view, archive_in, close_in, logo_props,
               is_time_tracking_enabled, is_issue_type_enabled,
               guest_view_all_features, timezone, members_can_edit_states,
               repo_url, base_branch, agent_default_interval_seconds,
               agent_default_max_ticks, agent_ticking_enabled, is_default,
               agent_review_default_interval_seconds, default_agent_executor,
               agent_test_default_interval_seconds, created_at, updated_at)
           VALUES (%s,%s,'',0,%s,%s,true,true,true,true,true,365,365,'{}',
                   true,false,false,'UTC',true,'','',10800,10,true,false,
                   10800,'local_runner',10800,now(),now())""",
        (proj_id, f"proj-{ident}", ident, workspace_id),
    )
    return proj_id, ident


def add_project_member(project_id: str, workspace_id: str, user_id: str, role: int = ROLE_ADMIN) -> None:
    _exec(
        """INSERT INTO project_members (id, role, member_id, project_id,
               workspace_id, view_props, default_props, sort_order, preferences,
               is_active, created_at, updated_at)
           VALUES (%s,%s,%s,%s,%s,'{}','{}',0,'{}',true,now(),now())""",
        (uid(), role, user_id, project_id, workspace_id),
    )


def make_state(project_id: str, workspace_id: str, name: str, group: str,
               *, default: bool = False, sequence: float = 1.0) -> str:
    state_id = uid()
    _exec(
        """INSERT INTO states (id, name, description, color, slug, project_id,
               workspace_id, sequence, "group", "default", is_triage,
               created_at, updated_at)
           VALUES (%s,%s,'','#808080',%s,%s,%s,%s,%s,%s,false,now(),now())""",
        (state_id, name, name.lower().replace(" ", "-"), project_id,
         workspace_id, sequence, group, default),
    )
    return state_id


def make_issue(world: World, alias: str, state_name: str, name: str | None = None,
               creator_id: str | None = None) -> str:
    world.seq += 1
    issue_id = uid()
    state = world.states[state_name]
    creator = creator_id or world.user_id
    _exec(
        """INSERT INTO issues (id, name, description_json, priority, sequence_id,
               project_id, workspace_id, state_id, description_html, sort_order,
               is_draft, git_work_branch, workpad, complexity_score,
               created_by_id, updated_by_id, created_at, updated_at)
           VALUES (%s,%s,'{}','none',%s,%s,%s,%s,'',%s,false,'','',0,%s,%s,now(),now())""",
        (issue_id, name or f"issue-{alias}", world.seq, world.project_id,
         world.workspace_id, state["id"], float(world.seq), creator, creator),
    )
    world.issues[alias] = issue_id
    return issue_id


# -- ticker / runs (engine tables the oracle observes) ---------------------

def set_ticker(issue_id: str, **fields) -> None:
    """Upsert the issue's clock row. Only `fire_tick` writes `used` in prod;
    tests may set any column to arrange preconditions."""
    defaults = {
        "id": uid(), "issue_id": issue_id, "user_disabled": False, "used": 0,
        "enabled": True, "next_run_at": None, "last_tick_at": None,
        "disarm_reason": "", "granted": 0, "pending_entry": False,
        "pending_entry_free": False, "pending_entry_actor_id": None,
        "pending_entry_trigger": "", "resume_parent_run_id": None, "waited": 0,
    }
    defaults.update(fields)
    cols = ",".join(defaults)
    vals = ",".join(["%s"] * len(defaults))
    updates = ",".join(f"{c}=excluded.{c}" for c in defaults if c not in ("id", "issue_id"))
    _exec(
        f"""INSERT INTO issue_agent_ticker ({cols}, created_at, updated_at)
            VALUES ({vals}, now(), now())
            ON CONFLICT (issue_id) DO UPDATE SET {updates}, updated_at=now()""",
        tuple(defaults.values()),
    )


def make_run(issue_id: str, workspace_id: str, creator_id: str, pod_id: str,
             status: str = "running", **fields) -> str:
    """Insert an AgentRun row (e.g. an active run for guard tests)."""
    run_id = uid()
    row = {
        "id": run_id, "status": status, "prompt": "", "run_config": "{}",
        "required_capabilities": "{}",
        "thread_id": "", "error": "", "workspace_id": workspace_id,
        "pod_id": pod_id, "created_by_id": creator_id, "work_item_id": issue_id,
        "llm_model": "", "refusal_category": "", "trigger": "tick",
        "executor_kind": "local_runner", "dispatch_attempts": 0, "cancel_reason": "",
        "error_code": "", "tool_plan": "{}", "phase_kind": "",
        "agent_metadata": "{}", "usage": "{}",
    }
    row.update(fields)
    cols = ",".join(row)
    vals = ",".join(["%s"] * len(row))
    _exec(
        f"INSERT INTO agent_run ({cols}, created_at) VALUES ({vals}, now())",
        tuple(row.values()),
    )
    return run_id


def make_pod(workspace_id: str, project_id: str, name: str | None = None,
             *, is_default: bool = False) -> str:
    pod_id = uid()
    _exec(
        """INSERT INTO pod (id, name, description, is_default, workspace_id,
               project_id, created_at, updated_at)
           VALUES (%s,%s,'',%s,%s,%s,now(),now())""",
        (pod_id, name or f"pod-{secrets.token_hex(4)}", is_default,
         workspace_id, project_id),
    )
    return pod_id


# -- composite --------------------------------------------------------------

STANDARD_STATES = [
    ("Backlog", "backlog", True),
    ("Unstarted", "unstarted", False),
    ("In Progress", "started", False),
    ("In Review", "review", False),
    ("In Test", "test", False),
    ("Done", "completed", False),
]


def build(tag: str, *, member_role: int = ROLE_ADMIN) -> World:
    """Full seed graph: user + token + workspace + project + states.

    Names are suffixed with randomness so every test gets an isolated
    world even when the tag repeats."""
    w = World()
    uniq = secrets.token_hex(4)
    w.user_id = make_user(f"{tag}-{uniq}@example.com")
    w.workspace_id, w.workspace_slug = make_workspace(w.user_id, slug(f"ws-{tag}-{uniq}"))
    add_workspace_member(w.workspace_id, w.user_id, ROLE_ADMIN)
    w.project_id, w.project_identifier = make_project(w.workspace_id, w.user_id, f"T{secrets.token_hex(2).upper()}")
    add_project_member(w.project_id, w.workspace_id, w.user_id, member_role)
    w.api_key = make_token(w.user_id, w.workspace_id)
    seq = 0.0
    for name, group, default in STANDARD_STATES:
        seq += 1.0
        w.states[name] = {"id": make_state(w.project_id, w.workspace_id, name, group, default=default, sequence=seq), "group": group}
    _exec("UPDATE projects SET default_state_id=%s WHERE id=%s",
               (w.states["Backlog"]["id"], w.project_id))
    return w
